//! Integration tests for Task Management (T-11).
//!
//! Covers CRUD, assignment, blocking logic with timezone, migration,
//! and enhanced child endpoint — per phase1c-test-cases.md.

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use chrono::{Datelike, Timelike};
use gamiscreen_server::{server, storage};
use gamiscreen_shared::api;
use gamiscreen_shared::domain::{Child, Task};
use reqwest::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TENANT_ID: &str = "test-tenant";

// ---------------------------------------------------------------------------
// Test server harness
// ---------------------------------------------------------------------------

struct TestServer {
    base: String,
    client: Client,
    handle: tokio::task::JoinHandle<()>,
    _tempdir: tempfile::TempDir,
    db_path: PathBuf,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Build config with given children, tasks, and timezone.
fn make_config(children: Vec<Child>, tasks: Vec<Task>, tz: chrono_tz::Tz) -> server::AppConfig {
    let parent_hash = bcrypt::hash("secret123", 4).unwrap(); // cost 4 for speed
    let child_hashes: Vec<_> = children
        .iter()
        .map(|c| (c.id.clone(), bcrypt::hash("kidpass", 4).unwrap()))
        .collect();

    let mut users = vec![server::UserConfig {
        username: "parent".into(),
        password_hash: parent_hash,
        role: server::Role::Parent,
        child_id: None,
    }];
    for (cid, hash) in &child_hashes {
        users.push(server::UserConfig {
            username: cid.clone(),
            password_hash: hash.clone(),
            role: server::Role::Child,
            child_id: Some(cid.clone()),
        });
    }

    server::AppConfig {
        config_version: env!("CARGO_PKG_VERSION").to_string(),
        push: None,
        tenant_id: TENANT_ID.into(),
        children,
        tasks,
        jwt_secret: "testsecret".into(),
        users,
        dev_cors_origin: None,
        listen_port: None,
        timezone: None,
        family_tz: tz,
    }
}

async fn start_with_config(
    tmp_db: &Path,
    config: server::AppConfig,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
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
    async fn spawn_custom(
        children: Vec<Child>,
        tasks: Vec<Task>,
        tz: chrono_tz::Tz,
    ) -> Option<Self> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let config = make_config(children, tasks, tz);
        let (addr, handle) = match start_with_config(&db_path, config).await {
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

    /// Standard 2-child, no-task server with UTC timezone.
    async fn spawn_default() -> Option<Self> {
        Self::spawn_custom(
            vec![
                Child {
                    id: "alice".into(),
                    display_name: "Alice".into(),
                },
                Child {
                    id: "bob".into(),
                    display_name: "Bob".into(),
                },
            ],
            vec![],
            chrono_tz::UTC,
        )
        .await
    }

    async fn login(&self, username: &str, password: &str) -> String {
        let body: api::AuthResp = self
            .json_request(
                "POST",
                "/api/v1/auth/login",
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

    async fn parent_token(&self) -> String {
        self.login("parent", "secret123").await
    }

    async fn child_token(&self, child_id: &str) -> String {
        self.login(child_id, "kidpass").await
    }

    async fn raw(
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
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
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

    async fn json_request<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) -> T {
        let (status, bytes) = self.raw(method, path, token, body).await;
        let body_text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {body_text}",
        );
        serde_json::from_slice::<T>(&bytes).unwrap_or_else(|e| {
            panic!("failed to deserialize response for {method} {path}: {e}; body: {body_text}")
        })
    }

    async fn expect_status(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        expected: StatusCode,
    ) {
        let (status, bytes) = self.raw(method, path, token, body).await;
        let body_text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            status, expected,
            "{method} {path} returned {status:?} with body {body_text}",
        );
    }

    // ----- Task helper methods -----

    async fn create_task(&self, token: &str, body: Value) -> api::TaskManagementDto {
        self.json_request(
            "POST",
            &tenant_path("tasks"),
            Some(token),
            Some(body),
            StatusCode::CREATED,
        )
        .await
    }

    async fn create_task_expect_error(&self, token: &str, body: Value, expected: StatusCode) {
        self.expect_status(
            "POST",
            &tenant_path("tasks"),
            Some(token),
            Some(body),
            expected,
        )
        .await;
    }

    async fn get_task(&self, token: &str, task_id: &str) -> api::TaskManagementDto {
        self.json_request(
            "GET",
            &tenant_path(&format!("tasks/{task_id}")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
    }

    async fn update_task(&self, token: &str, task_id: &str, body: Value) -> api::TaskManagementDto {
        self.json_request(
            "PUT",
            &tenant_path(&format!("tasks/{task_id}")),
            Some(token),
            Some(body),
            StatusCode::OK,
        )
        .await
    }

    async fn delete_task(&self, token: &str, task_id: &str) -> api::DeleteTaskResp {
        self.json_request(
            "DELETE",
            &tenant_path(&format!("tasks/{task_id}")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
    }

    async fn list_tasks(&self, token: &str) -> Vec<api::TaskManagementDto> {
        self.json_request(
            "GET",
            &tenant_path("tasks"),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
    }

    async fn list_child_tasks(&self, token: &str, child_id: &str) -> Vec<api::TaskWithStatusDto> {
        self.json_request(
            "GET",
            &tenant_path(&format!("children/{child_id}/tasks")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
    }

    async fn get_remaining(&self, token: &str, child_id: &str) -> api::RemainingDto {
        self.json_request(
            "GET",
            &tenant_path(&format!("children/{child_id}/remaining")),
            Some(token),
            None,
            StatusCode::OK,
        )
        .await
    }

    async fn reward_child(
        &self,
        token: &str,
        child_id: &str,
        task_id: Option<&str>,
        minutes: Option<i32>,
    ) -> api::RewardResp {
        self.json_request(
            "POST",
            &tenant_path(&format!("children/{child_id}/reward")),
            Some(token),
            Some(to_value(&api::RewardReq {
                child_id: child_id.to_string(),
                task_id: task_id.map(|s| s.to_string()),
                minutes,
                description: None,
                is_borrowed: None,
            })),
            StatusCode::OK,
        )
        .await
    }

    /// Direct DB access for verification.
    async fn run_sql<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut diesel::SqliteConnection) -> T + Send + 'static,
    ) -> T {
        use diesel::prelude::*;
        let db_path_str = self.db_path.to_str().unwrap().to_string();
        tokio::task::spawn_blocking(move || {
            let manager = diesel::r2d2::ConnectionManager::<SqliteConnection>::new(&db_path_str);
            let pool = diesel::r2d2::Pool::builder().build(manager).unwrap();
            let mut conn = pool.get().unwrap();
            f(&mut conn)
        })
        .await
        .unwrap()
    }
}

fn tenant_path(suffix: &str) -> String {
    format!(
        "{}/{}",
        gamiscreen_shared::api::tenant_scope(TENANT_ID),
        suffix.trim_start_matches('/')
    )
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("failed to serialize test body")
}

// ===========================================================================
// CRUD TESTS (TC-001 through TC-013)
// ===========================================================================

/// TC-001: Create task with minimal fields, verify defaults.
#[tokio::test]
async fn tc001_create_task_defaults() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 10}))
        .await;

    assert!(!task.id.is_empty(), "id should be non-empty");
    assert_eq!(task.name, "Brush teeth");
    assert_eq!(task.minutes, 10);
    assert_eq!(task.priority, 2, "default priority should be 2");
    assert_eq!(task.mandatory_days, 0, "default mandatory_days should be 0");
    assert!(
        task.assigned_children.is_none(),
        "no assignment => all children"
    );
    assert!(!task.created_at.is_empty());
    assert!(!task.updated_at.is_empty());
}

/// TC-002: Create task with all fields explicitly specified.
#[tokio::test]
async fn tc002_create_task_all_fields() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "priority": 1,
                "mandatory_days": 31,
                "mandatory_start_time": "15:00",
                "assigned_children": ["alice"]
            }),
        )
        .await;

    assert_eq!(task.priority, 1);
    assert_eq!(task.mandatory_days, 31);
    assert_eq!(task.mandatory_start_time.as_deref(), Some("15:00"));
    assert_eq!(task.assigned_children, Some(vec!["alice".to_string()]));
}

