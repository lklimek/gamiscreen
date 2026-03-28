use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use gamiscreen_server::{server, storage};
use gamiscreen_shared::api;
use gamiscreen_shared::domain::{Child, Task};
use reqwest::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const LOGIN_PATH: &str = "/api/v1/auth/login";
const RENEW_PATH: &str = "/api/v1/auth/renew";
const TENANT_ID: &str = "test-tenant";

struct TestServer {
    base: String,
    client: Client,
    handle: tokio::task::JoinHandle<()>,
    _tempdir: tempfile::TempDir,
    db_path: PathBuf,
}

impl TestServer {
    async fn spawn() -> Option<Self> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let (addr, handle) = match start_server(&db_path).await {
            Ok(v) => v,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                eprintln!("Skipping test due to sandbox restrictions: {e}");
                return None;
            }
            Err(e) => panic!("failed to start server: {e}"),
        };
        Some(Self {
            base: format!("http://{}", addr),
            client: Client::new(),
            handle,
            db_path: db_path.clone(),
            _tempdir: dir,
        })
    }

    async fn login(&self, username: &str, password: &str) -> String {
        let body: api::AuthResp = self
            .request_expect_json(
                "POST",
                LOGIN_PATH,
                None,
                Some(to_value(&api::AuthReq {
                    username: username.to_string(),
                    password: password.to_string(),
                })),
                StatusCode::OK,
            )
            .await;
        body.token
    }

    async fn request_raw(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Vec<u8>) {
        let url = format!("{}{}", self.base, path);
        let mut req = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            other => panic!("unsupported method {other}"),
        };
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap().to_vec();
        (status, bytes)
    }

    async fn request_expect_json<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) -> T {
        let (status, bytes) = self.request_raw(method, path, token, body).await;
        let body_text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {body_text}",
        );
        serde_json::from_slice::<T>(&bytes).unwrap_or_else(|e| {
            panic!("failed to deserialize response for {method} {path}: {e}; body: {body_text}")
        })
    }

    async fn request_expect_status(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) {
        let (status, bytes) = self.request_raw(method, path, token, body).await;
        let body_text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {body_text}",
        );
    }

    async fn request_expect_text(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) -> String {
        let (status, bytes) = self.request_raw(method, path, token, body).await;
        let body_text = String::from_utf8_lossy(&bytes).to_string();
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {body_text}",
        );
        body_text
    }

    /// Helper to backdate a session's last_used_at field in the database.
    /// This is used to test session idle timeout behavior.
    async fn backdate_session(&self, jti: &str, days_ago: i64) {
        use diesel::prelude::*;
        let db_path_str = self.db_path.to_str().unwrap().to_string();
        let jti_str = jti.to_string();
        tokio::task::spawn_blocking(move || {
            let manager = diesel::r2d2::ConnectionManager::<SqliteConnection>::new(&db_path_str);
            let pool = diesel::r2d2::Pool::builder().build(manager).unwrap();
            let mut conn = pool.get().unwrap();

            let target_time = (Utc::now() - Duration::days(days_ago)).naive_utc();
            diesel::sql_query("UPDATE sessions SET last_used_at = ?1 WHERE jti = ?2")
                .bind::<diesel::sql_types::Timestamp, _>(target_time)
                .bind::<diesel::sql_types::Text, _>(&jti_str)
                .execute(&mut conn)
                .unwrap();
        })
        .await
        .unwrap();
    }

    /// Helper to get the current last_used_at for a session.
    async fn get_session_last_used(&self, jti: &str) -> Option<chrono::NaiveDateTime> {
        use diesel::prelude::*;
        let db_path_str = self.db_path.to_str().unwrap().to_string();
        let jti_str = jti.to_string();
        tokio::task::spawn_blocking(move || {
            let manager = diesel::r2d2::ConnectionManager::<SqliteConnection>::new(&db_path_str);
            let pool = diesel::r2d2::Pool::builder().build(manager).unwrap();
            let mut conn = pool.get().unwrap();

            #[derive(QueryableByName)]
            struct SessionTime {
                #[diesel(sql_type = diesel::sql_types::Timestamp)]
                last_used_at: chrono::NaiveDateTime,
            }

            diesel::sql_query("SELECT last_used_at FROM sessions WHERE jti = ?1")
                .bind::<diesel::sql_types::Text, _>(&jti_str)
                .get_result::<SessionTime>(&mut conn)
                .optional()
                .unwrap()
                .map(|s| s.last_used_at)
        })
        .await
        .unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn start_server(
    tmp_db: &Path,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let parent_pwd = "secret123";
    let child_pwd = "kidpass";
    let parent_hash = bcrypt::hash(parent_pwd, bcrypt::DEFAULT_COST).unwrap();
    let child_hash = bcrypt::hash(child_pwd, bcrypt::DEFAULT_COST).unwrap();
    let config = server::AppConfig {
        config_version: env!("CARGO_PKG_VERSION").to_string(),
        push: None,
        tenant_id: TENANT_ID.into(),
        children: vec![
            Child {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            Child {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ],
        tasks: vec![
            Task {
                id: "homework".into(),
                name: "Homework".into(),
                minutes: 2,
                required: false,
            },
            Task {
                id: "chores".into(),
                name: "Chores".into(),
                minutes: 1,
                required: false,
            },
        ],
        jwt_secret: "testsecret".into(),
        users: vec![
            server::UserConfig {
                username: "parent".into(),
                password_hash: parent_hash,
                role: server::Role::Parent,
                child_id: None,
            },
            server::UserConfig {
                username: "alice".into(),
                password_hash: child_hash,
                role: server::Role::Child,
                child_id: Some("alice".into()),
            },
        ],
        dev_cors_origin: None,
        listen_port: None,
    };

    let store = storage::Store::connect_sqlite(tmp_db.to_str().unwrap())
        .await
        .expect("db");
    store
        .seed_from_config(&config.children, &config.tasks)
        .await
        .expect("seed");

    let state = server::AppState::new(config, store);
    let app = server::router(state);

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Ok((addr, handle))
}

async fn start_server_with_tasks(
    tmp_db: &Path,
    tasks: Vec<Task>,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let parent_pwd = "secret123";
    let child_pwd = "kidpass";
    let parent_hash = bcrypt::hash(parent_pwd, bcrypt::DEFAULT_COST).unwrap();
    let child_hash = bcrypt::hash(child_pwd, bcrypt::DEFAULT_COST).unwrap();
    let config = server::AppConfig {
        config_version: env!("CARGO_PKG_VERSION").to_string(),
        push: None,
        tenant_id: TENANT_ID.into(),
        children: vec![
            Child {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            Child {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ],
        tasks,
        jwt_secret: "testsecret".into(),
        users: vec![
            server::UserConfig {
                username: "parent".into(),
                password_hash: parent_hash,
                role: server::Role::Parent,
                child_id: None,
            },
            server::UserConfig {
                username: "alice".into(),
                password_hash: child_hash,
                role: server::Role::Child,
                child_id: Some("alice".into()),
            },
        ],
        dev_cors_origin: None,
        listen_port: None,
    };

    let store = storage::Store::connect_sqlite(tmp_db.to_str().unwrap())
        .await
        .expect("db");
    store
        .seed_from_config(&config.children, &config.tasks)
        .await
        .expect("seed");

    let state = server::AppState::new(config, store);
    let app = server::router(state);

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Ok((addr, handle))
}

impl TestServer {
    async fn spawn_with_tasks(tasks: Vec<Task>) -> Option<Self> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let (addr, handle) = match start_server_with_tasks(&db_path, tasks).await {
            Ok(v) => v,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                eprintln!("Skipping test due to sandbox restrictions: {e}");
                return None;
            }
            Err(e) => panic!("failed to start server: {e}"),
        };
        Some(Self {
            base: format!("http://{}", addr),
            client: Client::new(),
            handle,
            db_path: db_path.clone(),
            _tempdir: dir,
        })
    }
}

fn tenant_path(suffix: &str) -> String {
    format!(
        "{}/{}",
        gamiscreen_shared::api::tenant_scope(TENANT_ID),
        suffix.trim_start_matches('/')
    )
}

fn now_minute() -> i64 {
    Utc::now().timestamp() / 60
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("failed to serialize test body")
}

/// Extract the JTI (JWT ID) from a JWT token for testing purposes.
fn extract_jti_from_token(token: &str) -> String {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // JWT tokens have 3 parts: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "Invalid JWT token format");

    // Decode the payload (second part)
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .expect("Failed to decode JWT payload");
    let payload_json: Value =
        serde_json::from_slice(&payload_bytes).expect("Failed to parse JWT payload");

    payload_json["jti"]
        .as_str()
        .expect("JTI not found in token")
        .to_string()
}

#[tokio::test]
async fn public_endpoints_work() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let health = server
        .request_expect_text("GET", "/healthz", None, None, StatusCode::OK)
        .await;
    assert_eq!(health, "ok");
    let version: api::VersionInfoDto = server
        .request_expect_json("GET", "/api/version", None, None, StatusCode::OK)
        .await;
    assert!(!version.version.is_empty());
    let version_v1: api::VersionInfoDto = server
        .request_expect_json("GET", "/api/v1/version", None, None, StatusCode::OK)
        .await;
    assert_eq!(version_v1.version, version.version);
    let token = server.login("parent", "secret123").await;
    assert!(!token.is_empty());
}

#[tokio::test]
async fn unauthenticated_requests_are_rejected() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let minute_ts = now_minute();
    let cases: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", tenant_path("children"), None),
        ("GET", tenant_path("tasks"), None),
        ("GET", tenant_path("notifications"), None),
        ("GET", tenant_path("notifications/count"), None),
        (
            "POST",
            tenant_path("notifications/task-submissions/1/approve"),
            None,
        ),
        (
            "POST",
            tenant_path("notifications/task-submissions/1/discard"),
            None,
        ),
        ("GET", tenant_path("children/alice/remaining"), None),
        (
            "POST",
            tenant_path("children/alice/reward"),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(1),
                description: None,
                is_borrowed: None,
            })),
        ),
        ("GET", tenant_path("children/alice/reward"), None),
        ("GET", tenant_path("children/alice/tasks"), None),
        (
            "POST",
            tenant_path("children/alice/register"),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: "dev1".to_string(),
            })),
        ),
        (
            "POST",
            tenant_path("children/alice/device/dev1/heartbeat"),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![minute_ts],
            })),
        ),
        (
            "POST",
            tenant_path("children/alice/tasks/homework/submit"),
            Some(to_value(&api::SubmitTaskReq {
                child_id: "alice".to_string(),
                task_id: "homework".to_string(),
            })),
        ),
    ];

    for (method, path, body) in cases.iter() {
        server
            .request_expect_status(method, path, None, body.clone(), StatusCode::UNAUTHORIZED)
            .await;
    }
}

