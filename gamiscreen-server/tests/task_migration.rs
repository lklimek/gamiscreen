use diesel::Connection;
use diesel::connection::SimpleConnection;
use diesel::migration::MigrationSource;
use diesel::prelude::*;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use gamiscreen_server::storage::{Store, schema};

#[tokio::test]
async fn upgrades_existing_tasks_without_losing_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.db");
    let path = path.to_str().unwrap();
    let mut conn = SqliteConnection::establish(path).unwrap();
    const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
    let legacy: Vec<_> =
        <EmbeddedMigrations as MigrationSource<diesel::sqlite::Sqlite>>::migrations(&MIGRATIONS)
            .unwrap()
            .into_iter()
            .filter(|m| m.name().version().to_string() != "00000000000011")
            .collect();
    conn.applied_migrations().unwrap();
    conn.run_migrations(&legacy).unwrap();
    conn.batch_execute(
        "PRAGMA foreign_keys = ON;
         INSERT INTO children (id, display_name) VALUES ('alice', 'Alice');
         INSERT INTO tasks (id, name, minutes, required) VALUES ('homework', 'Homework', 10, 1);
         INSERT INTO tasks (id, name, minutes, required) VALUES ('reading', 'Reading', 5, 0);
         INSERT INTO task_completions (child_id, task_id, by_username) VALUES ('alice', 'homework', 'parent');
         INSERT INTO rewards (child_id, task_id, minutes) VALUES ('alice', 'homework', 10);",
    )
    .unwrap();

    let store = Store::connect_sqlite(path)
        .await
        .expect("upgrade existing database");
    let tasks = store.list_tasks().await.unwrap();
    assert_eq!(tasks.len(), 2);
    let required = tasks.iter().find(|t| t.id == "homework").unwrap();
    assert_eq!(required.mandatory_days, 127);
    assert_eq!(required.mandatory_start_time.as_deref(), Some("00:00"));
    let optional = tasks.iter().find(|t| t.id == "reading").unwrap();
    assert_eq!(optional.mandatory_days, 0);
    assert_eq!(required.priority, 2);
    assert_eq!(required.created_at.date(), chrono::Utc::now().date_naive());
    assert_eq!(
        schema::task_completions::table
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap(),
        1
    );
    assert_eq!(
        schema::rewards::table
            .select(schema::rewards::task_id)
            .first::<Option<String>>(&mut conn)
            .unwrap()
            .as_deref(),
        Some("homework")
    );

    let new = store
        .create_task("New task", 3, 2, 0, None, None)
        .await
        .unwrap();
    assert!(new.created_at >= required.created_at);
}

#[test]
fn rollback_preserves_deleted_task_history() {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    conn.batch_execute(
        "PRAGMA foreign_keys = ON;
         INSERT INTO children (id, display_name) VALUES ('alice', 'Alice');
         INSERT INTO tasks (id, name, minutes, required, deleted_at) VALUES ('homework', 'Homework', 10, 1, CURRENT_TIMESTAMP);
         INSERT INTO task_completions (child_id, task_id, by_username) VALUES ('alice', 'homework', 'parent');
         INSERT INTO rewards (child_id, task_id, minutes) VALUES ('alice', 'homework', 10);",
    ).unwrap();
    conn.revert_last_migration(MIGRATIONS)
        .expect("rollback preserves referenced tasks");
    assert_eq!(
        schema::task_completions::table
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap(),
        1
    );
    assert_eq!(
        schema::rewards::table
            .select(schema::rewards::task_id)
            .first::<Option<String>>(&mut conn)
            .unwrap()
            .as_deref(),
        Some("homework")
    );
    assert_eq!(
        schema::tasks::table
            .select(schema::tasks::id)
            .first::<String>(&mut conn)
            .unwrap(),
        "homework"
    );
}
