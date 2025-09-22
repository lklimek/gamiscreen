use axum::http::StatusCode;
use gamiscreen_server::{server, storage};
use gamiscreen_shared::domain::{Child, Task};
use reqwest::Client;
use serde_json::{Value, json};
use std::net::SocketAddr;

const LOGIN_PATH: &str = "/api/v1/auth/login";
const TENANT_ID: &str = "test-tenant";

const PORT: u16 = 51761;
async fn start_server(
    tmp_db: &std::path::Path,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    // Build config
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
        listen_port: Some(PORT),
    };

    // Connect DB
    let store = storage::Store::connect_sqlite(tmp_db.to_str().unwrap())
        .await
        .expect("db");
    // Seed
    store
        .seed_from_config(&config.children, &config.tasks)
        .await
        .expect("seed");

    let state = server::AppState::new(config, store);
    let app = server::router(state);

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, PORT)).await?;
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

async fn send_json(
    client: &Client,
    base: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let url = format!("{}{}", base, path);
    let mut req = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        other => panic!("unsupported method {other}"),
    };
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap();
    let text = resp.text().await.unwrap();
    let val = if text.is_empty() {
        json!(null)
    } else {
        serde_json::from_str(&text).unwrap_or(json!({"raw": text}))
    };
    (status, val)
}

#[tokio::test]
async fn end_to_end_scenario() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let (addr, handle) = match start_server(&db_path).await {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // In some sandboxes we can't bind to sockets; skip the test in that case.
            eprintln!("Skipping test due to sandbox restrictions: {e}");
            return;
        }
        Err(e) => panic!("failed to start server: {e}"),
    };
    let base = format!("http://{}", addr);
    let client = Client::new();

    // health
    let (st, _) = send_json(&client, &base, "GET", "/healthz", None, None).await;
    assert_eq!(st, StatusCode::OK);

    // parent login
    let (st, body) = send_json(
        &client,
        &base,
        "POST",
        LOGIN_PATH,
        None,
        Some(json!({"username":"parent","password":"secret123"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "login failed: {:?}", body);
    let parent_token = body
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // list children
    let children_path = tenant_path("children");
    let (st, body) = send_json(
        &client,
        &base,
        "GET",
        &children_path,
        Some(&parent_token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|c| c.get("id").unwrap() == "alice")
    );

    // list tasks
    let tasks_path = tenant_path("tasks");
    let (st, body) = send_json(
        &client,
        &base,
        "GET",
        &tasks_path,
        Some(&parent_token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|t| t.get("id").unwrap() == "homework")
    );

    // reward alice with homework (2 minutes)
    let child_reward_path = tenant_path("children/alice/reward");
    let (st, body) = send_json(
        &client,
        &base,
        "POST",
        &child_reward_path,
        Some(&parent_token),
        Some(json!({"child_id":"alice","task_id":"homework"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "reward failed: {:?}", body);
    let remaining = body.get("remaining_minutes").unwrap().as_i64().unwrap();
    assert_eq!(remaining, 2);

    // child login
    let (st, body) = send_json(
        &client,
        &base,
        "POST",
        LOGIN_PATH,
        None,
        Some(json!({"username":"alice","password":"kidpass"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "child login failed: {:?}", body);
    let child_token = body
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // heartbeat should decrement to 1
    let heartbeat_path = tenant_path("children/alice/device/dev1/heartbeat");
    let (st, body) = send_json(
        &client,
        &base,
        "POST",
        &heartbeat_path,
        Some(&child_token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "heartbeat failed: {:?}", body);
    assert_eq!(body.get("remaining_minutes").unwrap().as_i64().unwrap(), 1);

    // remaining via child
    let remaining_path = tenant_path("children/alice/remaining");
    let (st, body) = send_json(
        &client,
        &base,
        "GET",
        &remaining_path,
        Some(&child_token),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body.get("remaining_minutes").unwrap().as_i64().unwrap(), 1);

    handle.abort();
}