#[tokio::test]
async fn token_renew_rotates_sessions() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let original = server.login("parent", "secret123").await;
    let renewed: api::AuthResp = server
        .request_expect_json("POST", RENEW_PATH, Some(&original), None, StatusCode::OK)
        .await;
    assert_ne!(renewed.token, original);

    let children_path = tenant_path("children");
    server
        .request_expect_status(
            "GET",
            &children_path,
            Some(&renewed.token),
            None,
            StatusCode::OK,
        )
        .await;

    server
        .request_expect_status(
            "GET",
            &children_path,
            Some(&original),
            None,
            StatusCode::UNAUTHORIZED,
        )
        .await;
}

#[tokio::test]
async fn parent_access_control() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent_token = server.login("parent", "secret123").await;
    let child_token = server.login("alice", "kidpass").await;

    let children: Vec<api::ChildDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("children"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(children.iter().any(|c| c.id == "alice"));

    let tasks: Vec<api::TaskDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("tasks"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(tasks.iter().any(|t| t.id == "homework"));

    let reward_body: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: Some("homework".to_string()),
                minutes: None,
                description: None,
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reward_body.remaining_minutes, 2);
    assert_eq!(reward_body.balance, 0); // no borrowing, account_balance = 0

    let rewards_list: Vec<api::RewardHistoryItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(!rewards_list.is_empty());

    let remaining: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining.child_id, "alice");
    assert_eq!(remaining.remaining_minutes, 2);
    assert_eq!(remaining.balance, 0); // no borrowing, account_balance = 0
    assert!(!remaining.blocked_by_tasks);

    let notifications: Vec<api::NotificationItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(notifications.is_empty());

    let count: api::NotificationsCountDto = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(count.count, 0);

    let child_tasks: Vec<api::TaskWithStatusDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/tasks"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(child_tasks.iter().any(|t| t.id == "homework"));

    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&parent_token),
            None,
            StatusCode::FORBIDDEN,
        )
        .await;

    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/device/dev1/heartbeat"),
            Some(&parent_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![now_minute()],
            })),
            StatusCode::FORBIDDEN,
        )
        .await;

    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&child_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications: Vec<api::NotificationItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    let submission_id = notifications[0].id;
    assert_eq!(notifications[0].task_id, "homework");
    assert_eq!(notifications[0].child_id, "alice");

    server
        .request_expect_status(
            "POST",
            &tenant_path(&format!(
                "notifications/task-submissions/{submission_id}/approve"
            )),
            Some(&parent_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let count: api::NotificationsCountDto = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(count.count, 0);

    let remaining_after_approve: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining_after_approve.remaining_minutes, 4);

    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&child_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications: Vec<api::NotificationItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    let discard_id = notifications[0].id;

    server
        .request_expect_status(
            "POST",
            &tenant_path(&format!(
                "notifications/task-submissions/{discard_id}/discard"
            )),
            Some(&parent_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications_after_discard: Vec<api::NotificationItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(notifications_after_discard.is_empty());

    let count_after_discard: api::NotificationsCountDto = server
        .request_expect_json(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(count_after_discard.count, 0);
}

#[tokio::test]
async fn child_access_control() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent_token = server.login("parent", "secret123").await;
    server
        .request_expect_json::<api::RewardResp>(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: Some("homework".to_string()),
                minutes: None,
                description: None,
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;

    let child_token = server.login("alice", "kidpass").await;

    server
        .request_expect_json::<Vec<api::TaskDto>>(
            "GET",
            &tenant_path("tasks"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;

    let child_tasks: Vec<api::TaskWithStatusDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/tasks"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(child_tasks.iter().any(|t| t.id == "homework"));

    let rewards: Vec<api::RewardHistoryItemDto> = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/reward"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(!rewards.is_empty());
    assert_eq!(rewards[0].minutes, 2);
    assert_eq!(rewards[0].description.as_deref().unwrap(), "Homework");

    let remaining: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining.remaining_minutes, 2);

    let register_resp: api::ClientRegisterResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/register"),
            Some(&child_token),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: "dev1".to_string(),
            })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(register_resp.child_id, "alice");
    assert_eq!(register_resp.device_id, "dev1");
    let device_token = register_resp.token.clone();

    let heartbeat: api::HeartbeatResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/device/dev1/heartbeat"),
            Some(&device_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![now_minute()],
            })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(heartbeat.remaining_minutes, 1);

    let remaining_after: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining_after.remaining_minutes, 1);

    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&child_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let minute_next = now_minute() + 1;
    let negative_cases: Vec<(&str, String, Option<Value>, Option<&str>)> = vec![
        ("GET", tenant_path("children"), None, Some(&child_token)),
        (
            "GET",
            tenant_path("notifications"),
            None,
            Some(&child_token),
        ),
        (
            "GET",
            tenant_path("notifications/count"),
            None,
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("notifications/task-submissions/1/approve"),
            None,
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("notifications/task-submissions/1/discard"),
            None,
            Some(&child_token),
        ),
        (
            "GET",
            tenant_path("children/bob/remaining"),
            None,
            Some(&child_token),
        ),
        (
            "GET",
            tenant_path("children/bob/reward"),
            None,
            Some(&child_token),
        ),
        (
            "GET",
            tenant_path("children/bob/tasks"),
            None,
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/bob/register"),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: "dev-bob".to_string(),
            })),
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/bob/device/dev99/heartbeat"),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![minute_next],
            })),
            Some(&device_token),
        ),
        (
            "POST",
            tenant_path("children/bob/tasks/homework/submit"),
            None,
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/alice/reward"),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(1),
                description: None,
                is_borrowed: None,
            })),
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/alice/device/dev1/heartbeat"),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![minute_next + 1],
            })),
            Some(&child_token),
        ),
    ];

    for (method, path, body, token) in negative_cases.iter() {
        server
            .request_expect_status(method, path, *token, body.clone(), StatusCode::FORBIDDEN)
            .await;
    }
}