/// TC-003: Create task with empty name is rejected.
#[tokio::test]
async fn tc003_empty_name_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    s.create_task_expect_error(
        &t,
        json!({"name": "", "minutes": 10}),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

/// TC-004: Create task with zero minutes is rejected.
#[tokio::test]
async fn tc004_zero_minutes_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    s.create_task_expect_error(
        &t,
        json!({"name": "Test", "minutes": 0}),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

/// TC-005: Negative minutes (penalty task) allowed.
#[tokio::test]
async fn tc005_negative_minutes_allowed() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    let task = s
        .create_task(&t, json!({"name": "Penalty", "minutes": -15}))
        .await;
    assert_eq!(task.minutes, -15);
}

/// TC-006: Invalid priority values rejected.
#[tokio::test]
async fn tc006_invalid_priority_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    for p in [0, 4, -1] {
        s.create_task_expect_error(
            &t,
            json!({"name": "Test", "minutes": 10, "priority": p}),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
}

/// TC-007: Duplicate names allowed, different IDs.
#[tokio::test]
async fn tc007_duplicate_names_allowed() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let t1 = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 10}))
        .await;
    let t2 = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 5}))
        .await;
    assert_ne!(t1.id, t2.id, "duplicate names must have different IDs");
}

/// TC-008: Update task, changes persist immediately.
#[tokio::test]
async fn tc008_update_task() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let created = s
        .create_task(&t, json!({"name": "Homework", "minutes": 30}))
        .await;

    let updated = s
        .update_task(
            &t,
            &created.id,
            json!({"name": "Homework", "minutes": 45, "priority": 1}),
        )
        .await;
    assert_eq!(updated.minutes, 45);
    assert_eq!(updated.priority, 1);

    let fetched = s.get_task(&t, &created.id).await;
    assert_eq!(fetched.minutes, 45);
    assert_eq!(fetched.priority, 1);
    assert!(fetched.updated_at >= fetched.created_at);
}

