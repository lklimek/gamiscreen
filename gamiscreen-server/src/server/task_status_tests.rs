use gamiscreen_shared::jwt::JwtClaims;
use serde_json::{Value, json};

use super::*;
use crate::storage::Store;

fn auth(child: Option<&str>) -> Extension<AuthCtx> {
    Extension(AuthCtx {
        claims: JwtClaims {
            sub: child.unwrap_or("parent").into(),
            jti: "test-session".into(),
            exp: i64::MAX / 2,
            role: if child.is_some() {
                Role::Child
            } else {
                Role::Parent
            },
            child_id: child.map(str::to_owned),
            device_id: None,
            tenant_id: "family".into(),
        },
    })
}

async fn setup(minutes: i32) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::connect_sqlite(dir.path().join("test.db").to_str().unwrap())
        .await
        .unwrap();
    let config: AppConfig = serde_json::from_value(json!({
        "tenant_id": "family", "jwt_secret": "test-secret", "users": [],
        "children": [{"id": "alice", "display_name": "Alice"},
                     {"id": "bob", "display_name": "Bob"}]
    }))
    .unwrap();
    store
        .seed_children_from_config(&config.children)
        .await
        .unwrap();
    for child in &config.children {
        if minutes != 0 {
            store
                .add_reward_minutes(&child.id, minutes, None, None, false, None)
                .await
                .unwrap();
        }
    }
    (AppState::new(config, store), dir)
}

fn task_body() -> Value {
    json!({"name": "Homework", "minutes": 5, "mandatory_days": 127,
           "mandatory_start_time": "00:00", "assigned_children": ["alice"]})
}

fn expect_status(rx: &mut broadcast::Receiver<ServerEvent>, child: &str, blocked: bool) {
    let mut found = false;
    while let Ok(event) = rx.try_recv() {
        if let ServerEvent::RemainingUpdated {
            child_id,
            blocked_by_tasks,
            ..
        } = event
            && child_id == child
        {
            assert_eq!(blocked_by_tasks, blocked);
            found = true;
        }
    }
    assert!(found, "missing status event for {child}, blocked={blocked}");
}

#[tokio::test]
async fn task_status_delete_releases_a_client_without_a_heartbeat() {
    let (state, _dir) = setup(30).await;
    let (_, Json(task)) = api_create_task(
        State(state.clone()),
        auth(None),
        Json(serde_json::from_value(task_body()).unwrap()),
    )
    .await
    .unwrap();
    let mut rx = state.notif_tx.subscribe();
    let _ = api_delete_task(
        State(state.clone()),
        auth(None),
        Path(TaskPath { task_id: task.id }),
    )
    .await
    .unwrap();
    expect_status(&mut rx, "alice", false);
    assert_eq!(state.store.get_remaining("alice").await.unwrap(), 30);
}

#[tokio::test]
async fn task_status_assignment_changes_notify_both_children() {
    let (state, _dir) = setup(30).await;
    let mut alice_rx = state.notif_tx.subscribe();
    let (_, Json(task)) = api_create_task(
        State(state.clone()),
        auth(None),
        Json(serde_json::from_value(task_body()).unwrap()),
    )
    .await
    .unwrap();
    expect_status(&mut alice_rx, "alice", true);
    let mut bob_rx = state.notif_tx.subscribe();
    let mut body = task_body();
    body["assigned_children"] = json!(["bob"]);
    let _ = api_update_task(
        State(state.clone()),
        auth(None),
        Path(TaskPath {
            task_id: task.id.clone(),
        }),
        Json(serde_json::from_value(body.clone()).unwrap()),
    )
    .await
    .unwrap();
    expect_status(&mut alice_rx, "alice", false);
    expect_status(&mut bob_rx, "bob", true);

    body["mandatory_days"] = json!(0);
    let _ = api_update_task(
        State(state.clone()),
        auth(None),
        Path(TaskPath { task_id: task.id }),
        Json(serde_json::from_value(body).unwrap()),
    )
    .await
    .unwrap();
    expect_status(&mut bob_rx, "bob", false);
}

#[tokio::test]
async fn task_status_moving_start_time_forward_releases_block() {
    let (mut state, _dir) = setup(30).await;
    // Keep the test's local time before noon, regardless of the UTC test time.
    use chrono::Timelike;
    state.config.family_tz = if chrono::Utc::now().hour() < 12 {
        chrono_tz::UTC
    } else {
        chrono_tz::Etc::GMTPlus12
    };
    let (_, Json(task)) = api_create_task(
        State(state.clone()),
        auth(None),
        Json(serde_json::from_value(task_body()).unwrap()),
    )
    .await
    .unwrap();
    let mut rx = state.notif_tx.subscribe();
    let mut body = task_body();
    body["mandatory_start_time"] = json!("23:59");
    let _ = api_update_task(
        State(state.clone()),
        auth(None),
        Path(TaskPath { task_id: task.id }),
        Json(serde_json::from_value(body).unwrap()),
    )
    .await
    .unwrap();
    expect_status(&mut rx, "alice", false);
}