#[tokio::test]
async fn user_session_idle_timeout() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };

    // Login as a user (parent)
    let token = server.login("parent", "secret123").await;
    let jti = extract_jti_from_token(&token);

    // Verify the session works initially
    server
        .request_expect_status(
            "GET",
            &tenant_path("children"),
            Some(&token),
            None,
            StatusCode::OK,
        )
        .await;

    // Backdate the session beyond the user idle timeout (14 days)
    server.backdate_session(&jti, 15).await;

    // Request should now be unauthorized due to idle timeout
    server
        .request_expect_status(
            "GET",
            &tenant_path("children"),
            Some(&token),
            None,
            StatusCode::UNAUTHORIZED,
        )
        .await;
}

#[tokio::test]
async fn device_session_idle_timeout() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };

    // Login as a child and register a device
    let child_token = server.login("alice", "kidpass").await;
    let register_resp: api::ClientRegisterResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/register"),
            Some(&child_token),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: "test-device".to_string(),
            })),
            StatusCode::OK,
        )
        .await;
    let device_token = register_resp.token;
    let jti = extract_jti_from_token(&device_token);

    // Verify the device session works initially
    server
        .request_expect_json::<api::HeartbeatResp>(
            "POST",
            &tenant_path("children/alice/device/test-device/heartbeat"),
            Some(&device_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![now_minute()],
            })),
            StatusCode::OK,
        )
        .await;

    // Backdate the session beyond the device idle timeout (30 days)
    server.backdate_session(&jti, 31).await;

    // Request should now be unauthorized due to idle timeout
    server
        .request_expect_status(
            "POST",
            &tenant_path("children/alice/device/test-device/heartbeat"),
            Some(&device_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![now_minute()],
            })),
            StatusCode::UNAUTHORIZED,
        )
        .await;
}