/// TC-009: Name exceeding 100 characters rejected.
#[tokio::test]
async fn tc009_name_too_long_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let created = s
        .create_task(&t, json!({"name": "Test", "minutes": 10}))
        .await;

    let long_name = "x".repeat(101);
    s.expect_status(
        "PUT",
        &tenant_path(&format!("tasks/{}", created.id)),
        Some(&t),
        Some(json!({"name": long_name, "minutes": 10})),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

/// TC-010: Soft-delete removes from views but row persists.
#[tokio::test]
async fn tc010_soft_delete() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Summer reading", "minutes": 15}))
        .await;

    let resp = s.delete_task(&t, &task.id).await;
    assert!(resp.deleted);

    // GET /tasks should not include it
    let list = s.list_tasks(&t).await;
    assert!(
        list.iter().all(|tk| tk.id != task.id),
        "deleted task should not appear in list"
    );

    // GET /tasks/{id} should 404
    s.expect_status(
        "GET",
        &tenant_path(&format!("tasks/{}", task.id)),
        Some(&t),
        None,
        StatusCode::NOT_FOUND,
    )
    .await;

    // DB row should still exist with deleted_at set
    let task_id = task.id.clone();
    let exists = s
        .run_sql(move |conn| {
            use diesel::prelude::*;
            #[derive(QueryableByName)]
            struct Row {
                #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamp>)]
                deleted_at: Option<chrono::NaiveDateTime>,
            }
            let row: Option<Row> = diesel::sql_query("SELECT deleted_at FROM tasks WHERE id = ?1")
                .bind::<diesel::sql_types::Text, _>(&task_id)
                .get_result(conn)
                .ok();
            row.map(|r| r.deleted_at.is_some())
        })
        .await;
    assert_eq!(exists, Some(true), "task row should have deleted_at set");
}

/// TC-011: Delete non-existent task → 404.
#[tokio::test]
async fn tc011_delete_nonexistent() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    s.expect_status(
        "DELETE",
        &tenant_path("tasks/nonexistent-id-12345"),
        Some(&t),
        None,
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// TC-012: Completion history preserved after soft-delete.
#[tokio::test]
async fn tc012_completion_preserved_after_delete() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Summer reading", "minutes": 15}))
        .await;

    // Reward alice via task (creates completion)
    s.reward_child(&t, "alice", Some(&task.id), None).await;

    // Delete the task
    s.delete_task(&t, &task.id).await;

    // Verify completion still exists in DB
    let task_id = task.id.clone();
    let completion_count = s
        .run_sql(move |conn| {
            use diesel::prelude::*;
            #[derive(QueryableByName)]
            struct Count {
                #[diesel(sql_type = diesel::sql_types::BigInt)]
                cnt: i64,
            }
            let row: Count = diesel::sql_query(
                "SELECT COUNT(*) as cnt FROM task_completions WHERE task_id = ?1 AND child_id = 'alice'",
            )
            .bind::<diesel::sql_types::Text, _>(&task_id)
            .get_result(conn)
            .unwrap();
            row.cnt
        })
        .await;
    assert!(
        completion_count > 0,
        "completions should be preserved after soft-delete"
    );
}

/// TC-013: Child cannot create, update, or delete tasks.
#[tokio::test]
async fn tc013_child_forbidden() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let parent_t = s.parent_token().await;
    let child_t = s.child_token("alice").await;

    // Create a task as parent first
    let task = s
        .create_task(&parent_t, json!({"name": "Test", "minutes": 10}))
        .await;

    // Child create → 403
    s.create_task_expect_error(
        &child_t,
        json!({"name": "Hack", "minutes": 100}),
        StatusCode::FORBIDDEN,
    )
    .await;

    // Child update → 403
    s.expect_status(
        "PUT",
        &tenant_path(&format!("tasks/{}", task.id)),
        Some(&child_t),
        Some(json!({"name": "Hack", "minutes": 100})),
        StatusCode::FORBIDDEN,
    )
    .await;

    // Child delete → 403
    s.expect_status(
        "DELETE",
        &tenant_path(&format!("tasks/{}", task.id)),
        Some(&child_t),
        None,
        StatusCode::FORBIDDEN,
    )
    .await;
}

// ===========================================================================
// ASSIGNMENT TESTS (TC-014 through TC-019)
// ===========================================================================

/// TC-014: Default assignment is "all children".
#[tokio::test]
async fn tc014_default_all_children() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 5}))
        .await;
    assert!(task.assigned_children.is_none());

    let alice_tasks = s.list_child_tasks(&t, "alice").await;
    let bob_tasks = s.list_child_tasks(&t, "bob").await;

    assert!(alice_tasks.iter().any(|tk| tk.id == task.id));
    assert!(bob_tasks.iter().any(|tk| tk.id == task.id));
}

/// TC-016: Specific assignment restricts visibility.
#[tokio::test]
async fn tc016_specific_assignment() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({"name": "Homework", "minutes": 30, "assigned_children": ["alice"]}),
        )
        .await;

    let alice_tasks = s.list_child_tasks(&t, "alice").await;
    let bob_tasks = s.list_child_tasks(&t, "bob").await;

    assert!(
        alice_tasks.iter().any(|tk| tk.id == task.id),
        "alice should see assigned task"
    );
    assert!(
        !bob_tasks.iter().any(|tk| tk.id == task.id),
        "bob should NOT see task assigned only to alice"
    );
}