async fn scheduled_transition(minutes: i32) {
    let (state, _dir) = setup(minutes + 1).await;
    let minute = chrono::Utc::now().timestamp() / 60;
    // Duplicate usage is valid and must not change the stored balance.
    state
        .store
        .process_usage_minutes("alice", "device", &[minute])
        .await
        .unwrap();
    assert_eq!(state.store.get_remaining("alice").await.unwrap(), minutes);
    let _ = api_remaining(
        State(state.clone()),
        auth(Some("alice")),
        Path("alice".into()),
    )
    .await
    .unwrap();
    let mut rx = state.notif_tx.subscribe();
    // Activate a task outside the management API, as a clock boundary does.
    state
        .store
        .create_task(
            "Homework",
            5,
            2,
            127,
            Some("00:00"),
            Some(vec!["alice".into()]),
        )
        .await
        .unwrap();
    let heartbeat = || {
        api_device_heartbeat(
            State(state.clone()),
            auth(Some("alice")),
            Path(ChildDevicePath {
                id: "alice".into(),
                device_id: "device".into(),
            }),
            Json(api::HeartbeatReq {
                minutes: vec![minute],
            }),
        )
    };
    let Json(response) = heartbeat().await.unwrap();
    assert!(response.blocked_by_tasks);
    expect_status(&mut rx, "alice", true);
    let _ = heartbeat().await.unwrap();
    assert!(
        matches!(rx.try_recv(), Err(broadcast::error::TryRecvError::Empty)),
        "unchanged status must not repeat notifications"
    );
}

#[tokio::test]
async fn task_status_heartbeat_publishes_scheduled_block() {
    scheduled_transition(30).await;
}

#[tokio::test]
async fn task_status_block_flag_change_is_published_even_with_zero_minutes() {
    scheduled_transition(0).await;
}

async fn child_task(state: &AppState) -> Value {
    let Json(tasks) = api_list_child_tasks(
        State(state.clone()),
        auth(Some("alice")),
        Path("alice".into()),
    )
    .await
    .unwrap();
    serde_json::to_value(&tasks[0]).unwrap()
}

async fn submit(state: &AppState, id: &str) {
    api_submit_task(
        State(state.clone()),
        auth(Some("alice")),
        Path(ChildTaskPath {
            id: "alice".into(),
            task_id: id.into(),
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn task_status_pending_submission_follows_rejection_approval_and_new_day() {
    let (state, dir) = setup(30).await;
    let task = state
        .store
        .create_task("Homework", 5, 2, 127, Some("00:00"), None)
        .await
        .unwrap();
    assert_eq!(child_task(&state).await["pending_submission"], false);
    submit(&state, &task.id).await;
    assert_eq!(child_task(&state).await["pending_submission"], true);
    let pending = state.store.list_pending_submissions().await.unwrap();
    api_discard_submission(State(state.clone()), auth(None), Path(pending[0].0.id))
        .await
        .unwrap();
    assert_eq!(child_task(&state).await["pending_submission"], false);

    submit(&state, &task.id).await;
    let pending = state.store.list_pending_submissions().await.unwrap();
    api_approve_submission(State(state.clone()), auth(None), Path(pending[0].0.id))
        .await
        .unwrap();
    let approved = child_task(&state).await;
    assert_eq!(approved["pending_submission"], false);
    assert_eq!(approved["completed_today"], true);

    use diesel::{Connection, RunQueryDsl};
    let mut conn =
        diesel::SqliteConnection::establish(dir.path().join("test.db").to_str().unwrap()).unwrap();
    diesel::sql_query("UPDATE task_completions SET done_at = datetime('now', '-1 day')")
        .execute(&mut conn)
        .unwrap();
    let next_day = child_task(&state).await;
    assert_eq!(next_day["pending_submission"], false);
    assert_eq!(next_day["completed_today"], false);
    submit(&state, &task.id).await;
    assert_eq!(child_task(&state).await["pending_submission"], true);
    assert_eq!(
        serde_json::to_value(
            api_list_child_tasks(State(state), auth(Some("bob")), Path("bob".into()))
                .await
                .unwrap()
                .0
        )
        .unwrap()[0]["pending_submission"],
        false
    );
}

#[tokio::test]
async fn task_status_manual_accept_clears_pending_submissions() {
    let (state, _dir) = setup(30).await;
    let task = state
        .store
        .create_task("Homework", 5, 2, 127, Some("00:00"), None)
        .await
        .unwrap();
    submit(&state, &task.id).await;
    let submission_id = state.store.list_pending_submissions().await.unwrap()[0]
        .0
        .id;
    state.store.submit_task("bob", &task.id).await.unwrap();
    let mut rx = state.notif_tx.subscribe();
    let _ = api_child_reward(
        State(state.clone()),
        auth(None),
        Path(ChildPathId { id: "alice".into() }),
        Json(api::RewardReq {
            child_id: "alice".into(),
            task_id: Some(task.id),
            minutes: None,
            description: None,
            is_borrowed: None,
        }),
    )
    .await
    .unwrap();
    let accepted = child_task(&state).await;
    assert_eq!(accepted["completed_today"], true);
    assert_eq!(accepted["pending_submission"], false);
    assert_eq!(state.store.pending_submissions_count().await.unwrap(), 1);
    let mut count_updated = false;
    while let Ok(event) = rx.try_recv() {
        if let ServerEvent::PendingCount { count } = event {
            assert_eq!(count, 1);
            count_updated = true;
        }
    }
    assert!(
        count_updated,
        "manual acceptance must update the pending notification count"
    );
    api_approve_submission(State(state.clone()), auth(None), Path(submission_id))
        .await
        .unwrap();
    assert_eq!(state.store.get_remaining("alice").await.unwrap(), 35);
    assert_eq!(state.store.pending_submissions_count().await.unwrap(), 1);
}