#[tokio::test]
async fn authenticated_request_advances_last_used_at_for_user() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };

    // Login as a user
    let token = server.login("parent", "secret123").await;
    let jti = extract_jti_from_token(&token);

    // Backdate the session to 10 days ago (within the 14-day idle timeout)
    server.backdate_session(&jti, 10).await;

    // Get the backdated timestamp
    let old_timestamp = server.get_session_last_used(&jti).await.unwrap();

    // Wait a moment to ensure timestamp difference is detectable
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Make an authenticated request
    server
        .request_expect_status(
            "GET",
            &tenant_path("children"),
            Some(&token),
            None,
            StatusCode::OK,
        )
        .await;

    // Verify that last_used_at was advanced
    let new_timestamp = server.get_session_last_used(&jti).await.unwrap();
    assert!(
        new_timestamp > old_timestamp,
        "last_used_at should be advanced after authenticated request. Old: {}, New: {}",
        old_timestamp,
        new_timestamp
    );

    // Verify the new timestamp is recent (within the last second)
    let now = Utc::now().naive_utc();
    let diff = now.signed_duration_since(new_timestamp);
    assert!(
        diff.num_seconds() < 2,
        "last_used_at should be very recent. Diff: {} seconds",
        diff.num_seconds()
    );
}

#[tokio::test]
async fn authenticated_request_advances_last_used_at_for_device() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };

    // Login as a child and register a device
    let child_token = server.login("alice", "kidpass").await;
    let register_resp: api::ClientRegisterResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/register"),
            Some(&child_token),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: "test-device".to_string(),
            })),
            StatusCode::OK,
        )
        .await;
    let device_token = register_resp.token;
    let jti = extract_jti_from_token(&device_token);

    // Backdate the session to 20 days ago (within the 30-day device idle timeout)
    server.backdate_session(&jti, 20).await;

    // Get the backdated timestamp
    let old_timestamp = server.get_session_last_used(&jti).await.unwrap();

    // Wait a moment to ensure timestamp difference is detectable
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Make an authenticated request
    server
        .request_expect_json::<api::HeartbeatResp>(
            "POST",
            &tenant_path("children/alice/device/test-device/heartbeat"),
            Some(&device_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: vec![now_minute()],
            })),
            StatusCode::OK,
        )
        .await;

    // Verify that last_used_at was advanced
    let new_timestamp = server.get_session_last_used(&jti).await.unwrap();
    assert!(
        new_timestamp > old_timestamp,
        "last_used_at should be advanced after authenticated request. Old: {}, New: {}",
        old_timestamp,
        new_timestamp
    );

    // Verify the new timestamp is recent (within the last second)
    let now = Utc::now().naive_utc();
    let diff = now.signed_duration_since(new_timestamp);
    assert!(
        diff.num_seconds() < 2,
        "last_used_at should be very recent. Diff: {} seconds",
        diff.num_seconds()
    );
}