/// TC-018: Change from specific to "all children".
#[tokio::test]
async fn tc018_change_to_all_children() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({"name": "Clean room", "minutes": 10, "assigned_children": ["alice"]}),
        )
        .await;

    // Bob shouldn't see it yet
    let bob_tasks = s.list_child_tasks(&t, "bob").await;
    assert!(!bob_tasks.iter().any(|tk| tk.id == task.id));

    // Update to all children (no assigned_children)
    s.update_task(&t, &task.id, json!({"name": "Clean room", "minutes": 10}))
        .await;

    // Now bob should see it
    let bob_tasks = s.list_child_tasks(&t, "bob").await;
    assert!(bob_tasks.iter().any(|tk| tk.id == task.id));
}

/// TC-019: Empty assigned_children list is rejected.
#[tokio::test]
async fn tc019_empty_assignment_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    s.create_task_expect_error(
        &t,
        json!({"name": "Test", "minutes": 10, "assigned_children": []}),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

// ===========================================================================
// SCHEDULING & BLOCKING TESTS (TC-020 through TC-041)
// ===========================================================================

/// TC-020/TC-021: Mandatory task, blocking depends on start time.
/// We create a task with start_time in the past, verify blocking.
#[tokio::test]
async fn tc020_021_blocking_start_time() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create a mandatory task for all days, start at 00:00 (always active in UTC)
    let task = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    // Alice should be blocked (mandatory, all days, start at midnight, not completed)
    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let hw = child_tasks.iter().find(|tk| tk.id == task.id).unwrap();
    assert!(
        hw.is_currently_blocking,
        "mandatory task past start time should be blocking"
    );

    // Complete the task via reward
    s.reward_child(&t, "alice", Some(&task.id), None).await;

    // Now should not be blocking
    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let hw = child_tasks.iter().find(|tk| tk.id == task.id).unwrap();
    assert!(!hw.is_currently_blocking, "completed task should not block");
}

/// TC-022: Not blocking on non-scheduled day.
/// We use a bitmask for only one specific day and verify via the child tasks endpoint.
#[tokio::test]
async fn tc022_not_blocking_wrong_day() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Get current day of week, create a task mandatory on a DIFFERENT day
    let today = chrono::Utc::now().weekday();
    let today_bit = 1i32 << today.num_days_from_monday();
    // All days except today
    let other_days = 127 & !today_bit;

    if other_days == 0 {
        // Edge case: all 7 bits needed, skip
        return;
    }

    let task = s
        .create_task(
            &t,
            json!({
                "name": "Wrong day task",
                "minutes": 10,
                "mandatory_days": other_days,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let tk = child_tasks.iter().find(|tk| tk.id == task.id).unwrap();
    assert!(
        !tk.is_currently_blocking,
        "task not mandatory today should not block"
    );
}

/// TC-023: Mandatory all days blocks on any day.
#[tokio::test]
async fn tc023_all_days_blocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({
                "name": "Brush teeth",
                "minutes": 5,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let tk = child_tasks.iter().find(|tk| tk.id == task.id).unwrap();
    assert!(
        tk.is_currently_blocking,
        "all-days task should block on any day"
    );
}

/// TC-028: mandatory_days boundary values.
#[tokio::test]
async fn tc028_mandatory_days_boundaries() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Valid values: 0, 1, 64, 127
    for md in [0, 1, 64, 127] {
        let task = s
            .create_task(
                &t,
                json!({"name": "Test", "minutes": 10, "mandatory_days": md}),
            )
            .await;
        assert_eq!(task.mandatory_days, md);
    }

    // Invalid: 128, -1
    for md in [128, -1] {
        s.create_task_expect_error(
            &t,
            json!({"name": "Test", "minutes": 10, "mandatory_days": md}),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
}

/// TC-030: Default priority is 2.
#[tokio::test]
async fn tc030_default_priority() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Test", "minutes": 10}))
        .await;
    assert_eq!(task.priority, 2);
}

/// TC-031: Tasks sorted by priority ascending, then alphabetically.
#[tokio::test]
async fn tc031_sort_order() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    s.create_task(
        &t,
        json!({"name": "Zebra task", "minutes": 10, "priority": 1}),
    )
    .await;
    s.create_task(
        &t,
        json!({"name": "Apple task", "minutes": 10, "priority": 2}),
    )
    .await;
    s.create_task(
        &t,
        json!({"name": "Mango task", "minutes": 10, "priority": 2}),
    )
    .await;
    s.create_task(
        &t,
        json!({"name": "Banana task", "minutes": 10, "priority": 3}),
    )
    .await;

    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let names: Vec<&str> = child_tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Zebra task", "Apple task", "Mango task", "Banana task"]
    );
}

/// TC-033: No mandatory tasks → not blocked.
#[tokio::test]
async fn tc033_no_mandatory_not_blocked() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create optional-only tasks
    s.create_task(&t, json!({"name": "Optional", "minutes": 10}))
        .await;

    let remaining = s.get_remaining(&t, "alice").await;
    assert!(
        !remaining.blocked_by_tasks,
        "no mandatory tasks => not blocked"
    );
}

