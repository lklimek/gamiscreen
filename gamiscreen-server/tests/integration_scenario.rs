use axum::http::StatusCode;
use chrono::Utc;
use gamiscreen_server::{server, storage};
use gamiscreen_shared::domain::{Child, Task};
use reqwest::Client;
use serde_json::{Value, json};
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::Path;

const LOGIN_PATH: &str = "/api/v1/auth/login";
const TENANT_ID: &str = "test-tenant";

struct TestServer {
    base: String,
    client: Client,
    handle: tokio::task::JoinHandle<()>,
    _tempdir: tempfile::TempDir,
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
            _tempdir: dir,
        })
    }

    async fn login(&self, username: &str, password: &str) -> String {
        let body = self
            .request_expect(
                "POST",
                LOGIN_PATH,
                None,
                Some(json!({"username": username, "password": password})),
                StatusCode::OK,
            )
            .await;
        body.get("token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .expect("token missing from auth response")
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        let text = resp.text().await.unwrap();
        let val = if text.is_empty() {
            json!(null)
        } else {
            serde_json::from_str(&text).unwrap_or(json!({"raw": text}))
        };
        (status, val)
    }

    async fn request_expect(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) -> Value {
        let (status, value) = self.request(method, path, token, body).await;
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {value:?}",
        );
        value
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

#[tokio::test]
async fn public_endpoints_work() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    server
        .request_expect("GET", "/healthz", None, None, StatusCode::OK)
        .await;
    let version = server
        .request_expect("GET", "/api/version", None, None, StatusCode::OK)
        .await;
    assert!(version.get("version").and_then(|v| v.as_str()).is_some());
    server
        .request_expect("GET", "/api/v1/version", None, None, StatusCode::OK)
        .await;
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
            Some(json!({"child_id":"alice","minutes":1})),
        ),
        ("GET", tenant_path("children/alice/reward"), None),
        ("GET", tenant_path("children/alice/tasks"), None),
        (
            "POST",
            tenant_path("children/alice/register"),
            Some(json!({"device_id":"dev1"})),
        ),
        (
            "POST",
            tenant_path("children/alice/device/dev1/heartbeat"),
            Some(json!({"minutes": [minute_ts]})),
        ),
        (
            "POST",
            tenant_path("children/alice/tasks/homework/submit"),
            None,
        ),
    ];

    for (method, path, body) in cases.iter() {
        server
            .request_expect(method, path, None, body.clone(), StatusCode::UNAUTHORIZED)
            .await;
    }
}