#[tokio::test]
async fn login_rate_limit_enforced() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    // Exhaust the rate limit (default: 10 attempts per 60s window)
    for _ in 0..10 {
        let _ = server
            .request_raw(
                "POST",
                LOGIN_PATH,
                None,
                Some(to_value(&api::AuthReq {
                    username: "wrong".to_string(),
                    password: "wrong".to_string(),
                })),
            )
            .await;
    }
    // 11th attempt should be rate-limited, even with valid credentials
    server
        .request_expect_status(
            "POST",
            LOGIN_PATH,
            None,
            Some(to_value(&api::AuthReq {
                username: "parent".to_string(),
                password: "secret123".to_string(),
            })),
            StatusCode::TOO_MANY_REQUESTS,
        )
        .await;
}

#[tokio::test]
async fn test_borrowing_flow() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let token = server.login("parent", "secret123").await;

    // Step 1: Borrow 10 min for alice
    let borrow_resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(10),
                description: Some("Borrowed time".to_string()),
                is_borrowed: Some(true),
            })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(borrow_resp.remaining_minutes, 10);
    assert_eq!(borrow_resp.balance, -10); // account_balance = -10 (debt from borrowing)

    let remaining: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining.remaining_minutes, 10);
    assert_eq!(remaining.balance, -10);

    // Step 2: Earn 2 min via homework task (partial debt repay)
    let earn_resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: Some("homework".to_string()),
                minutes: None,
                description: None,
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    // homework = 2 min. account_balance was -10, all goes to repayment -> -8. Remaining unchanged.
    assert_eq!(earn_resp.remaining_minutes, 10);
    assert_eq!(earn_resp.balance, -8);

    // Step 3: Earn 8 min custom (full debt repay)
    let earn_resp2: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(8),
                description: Some("Extra time".to_string()),
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    // account_balance was -8, earn 8 -> exactly repays debt. Remaining unchanged.
    assert_eq!(earn_resp2.remaining_minutes, 10);
    assert_eq!(earn_resp2.balance, 0);

    // Step 4: Earn 5 more to verify positive (no-debt) earning works
    let earn_resp3: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(5),
                description: Some("Bonus".to_string()),
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    // No debt, full amount goes to remaining. account_balance stays 0.
    assert_eq!(earn_resp3.remaining_minutes, 15);
    assert_eq!(earn_resp3.balance, 0);
}

#[tokio::test]
async fn test_penalty_borrow_earn_remaining_converges_to_balance() {
    // Test: penalty -> borrow -> earn with new account_balance system.
    //
    // Sequence (starting from 0/0):
    //   +137 earn   -> remaining=137, account_balance=0
    //   -100 penalty -> remaining=37,  account_balance=0
    //   -40  penalty -> remaining=-3,  account_balance=0
    //   +10  borrow  -> remaining=7,   account_balance=-10
    //   +20  earn    -> remaining=17,  account_balance=0 (10 repays debt, 10 to remaining)
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let token = server.login("parent", "secret123").await;

    let grant = |minutes: i32, is_borrowed: bool| api::RewardReq {
        child_id: "alice".to_string(),
        task_id: None,
        minutes: Some(minutes),
        description: None,
        is_borrowed: Some(is_borrowed),
    };

    // +137 initial grant
    let resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&grant(137, false))),
            StatusCode::OK,
        )
        .await;
    assert_eq!(resp.remaining_minutes, 137);
    assert_eq!(resp.balance, 0); // no debt

    // -100 penalty (remaining=37, balance unchanged)
    let resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&grant(-100, false))),
            StatusCode::OK,
        )
        .await;
    assert_eq!(resp.remaining_minutes, 37);
    assert_eq!(resp.balance, 0); // penalties don't affect balance

    // -40 penalty -> remaining goes negative (remaining=-3, balance=0)
    let resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&grant(-40, false))),
            StatusCode::OK,
        )
        .await;
    assert_eq!(resp.remaining_minutes, -3);
    assert_eq!(resp.balance, 0); // still no debt

    // +10 lend (borrow) -> remaining recovers, debt created
    let resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&grant(10, true))),
            StatusCode::OK,
        )
        .await;
    assert_eq!(resp.remaining_minutes, 7);
    assert_eq!(resp.balance, -10); // debt from borrowing

    // +20 earn -> 10 repays debt, 10 goes to remaining
    let resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&grant(20, false))),
            StatusCode::OK,
        )
        .await;
    assert_eq!(resp.balance, 0); // debt fully repaid
    assert_eq!(
        resp.remaining_minutes, 17,
        "7 existing + 10 surplus after debt repayment"
    );
}