/// TC-034: At least one mandatory incomplete → blocked.
#[tokio::test]
async fn tc034_mandatory_incomplete_blocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create mandatory task (all days, midnight start)
    s.create_task(
        &t,
        json!({
            "name": "Homework",
            "minutes": 30,
            "mandatory_days": 127,
            "mandatory_start_time": "00:00"
        }),
    )
    .await;

    // Also an optional task (shouldn't matter)
    s.create_task(&t, json!({"name": "Optional", "minutes": 10}))
        .await;

    let remaining = s.get_remaining(&t, "alice").await;
    assert!(
        remaining.blocked_by_tasks,
        "mandatory incomplete task should block"
    );
}

/// TC-035: All mandatory done → unblocked.
#[tokio::test]
async fn tc035_all_mandatory_done_unblocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let t1 = s
        .create_task(
            &t,
            json!({
                "name": "Brush teeth",
                "minutes": 5,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;
    let t2 = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    // Blocked initially
    let r = s.get_remaining(&t, "alice").await;
    assert!(r.blocked_by_tasks);

    // Complete first
    s.reward_child(&t, "alice", Some(&t1.id), None).await;

    // Still blocked (second not done)
    let r = s.get_remaining(&t, "alice").await;
    assert!(r.blocked_by_tasks);

    // Complete second
    s.reward_child(&t, "alice", Some(&t2.id), None).await;

    // Now unblocked
    let r = s.get_remaining(&t, "alice").await;
    assert!(!r.blocked_by_tasks, "all mandatory done => unblocked");
}

/// TC-036: Blocking is per-child.
#[tokio::test]
async fn tc036_blocking_per_child() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Task assigned to alice only
    let alice_task = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00",
                "assigned_children": ["alice"]
            }),
        )
        .await;

    // Task assigned to bob only
    s.create_task(
        &t,
        json!({
            "name": "Reading",
            "minutes": 20,
            "mandatory_days": 127,
            "mandatory_start_time": "00:00",
            "assigned_children": ["bob"]
        }),
    )
    .await;

    // Complete alice's task
    s.reward_child(&t, "alice", Some(&alice_task.id), None)
        .await;

    let alice_r = s.get_remaining(&t, "alice").await;
    let bob_r = s.get_remaining(&t, "bob").await;

    assert!(!alice_r.blocked_by_tasks, "alice done => unblocked");
    assert!(bob_r.blocked_by_tasks, "bob not done => blocked");
}

/// TC-040: mandatory_days=0 never blocks.
#[tokio::test]
async fn tc040_optional_never_blocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    s.create_task(
        &t,
        json!({
            "name": "Optional with time",
            "minutes": 10,
            "mandatory_days": 0,
            "mandatory_start_time": "00:00"
        }),
    )
    .await;

    let r = s.get_remaining(&t, "alice").await;
    assert!(!r.blocked_by_tasks, "mandatory_days=0 should never block");
}

/// TC-041: Multiple mandatory, one incomplete → still blocked.
#[tokio::test]
async fn tc041_partial_completion_still_blocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let t1 = s
        .create_task(
            &t,
            json!({
                "name": "Task A",
                "minutes": 5,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;
    s.create_task(
        &t,
        json!({
            "name": "Task B",
            "minutes": 5,
            "mandatory_days": 127,
            "mandatory_start_time": "00:00"
        }),
    )
    .await;

    // Complete only task A
    s.reward_child(&t, "alice", Some(&t1.id), None).await;

    let r = s.get_remaining(&t, "alice").await;
    assert!(
        r.blocked_by_tasks,
        "one incomplete mandatory => still blocked"
    );
}

// ===========================================================================
// MIGRATION TESTS (TC-042 through TC-048)
// ===========================================================================

/// TC-042 + TC-043: YAML tasks migrated, IDs preserved, fields set correctly.
#[tokio::test]
async fn tc042_043_yaml_migration() {
    let Some(s) = TestServer::spawn_custom(
        vec![Child {
            id: "alice".into(),
            display_name: "Alice".into(),
        }],
        vec![
            Task {
                id: "brush_teeth".into(),
                name: "Brush teeth".into(),
                minutes: 10,
                required: true,
            },
            Task {
                id: "homework".into(),
                name: "Homework".into(),
                minutes: 30,
                required: true,
            },
            Task {
                id: "clean_room".into(),
                name: "Clean room".into(),
                minutes: 15,
                required: false,
            },
        ],
        chrono_tz::UTC,
    )
    .await
    else {
        return;
    };
    let t = s.parent_token().await;

    let tasks = s.list_tasks(&t).await;
    assert_eq!(tasks.len(), 3);

    // Verify IDs preserved
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"brush_teeth"));
    assert!(ids.contains(&"homework"));
    assert!(ids.contains(&"clean_room"));

    // Required tasks: mandatory_days=127, start_time=00:00
    let brush = tasks.iter().find(|t| t.id == "brush_teeth").unwrap();
    assert_eq!(brush.mandatory_days, 127);
    assert_eq!(brush.mandatory_start_time.as_deref(), Some("00:00"));
    assert_eq!(brush.priority, 2);

    // Optional task: mandatory_days=0
    let clean = tasks.iter().find(|t| t.id == "clean_room").unwrap();
    assert_eq!(clean.mandatory_days, 0);
}