#[tokio::test]
async fn parent_access_control() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent_token = server.login("parent", "secret123").await;
    let child_token = server.login("alice", "kidpass").await;

    let children = server
        .request_expect(
            "GET",
            &tenant_path("children"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(
        children
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.get("id").unwrap() == "alice")
    );

    let tasks = server
        .request_expect(
            "GET",
            &tenant_path("tasks"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id").unwrap() == "homework")
    );

    let reward_body = server
        .request_expect(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            Some(json!({"child_id":"alice","task_id":"homework"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        reward_body
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        2
    );

    let rewards_list = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(!rewards_list.as_array().unwrap().is_empty());

    let remaining = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        remaining
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        2
    );
    assert_eq!(
        remaining.get("child_id").and_then(|v| v.as_str()).unwrap(),
        "alice"
    );

    let notifications = server
        .request_expect(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(notifications.as_array().unwrap().is_empty());

    let count = server
        .request_expect(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(count.get("count").unwrap().as_u64().unwrap(), 0);

    let child_tasks = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/tasks"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(
        child_tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id").unwrap() == "homework")
    );

    server
        .request_expect(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&parent_token),
            None,
            StatusCode::FORBIDDEN,
        )
        .await;

    server
        .request_expect(
            "POST",
            &tenant_path("children/alice/device/dev1/heartbeat"),
            Some(&parent_token),
            Some(json!({"minutes": [now_minute()]})),
            StatusCode::FORBIDDEN,
        )
        .await;

    server
        .request_expect(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&child_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications = server
        .request_expect(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    let submission_id = notifications.as_array().unwrap()[0]
        .get("id")
        .and_then(|v| v.as_i64())
        .unwrap() as i32;
    assert_eq!(
        notifications.as_array().unwrap()[0]
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap(),
        "homework"
    );
    assert_eq!(
        notifications.as_array().unwrap()[0]
            .get("child_id")
            .and_then(|v| v.as_str())
            .unwrap(),
        "alice"
    );

    server
        .request_expect(
            "POST",
            &tenant_path(&format!(
                "notifications/task-submissions/{submission_id}/approve"
            )),
            Some(&parent_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let count = server
        .request_expect(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(count.get("count").unwrap().as_u64().unwrap(), 0);

    let remaining_after_approve = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        remaining_after_approve
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        4
    );

    server
        .request_expect(
            "POST",
            &tenant_path("children/alice/tasks/homework/submit"),
            Some(&child_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications = server
        .request_expect(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    let discard_id = notifications.as_array().unwrap()[0]
        .get("id")
        .and_then(|v| v.as_i64())
        .unwrap() as i32;

    server
        .request_expect(
            "POST",
            &tenant_path(&format!(
                "notifications/task-submissions/{discard_id}/discard"
            )),
            Some(&parent_token),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;

    let notifications_after_discard = server
        .request_expect(
            "GET",
            &tenant_path("notifications"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(notifications_after_discard.as_array().unwrap().is_empty());

    let count_after_discard = server
        .request_expect(
            "GET",
            &tenant_path("notifications/count"),
            Some(&parent_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        count_after_discard.get("count").unwrap().as_u64().unwrap(),
        0
    );
}

#[tokio::test]
async fn child_access_control() {
    let Some(server) = TestServer::spawn().await else {
        return;
    };
    let parent_token = server.login("parent", "secret123").await;
    server
        .request_expect(
            "POST",
            &tenant_path("children/alice/reward"),
            Some(&parent_token),
            Some(json!({"child_id":"alice","task_id":"homework"})),
            StatusCode::OK,
        )
        .await;

    let child_token = server.login("alice", "kidpass").await;

    server
        .request_expect(
            "GET",
            &tenant_path("tasks"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;

    let child_tasks = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/tasks"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(
        child_tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id").unwrap() == "homework")
    );

    let rewards = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/reward"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(!rewards.as_array().unwrap().is_empty());
    let first_reward = &rewards.as_array().unwrap()[0];
    assert_eq!(
        first_reward
            .get("minutes")
            .and_then(|v| v.as_i64())
            .unwrap(),
        2
    );
    assert_eq!(
        first_reward
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap(),
        "Homework"
    );

    let remaining = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        remaining
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        2
    );

    let register_resp = server
        .request_expect(
            "POST",
            &tenant_path("children/alice/register"),
            Some(&child_token),
            Some(json!({"device_id":"dev1"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        register_resp
            .get("child_id")
            .and_then(|v| v.as_str())
            .unwrap(),
        "alice"
    );
    assert_eq!(
        register_resp
            .get("device_id")
            .and_then(|v| v.as_str())
            .unwrap(),
        "dev1"
    );
    let device_token = register_resp
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let heartbeat = server
        .request_expect(
            "POST",
            &tenant_path("children/alice/device/dev1/heartbeat"),
            Some(&device_token),
            Some(json!({"minutes":[now_minute()]})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        heartbeat
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        1
    );

    let remaining_after = server
        .request_expect(
            "GET",
            &tenant_path("children/alice/remaining"),
            Some(&child_token),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        remaining_after
            .get("remaining_minutes")
            .unwrap()
            .as_i64()
            .unwrap(),
        1
    );

    server
        .request_expect(
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
            Some(json!({"device_id":"dev-bob"})),
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/bob/device/dev99/heartbeat"),
            Some(json!({"minutes":[minute_next]})),
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
            Some(json!({"child_id":"alice","minutes":1})),
            Some(&child_token),
        ),
        (
            "POST",
            tenant_path("children/alice/device/dev1/heartbeat"),
            Some(json!({"minutes":[minute_next + 1]})),
            Some(&child_token),
        ),
    ];

    for (method, path, body, token) in negative_cases.iter() {
        server
            .request_expect(method, path, *token, body.clone(), StatusCode::FORBIDDEN)
            .await;
    }
}