#[tokio::test]
async fn test_required_tasks_blocking() {
    let tasks = vec![
        Task {
            id: "homework".into(),
            name: "Homework".into(),
            minutes: 5,
            required: true,
        },
        Task {
            id: "chores".into(),
            name: "Chores".into(),
            minutes: 3,
            required: false,
        },
    ];
    let Some(server) = TestServer::spawn_with_tasks(tasks).await else {
        return;
    };
    let token = server.login("parent", "secret123").await;

    // Step 1: Grant alice 10 min custom reward
    let reward_resp: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: None,
                minutes: Some(10),
                description: Some("Custom".to_string()),
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    // Blocked by required tasks, so effective remaining = 0
    assert_eq!(reward_resp.remaining_minutes, 0);
    assert_eq!(reward_resp.balance, 0); // no borrowing, account_balance = 0

    // Step 2: Verify remaining shows blocked
    let remaining: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(remaining.remaining_minutes, 0);
    assert!(remaining.blocked_by_tasks);
    assert_eq!(remaining.balance, 0);

    // Step 3: Complete the required task via reward with task_id (triggers record_task_done)
    let task_reward: api::RewardResp = server
        .request_expect_json(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&token),
            Some(to_value(&api::RewardReq {
                child_id: "alice".to_string(),
                task_id: Some("homework".to_string()),
                minutes: None,
                description: None,
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await;
    // Now unblocked: remaining = 10 (custom) + 5 (homework) = 15
    assert!(task_reward.remaining_minutes > 0);
    assert_eq!(task_reward.remaining_minutes, 15);
    assert_eq!(task_reward.balance, 0); // no borrowing

    // Step 4: Verify remaining is now unblocked
    let remaining_after: api::RemainingDto = server
        .request_expect_json(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(remaining_after.remaining_minutes > 0);
    assert!(!remaining_after.blocked_by_tasks);
}

// ---------------------------------------------------------------------------
// Shared helpers for scenario tests
// ---------------------------------------------------------------------------

async fn parent_reward(
    server: &TestServer,
    token: &str,
    child_id: &str,
    req: &api::RewardReq,
) -> api::RewardResp {
    server
        .request_expect_json(
            "POST",
            &tenant_path(&format!("children/{child_id}/reward")),
            Some(token),
            Some(to_value(req)),
            StatusCode::OK,
        )
        .await
}

async fn get_remaining(server: &TestServer, token: &str, child_id: &str) -> api::RemainingDto {
    server
        .request_expect_json(
            "GET",
            &tenant_path(&format!("children/{child_id}/remaining")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
}

async fn register_device(
    server: &TestServer,
    child_token: &str,
    child_id: &str,
    device_id: &str,
) -> api::ClientRegisterResp {
    server
        .request_expect_json(
            "POST",
            &tenant_path(&format!("children/{child_id}/register")),
            Some(child_token),
            Some(to_value(&api::ClientRegisterReq {
                child_id: None,
                device_id: device_id.to_string(),
            })),
            StatusCode::OK,
        )
        .await
}

async fn send_heartbeat(
    server: &TestServer,
    device_token: &str,
    child_id: &str,
    device_id: &str,
    minutes: &[i64],
) -> api::HeartbeatResp {
    server
        .request_expect_json(
            "POST",
            &tenant_path(&format!("children/{child_id}/device/{device_id}/heartbeat")),
            Some(device_token),
            Some(to_value(&api::HeartbeatReq {
                minutes: minutes.to_vec(),
            })),
            StatusCode::OK,
        )
        .await
}

async fn get_reward_history(
    server: &TestServer,
    token: &str,
    child_id: &str,
) -> Vec<api::RewardHistoryItemDto> {
    server
        .request_expect_json(
            "GET",
            &tenant_path(&format!("children/{child_id}/reward")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
}

fn reward_req(
    child_id: &str,
    task_id: Option<&str>,
    minutes: Option<i32>,
    description: Option<&str>,
    is_borrowed: Option<bool>,
) -> api::RewardReq {
    api::RewardReq {
        child_id: child_id.to_string(),
        task_id: task_id.map(String::from),
        minutes,
        description: description.map(String::from),
        is_borrowed,
    }
}

// ---------------------------------------------------------------------------
// 12 real-life usage scenario tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_fresh_start_normal_day() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;
    let child = server.login("alice", "kidpass").await;

    // Fresh start: zero everything
    let rem = get_remaining(&server, &parent, "alice").await;
    assert_eq!(rem.remaining_minutes, 0);
    assert_eq!(rem.balance, 0);
    assert!(!rem.blocked_by_tasks);

    // Reward homework (task config = 2 min)
    let resp = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;
    assert_eq!(resp.remaining_minutes, 2);
    assert_eq!(resp.balance, 0); // no borrowing

    // Register device and send heartbeat for 2 past minutes
    let dev = register_device(&server, &child, "alice", "laptop1").await;
    let m = now_minute();
    let hb = send_heartbeat(&server, &dev.token, "alice", "laptop1", &[m - 2, m - 1]).await;
    assert_eq!(hb.remaining_minutes, 0);
    assert_eq!(hb.balance, 0); // usage doesn't affect account_balance
}

#[tokio::test]
async fn test_scenario_multiple_rewards_stack() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Homework = 2 min
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;
    assert_eq!(r1.remaining_minutes, 2);
    assert_eq!(r1.balance, 0); // no borrowing

    // Chores = 1 min, stacks on top
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("chores"), None, None, None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 3);
    assert_eq!(r2.balance, 0); // still no borrowing
}

#[tokio::test]
async fn test_scenario_borrowing_time() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;
    let child = server.login("alice", "kidpass").await;

    // Borrow 20
    let resp = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(20), Some("Borrow"), Some(true)),
    )
    .await;
    assert_eq!(resp.remaining_minutes, 20);
    assert_eq!(resp.balance, -20); // debt from borrowing

    // Use 10 minutes via heartbeat
    let dev = register_device(&server, &child, "alice", "dev1").await;
    let m = now_minute();
    let timestamps: Vec<i64> = (1..=10).map(|i| m - i).collect();
    let hb = send_heartbeat(&server, &dev.token, "alice", "dev1", &timestamps).await;
    assert_eq!(hb.remaining_minutes, 10);
    assert_eq!(hb.balance, -20); // usage doesn't change account_balance
}

#[tokio::test]
async fn test_scenario_paying_off_debt() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Step 1: Borrow 20
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(20), Some("Borrow"), Some(true)),
    )
    .await;
    assert_eq!(r1.remaining_minutes, 20);
    assert_eq!(r1.balance, -20); // debt from borrowing

    // Step 2: Earn homework (2 min) — all goes to debt repayment
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 20); // unchanged, all to repayment
    assert_eq!(r2.balance, -18); // -20 + 2 repaid

    // Step 3: Earn 25 custom — 18 repays remaining debt, 7 surplus to remaining
    let r3 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(25), Some("Big task"), None),
    )
    .await;
    assert_eq!(r3.balance, 0); // debt fully repaid
    assert_eq!(r3.remaining_minutes, 27); // 20 + 7 surplus
}