/// TC-044: Subsequent startup skips migration when DB tasks exist.
#[tokio::test]
async fn tc044_skip_migration_when_tasks_exist() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create a task (now DB has tasks)
    s.create_task(&t, json!({"name": "Existing", "minutes": 10}))
        .await;

    // Simulate re-seeding with YAML tasks — the store should skip migration
    let db_path = s.db_path.to_str().unwrap().to_string();
    let store = storage::Store::connect_sqlite(&db_path)
        .await
        .expect("connect");
    let yaml_tasks = vec![Task {
        id: "should_not_appear".into(),
        name: "Should not appear".into(),
        minutes: 99,
        required: true,
    }];
    store
        .seed_from_config(&[], &yaml_tasks)
        .await
        .expect("seed");

    // Verify the YAML task was NOT inserted
    let tasks = s.list_tasks(&t).await;
    assert!(
        !tasks.iter().any(|t| t.id == "should_not_appear"),
        "YAML task should not be migrated when DB already has tasks"
    );
    assert_eq!(tasks.len(), 1, "only the original task should exist");
}

// ===========================================================================
// CHILD VIEW / ENHANCED ENDPOINT TESTS (TC-049 through TC-053)
// ===========================================================================

/// TC-049: Child sees only assigned tasks.
#[tokio::test]
async fn tc049_child_sees_assigned_only() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // All children
    let brush = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 5}))
        .await;
    // Alice only
    let hw = s
        .create_task(
            &t,
            json!({"name": "Homework", "minutes": 30, "assigned_children": ["alice"]}),
        )
        .await;
    // Bob only
    s.create_task(
        &t,
        json!({"name": "Reading", "minutes": 20, "assigned_children": ["bob"]}),
    )
    .await;

    let alice_tasks = s.list_child_tasks(&t, "alice").await;
    let alice_ids: Vec<&str> = alice_tasks.iter().map(|t| t.id.as_str()).collect();
    assert!(alice_ids.contains(&brush.id.as_str()));
    assert!(alice_ids.contains(&hw.id.as_str()));
    assert_eq!(
        alice_ids.len(),
        2,
        "alice should see Brush teeth + Homework, not Reading"
    );
}

/// TC-051: Sort within child view by priority then alphabetically.
#[tokio::test]
async fn tc051_child_view_sorting() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    s.create_task(&t, json!({"name": "Zebra", "minutes": 5, "priority": 3}))
        .await;
    s.create_task(&t, json!({"name": "Alpha", "minutes": 5, "priority": 1}))
        .await;
    s.create_task(&t, json!({"name": "Mango", "minutes": 5, "priority": 2}))
        .await;
    s.create_task(&t, json!({"name": "Apple", "minutes": 5, "priority": 2}))
        .await;

    let tasks = s.list_child_tasks(&t, "alice").await;
    let names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "Apple", "Mango", "Zebra"]);
}

/// TC-053: No assigned tasks → empty list.
#[tokio::test]
async fn tc053_no_tasks_empty_list() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create task assigned to bob only
    s.create_task(
        &t,
        json!({"name": "Bob only", "minutes": 10, "assigned_children": ["bob"]}),
    )
    .await;

    let alice_tasks = s.list_child_tasks(&t, "alice").await;
    assert!(alice_tasks.is_empty(), "alice should see no tasks");
}

// ===========================================================================
// API TESTS (TC-054 through TC-062)
// ===========================================================================

/// TC-054: POST /tasks returns 201.
#[tokio::test]
async fn tc054_create_returns_201() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;
    let (status, _) = s
        .raw(
            "POST",
            &tenant_path("tasks"),
            Some(&t),
            Some(json!({"name": "Test", "minutes": 10})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

/// TC-057: GET /tasks excludes soft-deleted.
#[tokio::test]
async fn tc057_list_excludes_deleted() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    s.create_task(&t, json!({"name": "Keep", "minutes": 5}))
        .await;
    s.create_task(&t, json!({"name": "Keep2", "minutes": 5}))
        .await;
    let to_delete = s
        .create_task(&t, json!({"name": "Delete me", "minutes": 5}))
        .await;
    s.delete_task(&t, &to_delete.id).await;

    let tasks = s.list_tasks(&t).await;
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|t| t.id != to_delete.id));
}

