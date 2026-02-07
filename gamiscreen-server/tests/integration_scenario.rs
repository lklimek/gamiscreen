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
            },
            Task {
                id: "chores".into(),
                name: "Chores".into(),
                minutes: 1,
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
        axum::serve(listener, app).await.unwrap();
    });
    Ok((addr, handle))
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
            })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reward_body.remaining_minutes, 2);

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