#[tokio::test]
async fn test_scenario_borrow_while_in_debt() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Step 1: Borrow 10
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(10), None, Some(true)),
    )
    .await;
    assert_eq!(r1.remaining_minutes, 10);
    assert_eq!(r1.balance, -10); // debt from borrowing

    // Step 2: Earn 5 custom — partial debt repay (account_balance: -10+5=-5)
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(5), Some("Partial"), None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 10); // unchanged, all to repayment
    assert_eq!(r2.balance, -5); // debt partially repaid

    // Step 3: Borrow 15 more while still in debt
    let r3 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(15), None, Some(true)),
    )
    .await;
    assert_eq!(r3.remaining_minutes, 25);
    assert_eq!(r3.balance, -20); // -5 existing + -15 new borrowing
}

#[tokio::test]
async fn test_scenario_required_tasks_blocking() {
    let tasks = vec![
        Task {
            id: "homework".into(),
            name: "Homework".into(),
            minutes: 2,
            required: true,
        },
        Task {
            id: "chores".into(),
            name: "Chores".into(),
            minutes: 1,
            required: false,
        },
    ];
    let Some(server) = TestServer::spawn_with_tasks(tasks).await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Custom reward while required task not done
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(30), Some("Custom"), None),
    )
    .await;
    assert_eq!(r1.remaining_minutes, 0);
    assert_eq!(r1.balance, 0); // no borrowing

    let rem = get_remaining(&server, &parent, "alice").await;
    assert!(rem.blocked_by_tasks);
    assert_eq!(rem.remaining_minutes, 0);
    assert_eq!(rem.balance, 0);

    // Complete required task — unblocks and adds 2 min
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 32);
    assert_eq!(r2.balance, 0); // no borrowing

    let rem2 = get_remaining(&server, &parent, "alice").await;
    assert!(!rem2.blocked_by_tasks);
}