/// TC-059: Invalid JSON → 400.
#[tokio::test]
async fn tc059_invalid_json() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Send raw invalid JSON
    let url = format!("{}{}", s.base, tenant_path("tasks"));
    let resp = s
        .client
        .post(&url)
        .bearer_auth(&t)
        .header("content-type", "application/json")
        .body("{ invalid json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// TC-060: PUT on soft-deleted task → 404.
#[tokio::test]
async fn tc060_update_deleted_returns_404() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Test", "minutes": 10}))
        .await;
    s.delete_task(&t, &task.id).await;

    s.expect_status(
        "PUT",
        &tenant_path(&format!("tasks/{}", task.id)),
        Some(&t),
        Some(json!({"name": "Updated", "minutes": 10})),
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// TC-061: DELETE on already-deleted task → 404.
#[tokio::test]
async fn tc061_double_delete_returns_404() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(&t, json!({"name": "Test", "minutes": 10}))
        .await;
    s.delete_task(&t, &task.id).await;

    s.expect_status(
        "DELETE",
        &tenant_path(&format!("tasks/{}", task.id)),
        Some(&t),
        None,
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// TC-062: Unauthenticated requests → 401.
#[tokio::test]
async fn tc062_unauthenticated_401() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };

    let paths_methods = vec![
        ("GET", tenant_path("tasks")),
        ("POST", tenant_path("tasks")),
        ("PUT", tenant_path("tasks/some-id")),
        ("DELETE", tenant_path("tasks/some-id")),
    ];

    for (method, path) in paths_methods {
        s.expect_status(method, &path, None, None, StatusCode::UNAUTHORIZED)
            .await;
    }
}

// ===========================================================================
// EDGE CASE TESTS (TC-063 through TC-074)
// ===========================================================================

/// TC-063: Deleting a blocking task unblocks the child.
#[tokio::test]
async fn tc063_delete_blocking_task_unblocks() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    // Blocked
    let r = s.get_remaining(&t, "alice").await;
    assert!(r.blocked_by_tasks);

    // Delete the task
    s.delete_task(&t, &task.id).await;

    // No longer blocked
    let r = s.get_remaining(&t, "alice").await;
    assert!(!r.blocked_by_tasks, "deleting blocking task should unblock");
}

/// TC-065: mandatory_days > 0 without start_time defaults to 00:00.
#[tokio::test]
async fn tc065_default_start_time() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s
        .create_task(
            &t,
            json!({"name": "Test", "minutes": 10, "mandatory_days": 127}),
        )
        .await;
    assert_eq!(
        task.mandatory_start_time.as_deref(),
        Some("00:00"),
        "default start_time should be 00:00 when mandatory"
    );
}

/// TC-067: Exactly 100-character name succeeds.
#[tokio::test]
async fn tc067_name_100_chars() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let name = "x".repeat(100);
    let task = s
        .create_task(&t, json!({"name": name, "minutes": 10}))
        .await;
    assert_eq!(task.name.len(), 100);
}

/// TC-068: Single-character name succeeds.
#[tokio::test]
async fn tc068_name_1_char() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let task = s.create_task(&t, json!({"name": "X", "minutes": 10})).await;
    assert_eq!(task.name, "X");
}

/// TC-072: Invalid mandatory_start_time formats rejected.
#[tokio::test]
async fn tc072_invalid_start_time_rejected() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    let invalid_times = ["25:00", "12:60", "abc"];
    for time in invalid_times {
        s.create_task_expect_error(
            &t,
            json!({
                "name": "Test",
                "minutes": 10,
                "mandatory_days": 127,
                "mandatory_start_time": time
            }),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
}

/// TC-077: `required` backward-compat column derived from mandatory_days.
#[tokio::test]
async fn tc077_required_column_backward_compat() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Create mandatory task
    let mandatory = s
        .create_task(
            &t,
            json!({"name": "Mandatory", "minutes": 10, "mandatory_days": 31}),
        )
        .await;

    // Create optional task
    let optional = s
        .create_task(
            &t,
            json!({"name": "Optional", "minutes": 10, "mandatory_days": 0}),
        )
        .await;

    // Check via child endpoint (has `required` field)
    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let m_task = child_tasks.iter().find(|tk| tk.id == mandatory.id).unwrap();
    let o_task = child_tasks.iter().find(|tk| tk.id == optional.id).unwrap();

    assert!(m_task.required, "mandatory_days!=0 => required=true");
    assert!(!o_task.required, "mandatory_days=0 => required=false");

    // Update mandatory to optional
    s.update_task(
        &t,
        &mandatory.id,
        json!({"name": "Mandatory", "minutes": 10, "mandatory_days": 0}),
    )
    .await;

    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let m_task = child_tasks.iter().find(|tk| tk.id == mandatory.id).unwrap();
    assert!(
        !m_task.required,
        "after changing mandatory_days to 0, required should be false"
    );
}

/// Enhanced child endpoint returns is_currently_blocking correctly (TC-049-053 supplement).
#[tokio::test]
async fn child_endpoint_is_currently_blocking() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Mandatory task
    let mandatory = s
        .create_task(
            &t,
            json!({
                "name": "Mandatory",
                "minutes": 10,
                "mandatory_days": 127,
                "mandatory_start_time": "00:00"
            }),
        )
        .await;

    // Optional task
    let optional = s
        .create_task(&t, json!({"name": "Optional", "minutes": 5}))
        .await;

    let child_tasks = s.list_child_tasks(&t, "alice").await;

    let m = child_tasks.iter().find(|t| t.id == mandatory.id).unwrap();
    let o = child_tasks.iter().find(|t| t.id == optional.id).unwrap();

    assert!(m.is_currently_blocking, "mandatory task should be blocking");
    assert!(!o.is_currently_blocking, "optional task should not block");

    // Verify fields exist
    assert!(m.priority > 0);
    assert!(m.mandatory_days > 0);
}