#[tokio::test]
async fn test_scenario_penalty() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Earn 20 custom
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(20), Some("Earned"), None),
    )
    .await;
    assert_eq!(r1.remaining_minutes, 20);
    assert_eq!(r1.balance, 0); // no borrowing

    // Penalty: -10
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(-10), Some("Penalty"), None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 10);
    assert_eq!(r2.balance, 0); // penalties don't affect balance
}

#[tokio::test]
async fn test_scenario_custom_reward_no_task() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Custom reward with description, no task_id
    let resp = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(15), Some("Helped with groceries"), None),
    )
    .await;
    assert_eq!(resp.remaining_minutes, 15);
    assert_eq!(resp.balance, 0); // no borrowing

    // Check reward history
    let history = get_reward_history(&server, &parent, "alice").await;
    assert!(!history.is_empty());
    let entry = &history[0];
    assert_eq!(entry.minutes, 15);
    assert_eq!(
        entry.description.as_deref().unwrap(),
        "Helped with groceries"
    );
    assert!(!entry.is_borrowed);
}

#[tokio::test]
async fn test_scenario_children_independent() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Borrow 10 for alice
    parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(10), None, Some(true)),
    )
    .await;

    // Earn 20 for bob
    parent_reward(
        &server,
        &parent,
        "bob",
        &reward_req("bob", None, Some(20), Some("Great work"), None),
    )
    .await;

    // Verify alice: 10 remaining, -10 account_balance (debt from borrowing)
    let alice = get_remaining(&server, &parent, "alice").await;
    assert_eq!(alice.remaining_minutes, 10);
    assert_eq!(alice.balance, -10);

    // Verify bob: 20 remaining, 0 account_balance (no borrowing)
    let bob = get_remaining(&server, &parent, "bob").await;
    assert_eq!(bob.remaining_minutes, 20);
    assert_eq!(bob.balance, 0);
}

#[tokio::test]
async fn test_scenario_heartbeat_idempotent() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;
    let child = server.login("alice", "kidpass").await;

    // Earn 10 custom
    parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(10), Some("Study"), None),
    )
    .await;

    let dev = register_device(&server, &child, "alice", "pc1").await;
    let m = now_minute() - 5;

    // First heartbeat with minute m
    let hb1 = send_heartbeat(&server, &dev.token, "alice", "pc1", &[m]).await;
    assert_eq!(hb1.remaining_minutes, 9);

    // Replay same minute — idempotent
    let hb2 = send_heartbeat(&server, &dev.token, "alice", "pc1", &[m]).await;
    assert_eq!(hb2.remaining_minutes, 9);

    // Send m again plus m+1 — only m+1 is new
    let hb3 = send_heartbeat(&server, &dev.token, "alice", "pc1", &[m, m + 1]).await;
    assert_eq!(hb3.remaining_minutes, 8);
}

#[tokio::test]
async fn test_scenario_required_task_plus_borrowing() {
    let tasks = vec![
        Task {
            id: "homework".into(),
            name: "Homework".into(),
            minutes: 2,
            required: true,
        },
        Task {
            id: "chores".into(),
            name: "Chores".into(),
            minutes: 1,
            required: false,
        },
    ];
    let Some(server) = TestServer::spawn_with_tasks(tasks).await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Borrow 30 — blocked by required task
    let r1 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(30), None, Some(true)),
    )
    .await;
    assert_eq!(r1.balance, -30); // debt from borrowing

    let rem = get_remaining(&server, &parent, "alice").await;
    assert_eq!(rem.remaining_minutes, 0);
    assert!(rem.blocked_by_tasks);
    assert_eq!(rem.balance, -30);

    // Complete required task — unblocks, 2 min goes to debt repayment
    let r2 = parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;
    assert_eq!(r2.remaining_minutes, 30); // unchanged, all to repayment
    assert!(
        !get_remaining(&server, &parent, "alice")
            .await
            .blocked_by_tasks
    );
    assert_eq!(r2.balance, -28); // -30 + 2 repaid
}

#[tokio::test]
async fn test_scenario_reward_history_borrowed_vs_earned() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent = server.login("parent", "secret123").await;

    // Earned reward via homework task
    parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", Some("homework"), None, None, None),
    )
    .await;

    // Borrowed reward
    parent_reward(
        &server,
        &parent,
        "alice",
        &reward_req("alice", None, Some(20), Some("Borrowed time"), Some(true)),
    )
    .await;

    let history = get_reward_history(&server, &parent, "alice").await;
    assert_eq!(history.len(), 2);

    // With the new system, is_borrowed on reward rows is always false.
    // Debt is tracked in balance_transactions, not on reward rows.
    let borrowed = history.iter().find(|h| h.minutes == 20).unwrap();
    assert!(
        !borrowed.is_borrowed,
        "new system always sets is_borrowed=false on reward rows"
    );

    let earned = history.iter().find(|h| h.minutes == 2).unwrap();
    assert!(!earned.is_borrowed);
}