/// TC-015 equivalent: New child sees "all children" tasks dynamically.
/// We test by creating task, then adding new child via config.
#[tokio::test]
async fn tc015_new_child_sees_all_tasks() {
    // Start with 1 child, then add another via DB seed
    let Some(s) = TestServer::spawn_custom(
        vec![Child {
            id: "alice".into(),
            display_name: "Alice".into(),
        }],
        vec![],
        chrono_tz::UTC,
    )
    .await
    else {
        return;
    };
    let t = s.parent_token().await;

    // Create "all children" task
    let task = s
        .create_task(&t, json!({"name": "Brush teeth", "minutes": 5}))
        .await;
    assert!(task.assigned_children.is_none());

    // Now add bob to the DB
    let db_path = s.db_path.to_str().unwrap().to_string();
    let store = storage::Store::connect_sqlite(&db_path)
        .await
        .expect("connect");
    store
        .seed_children_from_config(&[Child {
            id: "bob".into(),
            display_name: "Bob".into(),
        }])
        .await
        .expect("seed bob");

    // Bob should see the task (since it's "all children" — dynamic, not snapshot)
    // We need to use parent token since bob doesn't have auth
    // Actually bob has no user config, so we query via parent
    // The list_tasks_for_child is what matters — let's query the store directly
    let tasks = store.list_tasks_for_child("bob").await.expect("bob tasks");
    assert!(
        tasks.iter().any(|(t, _)| t.id == task.id),
        "new child bob should see 'all children' task"
    );
}

/// TC-026: Timezone evaluation uses family timezone, not UTC.
/// We verify by creating a task with timezone-specific blocking.
#[tokio::test]
async fn tc026_timezone_evaluation() {
    // Use a timezone with known offset
    let tz: chrono_tz::Tz = "Asia/Tokyo".parse().unwrap(); // UTC+9
    let Some(s) = TestServer::spawn_custom(
        vec![Child {
            id: "alice".into(),
            display_name: "Alice".into(),
        }],
        vec![],
        tz,
    )
    .await
    else {
        return;
    };
    let t = s.parent_token().await;

    // Get the current day in Tokyo timezone
    let now_tokyo = chrono::Utc::now().with_timezone(&tz);
    let today_bit = 1i32 << now_tokyo.weekday().num_days_from_monday();

    // Create a task mandatory today (in Tokyo) at a start time in the past (Tokyo time)
    let start = format!("{:02}:{:02}", now_tokyo.hour().saturating_sub(1).max(0), 0);

    let task = s
        .create_task(
            &t,
            json!({
                "name": "Tokyo task",
                "minutes": 10,
                "mandatory_days": today_bit,
                "mandatory_start_time": start
            }),
        )
        .await;

    // Should be blocking in Tokyo timezone
    let child_tasks = s.list_child_tasks(&t, "alice").await;
    let tk = child_tasks.iter().find(|tk| tk.id == task.id).unwrap();
    assert!(
        tk.is_currently_blocking,
        "task should be blocking in family timezone (Asia/Tokyo)"
    );
}

/// TC-029: Bitmask correctly maps bits to days.
#[tokio::test]
async fn tc029_bitmask_day_mapping() {
    // Test by creating tasks with specific day bits and verifying they're created correctly
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64
    let test_cases = [
        (1, "Monday only"),
        (2, "Tuesday only"),
        (4, "Wednesday only"),
        (8, "Thursday only"),
        (16, "Friday only"),
        (32, "Saturday only"),
        (64, "Sunday only"),
        (42, "Tue+Thu+Sat"),
    ];

    for (bitmask, desc) in test_cases {
        let task = s
            .create_task(
                &t,
                json!({
                    "name": desc,
                    "minutes": 10,
                    "mandatory_days": bitmask,
                    "mandatory_start_time": "00:00"
                }),
            )
            .await;
        assert_eq!(task.mandatory_days, bitmask, "bitmask for {desc}");
    }
}

/// TC-017: Removing a child from assignment preserves completion history.
#[tokio::test]
async fn tc017_reassignment_preserves_history() {
    let Some(s) = TestServer::spawn_default().await else {
        return;
    };
    let t = s.parent_token().await;

    // Task assigned to both
    let task = s
        .create_task(
            &t,
            json!({
                "name": "Homework",
                "minutes": 30,
                "assigned_children": ["alice", "bob"]
            }),
        )
        .await;

    // Alice completes it (via reward)
    s.reward_child(&t, "alice", Some(&task.id), None).await;

    // Reassign to bob only
    s.update_task(
        &t,
        &task.id,
        json!({
            "name": "Homework",
            "minutes": 30,
            "assigned_children": ["bob"]
        }),
    )
    .await;

    // Alice should not see the task anymore
    let alice_tasks = s.list_child_tasks(&t, "alice").await;
    assert!(!alice_tasks.iter().any(|tk| tk.id == task.id));

    // But alice's completion should still exist in DB
    let task_id = task.id.clone();
    let count = s
        .run_sql(move |conn| {
            use diesel::prelude::*;
            #[derive(QueryableByName)]
            struct Count {
                #[diesel(sql_type = diesel::sql_types::BigInt)]
                cnt: i64,
            }
            let row: Count = diesel::sql_query(
                "SELECT COUNT(*) as cnt FROM task_completions WHERE task_id = ?1 AND child_id = 'alice'",
            )
            .bind::<diesel::sql_types::Text, _>(&task_id)
            .get_result(conn)
            .unwrap();
            row.cnt
        })
        .await;
    assert!(count > 0, "alice's completions should be preserved");
}
