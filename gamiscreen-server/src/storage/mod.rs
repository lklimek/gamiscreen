pub mod models;
pub mod schema;

use chrono::{NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use models::{
    Child, NewChild, NewPushSubscription, NewReward, NewSession, NewTask, NewTaskAssignment,
    PushSubscription, Session, Task,
};
use tracing::{info, trace};
use uuid::Uuid;

/// Generate a human-readable task ID from the task name.
///
/// The ID is composed of the slugified name plus an 8-character hex suffix
/// derived from a UUID v4, e.g. `homework-a1b2c3d4`.
pub fn generate_task_id(name: &str) -> String {
    let slug_part = slug::slugify(name);
    let suffix = &Uuid::new_v4().simple().to_string()[..8];
    if slug_part.is_empty() {
        // Fallback for names that slugify to empty (e.g. all-emoji names)
        format!("task-{suffix}")
    } else {
        format!("{slug_part}-{suffix}")
    }
}

/// Structured error type for all storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// A Diesel ORM error (query failure, constraint violation, etc.)
    #[error("database error: {0}")]
    Database(#[from] diesel::result::Error),

    /// Failed to acquire or build a connection from the pool.
    #[error("pool error: {0}")]
    Pool(#[from] diesel::r2d2::PoolError),

    /// A `spawn_blocking` task panicked or was cancelled.
    #[error("task error: {0}")]
    Task(#[from] tokio::task::JoinError),

    /// A database migration failed to apply.
    #[error("migration error: {0}")]
    Migration(String),

    /// The caller supplied invalid input.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Clone)]
pub struct Store {
    pool: Pool<ConnectionManager<SqliteConnection>>,
}

impl Store {
    pub async fn connect_sqlite(path: &str) -> Result<Self, StorageError> {
        let url = path.to_string();
        let manager = ConnectionManager::<SqliteConnection>::new(url);
        let pool = Pool::builder().max_size(8).build(manager)?;

        // Run pending Diesel migrations on startup (auto-init empty DBs)
        {
            let pool_clone = pool.clone();
            tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
                const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
                let mut conn = pool_clone.get()?;
                configure_sqlite_conn(&mut conn)?;
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|e| StorageError::Migration(e.to_string()))?;
                Ok(())
            })
            .await??;
        }

        Ok(Store { pool })
    }

    /// Seed children and tasks from config.
    ///
    /// This is a convenience wrapper that calls `seed_children_from_config` and
    /// `migrate_yaml_tasks_to_db`. Kept for backward compatibility with tests.
    pub async fn seed_from_config(
        &self,
        cfg_children: &[gamiscreen_shared::domain::Child],
        cfg_tasks: &[gamiscreen_shared::domain::Task],
    ) -> Result<(), StorageError> {
        self.seed_children_from_config(cfg_children).await?;
        self.migrate_yaml_tasks_to_db(cfg_tasks).await?;
        Ok(())
    }

    pub async fn list_children(&self) -> Result<Vec<Child>, StorageError> {
        use schema::children::dsl::*;
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Child>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(children
                .order(display_name.asc())
                .load::<Child>(&mut conn)?)
        })
        .await?
    }

    pub async fn upsert_push_subscription(
        &self,
        tenant_id: &str,
        child_id: &str,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
    ) -> Result<PushSubscription, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        let child_owned = child_id.to_string();
        let endpoint_owned = endpoint.to_string();
        let p256dh_owned = p256dh.to_string();
        let auth_owned = auth.to_string();
        trace!(
            child_id = %child_owned,
            endpoint = %endpoint_owned,
            "upsert_push_subscription starting"
        );
        tokio::task::spawn_blocking(move || -> Result<PushSubscription, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let now = Utc::now().naive_utc();
            let new_row = NewPushSubscription {
                tenant_id: &tenant_owned,
                child_id: &child_owned,
                endpoint: &endpoint_owned,
                p256dh: &p256dh_owned,
                auth: &auth_owned,
                created_at: now,
                updated_at: now,
            };
            diesel::insert_into(ps::push_subscriptions)
                .values(&new_row)
                .on_conflict(ps::endpoint)
                .do_update()
                .set((
                    ps::tenant_id.eq(&tenant_owned),
                    ps::child_id.eq(&child_owned),
                    ps::p256dh.eq(&p256dh_owned),
                    ps::auth.eq(&auth_owned),
                    ps::updated_at.eq(now),
                    ps::last_error.eq::<Option<String>>(None::<String>),
                    ps::last_success_at
                        .eq::<Option<chrono::NaiveDateTime>>(None::<chrono::NaiveDateTime>),
                ))
                .execute(&mut conn)?;
            Ok(ps::push_subscriptions
                .filter(ps::tenant_id.eq(&tenant_owned))
                .filter(ps::endpoint.eq(&endpoint_owned))
                .first::<PushSubscription>(&mut conn)?)
        })
        .await?
    }

    pub async fn list_push_subscriptions_for_child(
        &self,
        tenant_id: &str,
        child_id: &str,
    ) -> Result<Vec<PushSubscription>, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        let child_owned = child_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<PushSubscription>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(ps::push_subscriptions
                .filter(ps::tenant_id.eq(&tenant_owned))
                .filter(ps::child_id.eq(&child_owned))
                .order(ps::created_at.asc())
                .load::<PushSubscription>(&mut conn)?)
        })
        .await?
    }

    pub async fn list_all_push_subscriptions(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<PushSubscription>, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<PushSubscription>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(ps::push_subscriptions
                .filter(ps::tenant_id.eq(&tenant_owned))
                .order(ps::created_at.asc())
                .load::<PushSubscription>(&mut conn)?)
        })
        .await?
    }

    pub async fn push_subscription_count_for_child(
        &self,
        tenant_id: &str,
        child_id: &str,
    ) -> Result<i64, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        let child_owned = child_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<i64, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(ps::push_subscriptions
                .filter(ps::tenant_id.eq(&tenant_owned))
                .filter(ps::child_id.eq(&child_owned))
                .count()
                .get_result::<i64>(&mut conn)?)
        })
        .await?
    }

    pub async fn get_push_subscription_by_endpoint(
        &self,
        tenant_id: &str,
        endpoint: &str,
    ) -> Result<Option<PushSubscription>, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        let endpoint_owned = endpoint.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<PushSubscription>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(ps::push_subscriptions
                .filter(ps::tenant_id.eq(&tenant_owned))
                .filter(ps::endpoint.eq(&endpoint_owned))
                .first::<PushSubscription>(&mut conn)
                .optional()?)
        })
        .await?
    }

    pub async fn delete_push_subscription(
        &self,
        tenant_id: &str,
        child_id: &str,
        endpoint: &str,
    ) -> Result<bool, StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let tenant_owned = tenant_id.to_string();
        let child_owned = child_id.to_string();
        let endpoint_owned = endpoint.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let deleted = diesel::delete(
                ps::push_subscriptions
                    .filter(ps::tenant_id.eq(&tenant_owned))
                    .filter(ps::child_id.eq(&child_owned))
                    .filter(ps::endpoint.eq(&endpoint_owned)),
            )
            .execute(&mut conn)?;
            Ok(deleted > 0)
        })
        .await?
    }

    pub async fn mark_push_delivery_result(
        &self,
        id: i32,
        success: bool,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        use schema::push_subscriptions::dsl as ps;
        let pool = self.pool.clone();
        let error_owned = error.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let now = Utc::now().naive_utc();
            if success {
                diesel::update(ps::push_subscriptions.filter(ps::id.eq(id)))
                    .set((
                        ps::updated_at.eq(now),
                        ps::last_success_at.eq(Some(now)),
                        ps::last_error.eq::<Option<String>>(None::<String>),
                    ))
                    .execute(&mut conn)?;
            } else {
                diesel::update(ps::push_subscriptions.filter(ps::id.eq(id)))
                    .set((
                        ps::updated_at.eq(now),
                        ps::last_error.eq(error_owned.as_deref()),
                    ))
                    .execute(&mut conn)?;
            }
            Ok(())
        })
        .await?
    }

    pub async fn child_exists(&self, child: &str) -> Result<bool, StorageError> {
        use schema::children::dsl::*;
        let pool = self.pool.clone();
        let child_id = child.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let count: i64 = children
                .filter(id.eq(&child_id))
                .count()
                .get_result(&mut conn)?;
            Ok(count > 0)
        })
        .await?
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>, StorageError> {
        use schema::tasks::dsl::*;
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Task>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(tasks
                .filter(deleted_at.is_null())
                .order((priority.asc(), name.asc()))
                .load::<Task>(&mut conn)?)
        })
        .await?
    }

    /// Create a new task with optional child assignments.
    ///
    /// Generates a slug-based ID, derives `required` from `mandatory_days`,
    /// and inserts assignment rows if specific children are provided.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_task(
        &self,
        name_: &str,
        minutes_: i32,
        priority_: i32,
        mandatory_days_: i32,
        mandatory_start_time_: Option<&str>,
        assigned_children: Option<Vec<String>>,
    ) -> Result<Task, StorageError> {
        let pool = self.pool.clone();
        let name_owned = name_.to_string();
        let minutes_owned = minutes_;
        let priority_owned = priority_;
        let mandatory_days_owned = mandatory_days_;
        let mandatory_start_time_owned = mandatory_start_time_.map(|s| s.to_string());
        let children_owned = assigned_children;
        tokio::task::spawn_blocking(move || -> Result<Task, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            conn.immediate_transaction(|conn| -> Result<Task, StorageError> {
                let task_id = generate_task_id(&name_owned);
                let required = mandatory_days_owned != 0;
                let new_task = NewTask {
                    id: &task_id,
                    name: &name_owned,
                    minutes: minutes_owned,
                    required,
                    priority: priority_owned,
                    mandatory_days: mandatory_days_owned,
                    mandatory_start_time: mandatory_start_time_owned.as_deref(),
                };
                diesel::insert_into(schema::tasks::table)
                    .values(&new_task)
                    .execute(conn)?;

                sync_task_assignments_inner(conn, &task_id, &children_owned)?;

                let task = schema::tasks::table
                    .filter(schema::tasks::id.eq(&task_id))
                    .first::<Task>(conn)?;
                Ok(task)
            })
        })
        .await?
    }

    /// Update an existing task. Returns error if task not found or soft-deleted.
    ///
    /// Replaces all fields and reassigns children.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_task(
        &self,
        id_: &str,
        name_: &str,
        minutes_: i32,
        priority_: i32,
        mandatory_days_: i32,
        mandatory_start_time_: Option<&str>,
        assigned_children: Option<Vec<String>>,
    ) -> Result<Task, StorageError> {
        let pool = self.pool.clone();
        let id_owned = id_.to_string();
        let name_owned = name_.to_string();
        let minutes_owned = minutes_;
        let priority_owned = priority_;
        let mandatory_days_owned = mandatory_days_;
        let mandatory_start_time_owned = mandatory_start_time_.map(|s| s.to_string());
        let children_owned = assigned_children;
        tokio::task::spawn_blocking(move || -> Result<Task, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            conn.immediate_transaction(|conn| -> Result<Task, StorageError> {
                use schema::tasks::dsl::*;

                // Verify task exists and is not soft-deleted
                let existing: Option<Task> = tasks
                    .filter(id.eq(&id_owned))
                    .filter(deleted_at.is_null())
                    .first::<Task>(conn)
                    .optional()?;

                if existing.is_none() {
                    return Err(StorageError::InvalidInput(format!(
                        "task '{}' not found or deleted",
                        id_owned
                    )));
                }

                let now = Utc::now().naive_utc();
                let required_val = mandatory_days_owned != 0;

                diesel::update(tasks.filter(id.eq(&id_owned)))
                    .set((
                        name.eq(&name_owned),
                        minutes.eq(minutes_owned),
                        required.eq(required_val),
                        priority.eq(priority_owned),
                        mandatory_days.eq(mandatory_days_owned),
                        mandatory_start_time.eq(mandatory_start_time_owned.as_deref()),
                        updated_at.eq(now),
                    ))
                    .execute(conn)?;

                sync_task_assignments_inner(conn, &id_owned, &children_owned)?;

                let task = tasks.filter(id.eq(&id_owned)).first::<Task>(conn)?;
                Ok(task)
            })
        })
        .await?
    }

    /// Soft-delete a task and remove its pending submissions.
    ///
    /// Returns `true` if the task was found and deleted, `false` if not found.
    pub async fn soft_delete_task(&self, id_: &str) -> Result<bool, StorageError> {
        let pool = self.pool.clone();
        let id_owned = id_.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            conn.immediate_transaction(|conn| -> Result<bool, StorageError> {
                use schema::tasks::dsl::*;

                let now = Utc::now().naive_utc();
                let updated =
                    diesel::update(tasks.filter(id.eq(&id_owned)).filter(deleted_at.is_null()))
                        .set(deleted_at.eq(Some(now)))
                        .execute(conn)?;

                if updated == 0 {
                    return Ok(false);
                }

                // Delete pending task_submissions for this task
                diesel::delete(
                    schema::task_submissions::table
                        .filter(schema::task_submissions::task_id.eq(&id_owned)),
                )
                .execute(conn)?;

                Ok(true)
            })
        })
        .await?
    }

    /// Get a task with its assigned child IDs. Empty vec means all children.
    ///
    /// Excludes soft-deleted tasks.
    pub async fn get_task_with_assignments(
        &self,
        id_: &str,
    ) -> Result<Option<(Task, Vec<String>)>, StorageError> {
        let pool = self.pool.clone();
        let id_owned = id_.to_string();
        tokio::task::spawn_blocking(
            move || -> Result<Option<(Task, Vec<String>)>, StorageError> {
                let mut conn = pool.get()?;
                configure_sqlite_conn(&mut conn)?;
                use schema::tasks::dsl::*;

                let task_opt: Option<Task> = tasks
                    .filter(id.eq(&id_owned))
                    .filter(deleted_at.is_null())
                    .first::<Task>(&mut conn)
                    .optional()?;

                let Some(task) = task_opt else {
                    return Ok(None);
                };

                let assigned: Vec<String> = schema::task_assignments::table
                    .filter(schema::task_assignments::task_id.eq(&id_owned))
                    .select(schema::task_assignments::child_id)
                    .load::<String>(&mut conn)?;

                Ok(Some((task, assigned)))
            },
        )
        .await?
    }

    /// List tasks assigned to a specific child (or all children), with last_done timestamp.
    ///
    /// A task is assigned to a child if it has no rows in `task_assignments` (meaning all
    /// children) or has a specific row matching the child_id. Excludes soft-deleted tasks.
    pub async fn list_tasks_for_child(
        &self,
        child_id_: &str,
    ) -> Result<Vec<(Task, Option<NaiveDateTime>)>, StorageError> {
        let pool = self.pool.clone();
        let child_owned = child_id_.to_string();
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(Task, Option<NaiveDateTime>)>, StorageError> {
                let mut conn = pool.get()?;
                configure_sqlite_conn(&mut conn)?;

                // Get tasks that are either assigned to all children (no assignment rows)
                // or specifically assigned to this child
                let assigned_tasks: Vec<Task> = diesel::sql_query(
                    "SELECT t.id, t.name, t.minutes, t.required, t.priority, \
                     t.mandatory_days, t.mandatory_start_time, t.created_at, \
                     t.updated_at, t.deleted_at \
                     FROM tasks t \
                     WHERE t.deleted_at IS NULL \
                     AND (NOT EXISTS (SELECT 1 FROM task_assignments ta WHERE ta.task_id = t.id) \
                          OR EXISTS (SELECT 1 FROM task_assignments ta WHERE ta.task_id = t.id AND ta.child_id = ?)) \
                     ORDER BY t.priority ASC, t.name ASC",
                )
                .bind::<diesel::sql_types::Text, _>(&child_owned)
                .load::<Task>(&mut conn)?;

                // Fetch last done per task for this child
                use diesel::dsl::max;
                use schema::task_completions::dsl as tc;
                let rows: Vec<(String, Option<NaiveDateTime>)> = tc::task_completions
                    .filter(tc::child_id.eq(&child_owned))
                    .group_by(tc::task_id)
                    .select((tc::task_id, max(tc::done_at)))
                    .load::<(String, Option<NaiveDateTime>)>(&mut conn)?;
                let done_map: std::collections::HashMap<String, Option<NaiveDateTime>> =
                    rows.into_iter().collect();

                let out = assigned_tasks
                    .into_iter()
                    .map(|t| {
                        let ld = done_map.get(&t.id).cloned().unwrap_or(None);
                        (t, ld)
                    })
                    .collect();
                Ok(out)
            },
        )
        .await?
    }

    /// Seed children from config (always runs on startup).
    pub async fn seed_children_from_config(
        &self,
        cfg_children: &[gamiscreen_shared::domain::Child],
    ) -> Result<(), StorageError> {
        use schema::children;

        let pool = self.pool.clone();
        let children_owned = cfg_children.to_owned();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;

            for c in &children_owned {
                let new_child = NewChild {
                    id: &c.id,
                    display_name: &c.display_name,
                };
                diesel::insert_into(children::table)
                    .values(&new_child)
                    .on_conflict(children::id)
                    .do_update()
                    .set(children::display_name.eq(new_child.display_name))
                    .execute(&mut conn)?;

                // Ensure every child has a balances row
                diesel::insert_into(schema::balances::table)
                    .values(schema::balances::child_id.eq(&c.id))
                    .on_conflict_do_nothing()
                    .execute(&mut conn)?;
            }

            Ok(())
        })
        .await?
    }

    /// Migrate tasks from YAML config to DB (runs only if no non-deleted tasks exist).
    ///
    /// Preserves original YAML IDs. Sets `priority = 2`, derives `mandatory_days`
    /// from `required`, and assigns to all children (no assignment rows).
    pub async fn migrate_yaml_tasks_to_db(
        &self,
        cfg_tasks: &[gamiscreen_shared::domain::Task],
    ) -> Result<(), StorageError> {
        use schema::tasks;

        let pool = self.pool.clone();
        let tasks_owned = cfg_tasks.to_owned();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;

            // Check if any non-deleted tasks exist
            let existing_count: i64 = tasks::table
                .filter(tasks::deleted_at.is_null())
                .count()
                .get_result(&mut conn)?;

            if existing_count > 0 {
                info!(
                    count = existing_count,
                    "DB tasks exist, skipping YAML migration"
                );
                return Ok(());
            }

            if tasks_owned.is_empty() {
                return Ok(());
            }

            for t in &tasks_owned {
                let mandatory_days = if t.required { 127 } else { 0 };
                let mandatory_start_time = if t.required { Some("00:00") } else { None };
                let new_task = NewTask {
                    id: &t.id,
                    name: &t.name,
                    minutes: t.minutes,
                    required: t.required,
                    priority: 2,
                    mandatory_days,
                    mandatory_start_time,
                };
                diesel::insert_into(tasks::table)
                    .values(&new_task)
                    .on_conflict(tasks::id)
                    .do_nothing()
                    .execute(&mut conn)?;
            }

            info!(
                count = tasks_owned.len(),
                "Migrated tasks from YAML config to database"
            );
            Ok(())
        })
        .await?
    }

    pub async fn record_task_done(
        &self,
        child: &str,
        task: &str,
        by_username: &str,
    ) -> Result<(), StorageError> {
        let pool = self.pool.clone();
        let child = child.to_string();
        let task = task.to_string();
        let user = by_username.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            record_task_done_inner(&mut conn, &child, &task, &user)
        })
        .await?
    }

    pub async fn list_tasks_with_last_done(
        &self,
        child: &str,
    ) -> Result<Vec<(Task, Option<chrono::NaiveDateTime>)>, StorageError> {
        let pool = self.pool.clone();
        let child = child.to_string();
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(Task, Option<chrono::NaiveDateTime>)>, StorageError> {
                let mut conn = pool.get()?;
                configure_sqlite_conn(&mut conn)?;
                // Fetch tasks
                use crate::storage::schema::tasks::dsl as t;
                let ts = t::tasks.order(t::name.asc()).load::<Task>(&mut conn)?;
                // Fetch last done per task for child using Diesel aggregates
                use diesel::dsl::max;

                use crate::storage::schema::task_completions::dsl as tc;
                let rows: Vec<(String, Option<chrono::NaiveDateTime>)> = tc::task_completions
                    .filter(tc::child_id.eq(&child))
                    .group_by(tc::task_id)
                    .select((tc::task_id, max(tc::done_at)))
                    .load::<(String, Option<chrono::NaiveDateTime>)>(&mut conn)?;
                let mut map: std::collections::HashMap<String, Option<chrono::NaiveDateTime>> =
                    std::collections::HashMap::new();
                for (tid, last) in rows {
                    map.insert(tid, last);
                }
                let out = ts
                    .into_iter()
                    .map(|t| {
                        let ld = map.get(&t.id).cloned().unwrap_or(None);
                        (t, ld)
                    })
                    .collect();
                Ok(out)
            },
        )
        .await?
    }

    // Task submissions (pending approvals)
    pub async fn submit_task(&self, child: &str, task: &str) -> Result<(), StorageError> {
        use models::NewTaskSubmission;
        use schema::task_submissions;
        let pool = self.pool.clone();
        let c = child.to_string();
        let t = task.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let rec = NewTaskSubmission {
                child_id: &c,
                task_id: &t,
            };
            diesel::insert_into(task_submissions::table)
                .values(&rec)
                .execute(&mut conn)?;
            Ok(())
        })
        .await?
    }

    pub async fn list_pending_submissions(
        &self,
    ) -> Result<Vec<(models::TaskSubmission, Child, Task)>, StorageError> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(
            move || -> Result<Vec<(models::TaskSubmission, Child, Task)>, StorageError> {
                let mut conn = pool.get()?;
                configure_sqlite_conn(&mut conn)?;
                use crate::storage::schema::{children, task_submissions, tasks};
                let rows = task_submissions::table
                    .inner_join(children::table.on(children::id.eq(task_submissions::child_id)))
                    .inner_join(tasks::table.on(tasks::id.eq(task_submissions::task_id)))
                    .order(task_submissions::submitted_at.desc())
                    .select((
                        models::TaskSubmission::as_select(),
                        Child::as_select(),
                        Task::as_select(),
                    ))
                    .load::<(models::TaskSubmission, Child, Task)>(&mut conn)?;
                Ok(rows)
            },
        )
        .await?
    }

    pub async fn pending_submissions_count(&self) -> Result<i64, StorageError> {
        use schema::task_submissions::dsl as ts;
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<i64, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(ts::task_submissions.count().get_result(&mut conn)?)
        })
        .await?
    }

    /// Approve a task submission: insert reward, record completion, delete submission.
    /// Returns (child_id, new_remaining) if a submission was found.
    pub async fn approve_submission(
        &self,
        submission_id: i32,
        approver: &str,
    ) -> Result<Option<(String, i32)>, StorageError> {
        let pool = self.pool.clone();
        let approver = approver.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<(String, i32)>, StorageError> {
            use crate::storage::schema::{balances, rewards, task_submissions, tasks};
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let mut result: Option<(String, i32)> = None;
            conn.immediate_transaction(|conn| -> Result<(), StorageError> {
                let rec: Option<(String, String, i32, String)> = task_submissions::table
                    .inner_join(tasks::table.on(tasks::id.eq(task_submissions::task_id)))
                    .filter(task_submissions::id.eq(submission_id))
                    .select((
                        task_submissions::child_id,
                        task_submissions::task_id,
                        tasks::minutes,
                        tasks::name,
                    ))
                    .first::<(String, String, i32, String)>(conn)
                    .optional()?;
                let Some((child_id, task_id, mins, task_name)) = rec else {
                    return Ok(());
                };

                // Read current account_balance for debt tracking
                let account_balance: i32 = balances::table
                    .filter(balances::child_id.eq(&child_id))
                    .select(balances::account_balance)
                    .first(conn)?;

                let new_reward = NewReward {
                    child_id: &child_id,
                    task_id: Some(&task_id),
                    minutes: mins,
                    description: Some(&task_name),
                    is_borrowed: false,
                };
                diesel::insert_into(rewards::table)
                    .values(&new_reward)
                    .execute(conn)?;

                // Get the inserted reward id for balance_transactions FK
                let reward_id: i32 = diesel::select(
                    diesel::dsl::sql::<diesel::sql_types::Integer>("last_insert_rowid()"),
                )
                .get_result(conn)?;

                let (rem_delta, bal_delta) = apply_reward_to_balance(
                    conn,
                    &child_id,
                    mins,
                    false, // task approvals are never borrowed
                    account_balance,
                    reward_id,
                )?;

                diesel::update(balances::table.filter(balances::child_id.eq(&child_id)))
                    .set((
                        balances::minutes_remaining.eq(balances::minutes_remaining + rem_delta),
                        balances::account_balance.eq(balances::account_balance + bal_delta),
                    ))
                    .execute(conn)?;

                let new_remaining: i32 = balances::table
                    .filter(balances::child_id.eq(&child_id))
                    .select(balances::minutes_remaining)
                    .first(conn)?;
                record_task_done_inner(conn, &child_id, &task_id, &approver)?;
                diesel::delete(
                    task_submissions::table.filter(task_submissions::id.eq(submission_id)),
                )
                .execute(conn)?;
                result = Some((child_id, new_remaining));
                Ok(())
            })?;
            Ok(result)
        })
        .await?
    }

    pub async fn discard_submission(&self, submission_id: i32) -> Result<(), StorageError> {
        use schema::task_submissions::dsl as ts;
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let _ = diesel::delete(ts::task_submissions.filter(ts::id.eq(submission_id)))
                .execute(&mut conn)?;
            Ok(())
        })
        .await?
    }

    pub async fn list_rewards_for_child(
        &self,
        child: &str,
        page: usize,
        per_page: usize,
    ) -> Result<Vec<models::Reward>, StorageError> {
        let pool = self.pool.clone();
        let child = child.to_string();
        let page = page.max(1);
        let per_page = per_page.clamp(1, 1000) as i64;
        let offset = ((page as i64) - 1) * per_page;
        tokio::task::spawn_blocking(move || -> Result<Vec<models::Reward>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            use crate::storage::schema::rewards;
            // Only read from rewards; description is stored at creation time
            Ok(rewards::table
                .filter(rewards::child_id.eq(&child))
                .order(rewards::created_at.desc())
                .offset(offset)
                .limit(per_page)
                .select((
                    rewards::id,
                    rewards::child_id,
                    rewards::task_id,
                    rewards::minutes,
                    rewards::description,
                    rewards::created_at,
                    rewards::is_borrowed,
                ))
                .load::<models::Reward>(&mut conn)?)
        })
        .await?
    }

    pub async fn get_task_by_id(&self, id_: &str) -> Result<Option<Task>, StorageError> {
        use schema::tasks::dsl::*;
        let pool = self.pool.clone();
        let tid = id_.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Task>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(tasks
                .filter(id.eq(&tid))
                .first::<Task>(&mut conn)
                .optional()?)
        })
        .await?
    }

    /// Add a reward and optionally record task completion in a single transaction.
    ///
    /// When `task_completion` is `Some((task_id, approved_by))`, the task is marked
    /// done inside the same transaction as the reward, ensuring atomicity.
    pub async fn add_reward_minutes(
        &self,
        child_id: &str,
        mins: i32,
        task: Option<&str>,
        description: Option<&str>,
        is_borrowed: bool,
        task_completion: Option<(&str, &str)>,
    ) -> Result<i32, StorageError> {
        use schema::{balances, rewards};
        let pool = self.pool.clone();
        let child = child_id.to_string();
        let task_opt = task.map(|s| s.to_string());
        let description_opt = description.map(|s| s.to_string());
        let completion_opt = task_completion.map(|(tid, user)| (tid.to_string(), user.to_string()));
        tokio::task::spawn_blocking(move || -> Result<i32, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            conn.immediate_transaction(|conn| -> Result<i32, StorageError> {
                // Read current account_balance for debt tracking
                let account_balance: i32 = balances::table
                    .filter(balances::child_id.eq(&child))
                    .select(balances::account_balance)
                    .first(conn)?;

                // Insert reward row — is_borrowed is a display flag for "(lent)" labels in UI
                let new_reward = NewReward {
                    child_id: &child,
                    task_id: task_opt.as_deref(),
                    minutes: mins,
                    description: description_opt.as_deref(),
                    is_borrowed,
                };
                diesel::insert_into(rewards::table)
                    .values(&new_reward)
                    .execute(conn)?;

                // Get the inserted reward id for balance_transactions FK
                let reward_id: i32 = diesel::select(
                    diesel::dsl::sql::<diesel::sql_types::Integer>("last_insert_rowid()"),
                )
                .get_result(conn)?;

                let (rem_delta, bal_delta) = apply_reward_to_balance(
                    conn,
                    &child,
                    mins,
                    is_borrowed,
                    account_balance,
                    reward_id,
                )?;

                diesel::update(balances::table.filter(balances::child_id.eq(&child)))
                    .set((
                        balances::minutes_remaining.eq(balances::minutes_remaining + rem_delta),
                        balances::account_balance.eq(balances::account_balance + bal_delta),
                    ))
                    .execute(conn)?;

                if let Some((ref tid, ref user)) = completion_opt {
                    record_task_done_inner(conn, &child, tid, user)?;
                }

                let new_remaining: i32 = balances::table
                    .filter(balances::child_id.eq(&child))
                    .select(balances::minutes_remaining)
                    .first(conn)?;
                Ok(new_remaining)
            })
        })
        .await?
    }

    pub async fn process_usage_minutes(
        &self,
        child: &str,
        device: &str,
        minutes: &[i64],
    ) -> Result<i32, StorageError> {
        use schema::{balances, usage_minutes};

        use crate::storage::models::NewUsageMinute;
        if minutes.is_empty() {
            return Err(StorageError::InvalidInput(
                "no minutes provided".to_string(),
            ));
        }
        // Validate timestamp bounds to prevent retroactive inflation or
        // future-timestamp poisoning of screen-time accounting.
        let now_minute = Utc::now().timestamp() / 60;
        let window_past = 7 * 24 * 60; // 7 days back
        let window_future = 5; // 5 minutes forward (clock skew tolerance)
        if minutes
            .iter()
            .any(|&m| m < now_minute - window_past || m > now_minute + window_future)
        {
            return Err(StorageError::InvalidInput(
                "minute timestamp out of acceptable range".to_string(),
            ));
        }
        let pool = self.pool.clone();
        let child_owned = child.to_string();
        let device_owned = device.to_string();
        let minutes_vec = minutes.to_vec();
        tokio::task::spawn_blocking(move || -> Result<i32, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            conn.immediate_transaction(|conn| -> Result<i32, StorageError> {
                let mut new_count = 0i32;
                for m in &minutes_vec {
                    let row = NewUsageMinute {
                        child_id: &child_owned,
                        minute_ts: *m,
                        device_id: &device_owned,
                    };
                    let inserted = diesel::insert_into(usage_minutes::table)
                        .values(&row)
                        .on_conflict_do_nothing()
                        .execute(conn)?;
                    new_count += inserted as i32;
                }
                if new_count > 0 {
                    diesel::update(balances::table.filter(balances::child_id.eq(&child_owned)))
                        .set(
                            balances::minutes_remaining.eq(balances::minutes_remaining - new_count),
                        )
                        .execute(conn)?;
                }
                let new_remaining: i32 = balances::table
                    .filter(balances::child_id.eq(&child_owned))
                    .select(balances::minutes_remaining)
                    .first(conn)?;
                Ok(new_remaining)
            })
        })
        .await?
    }

    pub async fn list_usage_minutes(
        &self,
        child: &str,
        minute_from: i64,
        minute_to: i64,
    ) -> Result<Vec<i64>, StorageError> {
        use schema::usage_minutes::dsl as um;
        if minute_to <= minute_from {
            return Ok(Vec::new());
        }
        let pool = self.pool.clone();
        let child_owned = child.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<i64>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(um::usage_minutes
                .filter(um::child_id.eq(&child_owned))
                .filter(um::minute_ts.ge(minute_from))
                .filter(um::minute_ts.lt(minute_to))
                .select(um::minute_ts)
                .distinct()
                .order(um::minute_ts.asc())
                .load::<i64>(&mut conn)?)
        })
        .await?
    }

    pub async fn get_remaining(&self, child_id: &str) -> Result<i32, StorageError> {
        use schema::balances;
        let pool = self.pool.clone();
        let child = child_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<i32, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(balances::table
                .filter(balances::child_id.eq(&child))
                .select(balances::minutes_remaining)
                .first(&mut conn)?)
        })
        .await?
    }

    pub async fn compute_balance(&self, child_id: &str) -> Result<i32, StorageError> {
        let pool = self.pool.clone();
        let child = child_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<i32, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            compute_balance_inner(&mut conn, &child)
        })
        .await?
    }

    pub async fn all_required_tasks_done_today(
        &self,
        child_id: &str,
    ) -> Result<bool, StorageError> {
        let pool = self.pool.clone();
        let child = child_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            all_required_tasks_done_today_inner(&mut conn, &child)
        })
        .await?
    }

    // Session helpers for JWT inactivity windows
    pub async fn create_session(&self, jti_: &str, username_: &str) -> Result<(), StorageError> {
        use schema::sessions;
        let pool = self.pool.clone();
        let j = jti_.to_string();
        let u = username_.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let new = NewSession {
                jti: &j,
                username: &u,
            };
            diesel::insert_into(sessions::table)
                .values(&new)
                .on_conflict_do_nothing()
                .execute(&mut conn)?;
            Ok(())
        })
        .await?
    }

    pub async fn get_session(&self, jti_: &str) -> Result<Option<Session>, StorageError> {
        use schema::sessions::dsl::*;
        let pool = self.pool.clone();
        let j = jti_.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Session>, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            Ok(sessions
                .filter(jti.eq(&j))
                .first::<Session>(&mut conn)
                .optional()?)
        })
        .await?
    }

    pub async fn delete_session(&self, jti_: &str) -> Result<bool, StorageError> {
        use schema::sessions::dsl::*;
        let pool = self.pool.clone();
        let j = jti_.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let deleted = diesel::delete(sessions.filter(jti.eq(&j))).execute(&mut conn)?;
            Ok(deleted > 0)
        })
        .await?
    }

    /// Touch session atomically, but only if it hasn't expired.
    /// Returns `true` if the session was found and updated, `false` otherwise.
    ///
    /// This combines the idle timeout check and the `last_used_at` update into
    /// a single atomic UPDATE, eliminating the race condition between checking
    /// and updating the session.
    pub async fn touch_session_with_cutoff(
        &self,
        jti_: &str,
        cutoff: chrono::NaiveDateTime,
    ) -> Result<bool, StorageError> {
        use schema::sessions::dsl::*;
        let pool = self.pool.clone();
        let j = jti_.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let now = Utc::now().naive_utc();
            let updated =
                diesel::update(sessions.filter(jti.eq(&j)).filter(last_used_at.ge(cutoff)))
                    .set(last_used_at.eq(now))
                    .execute(&mut conn)?;
            Ok(updated > 0)
        })
        .await?
    }
}

fn record_task_done_inner(
    conn: &mut SqliteConnection,
    child_id: &str,
    task_id: &str,
    by_username: &str,
) -> Result<(), StorageError> {
    use models::NewTaskCompletion;
    use schema::task_completions;
    let rec = NewTaskCompletion {
        child_id,
        task_id,
        by_username,
    };
    diesel::insert_into(task_completions::table)
        .values(&rec)
        .execute(conn)?;
    Ok(())
}

/// Read the stored account balance (virtual bank) for a child.
///
/// This is a simple column read — no computation. The account_balance is
/// maintained transactionally by `add_reward_minutes` and `approve_submission`.
/// Negative values indicate debt from borrowed time.
fn compute_balance_inner(conn: &mut SqliteConnection, child_id: &str) -> Result<i32, StorageError> {
    use schema::balances;
    Ok(balances::table
        .filter(balances::child_id.eq(child_id))
        .select(balances::account_balance)
        .first(conn)?)
}

// Note: required tasks are global, not per-child. All children must complete all required tasks.
// Day boundary uses UTC. Configure server timezone or document for users in non-UTC zones.
fn all_required_tasks_done_today_inner(
    conn: &mut SqliteConnection,
    child_id: &str,
) -> Result<bool, StorageError> {
    let today = Utc::now().date_naive();
    let today_start = today
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight timestamp");
    let tomorrow_start = (today + chrono::Days::new(1))
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight timestamp");

    let required_task_ids: Vec<String> = schema::tasks::table
        .filter(schema::tasks::required.eq(true))
        .select(schema::tasks::id)
        .load(conn)?;

    if required_task_ids.is_empty() {
        return Ok(true);
    }

    let completed_task_ids: Vec<String> = schema::task_completions::table
        .filter(schema::task_completions::child_id.eq(child_id))
        .filter(schema::task_completions::done_at.ge(today_start))
        .filter(schema::task_completions::done_at.lt(tomorrow_start))
        .select(schema::task_completions::task_id)
        .distinct()
        .load(conn)?;

    Ok(required_task_ids
        .iter()
        .all(|tid| completed_task_ids.contains(tid)))
}

/// Apply reward/penalty logic to balances and record balance transactions.
///
/// Returns `(remaining_delta, balance_delta)` — the changes to apply to the
/// stored `minutes_remaining` and `account_balance` columns respectively.
///
/// This also inserts the appropriate `balance_transactions` row(s) for audit.
fn apply_reward_to_balance(
    conn: &mut SqliteConnection,
    child_id: &str,
    mins: i32,
    is_borrowed: bool,
    account_balance: i32,
    reward_id: i32,
) -> Result<(i32, i32), StorageError> {
    use models::NewBalanceTransaction;
    use schema::balance_transactions;

    if is_borrowed {
        // LEND: remaining goes up, account_balance goes down (debt)
        diesel::insert_into(balance_transactions::table)
            .values(&NewBalanceTransaction {
                child_id,
                amount: -mins,
                description: Some("Lent time"),
                related_reward_id: Some(reward_id),
            })
            .execute(conn)?;
        Ok((mins, -mins))
    } else if mins < 0 {
        // PENALTY: remaining goes down directly, balance untouched
        Ok((mins, 0))
    } else {
        // EARN: repay debt first, then add surplus to remaining
        if account_balance < 0 {
            let repay = mins.min(account_balance.saturating_abs());
            let surplus = mins - repay;
            diesel::insert_into(balance_transactions::table)
                .values(&NewBalanceTransaction {
                    child_id,
                    amount: repay,
                    description: Some("Auto-repayment"),
                    related_reward_id: Some(reward_id),
                })
                .execute(conn)?;
            Ok((surplus, repay))
        } else {
            // No debt — full amount goes to remaining
            Ok((mins, 0))
        }
    }
}

/// Sync task assignments: replace all assignment rows for a task.
///
/// `None` or empty vec means "all children" (delete all rows).
/// `Some(vec)` means specific children (delete existing, insert new).
fn sync_task_assignments_inner(
    conn: &mut SqliteConnection,
    task_id: &str,
    assigned_children: &Option<Vec<String>>,
) -> Result<(), StorageError> {
    // Always delete existing assignments first
    diesel::delete(
        schema::task_assignments::table.filter(schema::task_assignments::task_id.eq(task_id)),
    )
    .execute(conn)?;

    // Insert new assignments if specific children are provided
    if let Some(children) = assigned_children
        && !children.is_empty()
    {
        for child_id in children {
            let assignment = NewTaskAssignment { task_id, child_id };
            diesel::insert_into(schema::task_assignments::table)
                .values(&assignment)
                .execute(conn)?;
        }
    }

    Ok(())
}

fn configure_sqlite_conn(conn: &mut SqliteConnection) -> Result<(), diesel::result::Error> {
    // Enable WAL for better read/write concurrency and set a busy timeout
    // Ignore the result rows; Diesel's execute is fine for PRAGMAs
    diesel::sql_query("PRAGMA journal_mode=WAL;").execute(conn)?;
    diesel::sql_query("PRAGMA synchronous=NORMAL;").execute(conn)?;
    diesel::sql_query("PRAGMA busy_timeout=5000;").execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use diesel::sqlite::SqliteConnection;

    use super::*;

    /// Create an in-memory SQLite database with the schema for testing.
    fn setup_test_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to create test database");
        // Create tables needed for balance tests
        diesel::sql_query(
            "CREATE TABLE children (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE balances (
                child_id TEXT PRIMARY KEY REFERENCES children(id),
                minutes_remaining INTEGER NOT NULL DEFAULT 0,
                account_balance INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                minutes INTEGER NOT NULL,
                required INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE rewards (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL REFERENCES children(id),
                task_id TEXT REFERENCES tasks(id),
                minutes INTEGER NOT NULL,
                description TEXT,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                is_borrowed INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE balance_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL REFERENCES children(id),
                amount INTEGER NOT NULL,
                description TEXT,
                related_reward_id INTEGER REFERENCES rewards(id),
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE usage_minutes (
                child_id TEXT NOT NULL,
                minute_ts BIGINT NOT NULL,
                device_id TEXT NOT NULL,
                PRIMARY KEY (child_id, minute_ts, device_id)
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE task_completions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                by_username TEXT NOT NULL,
                done_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();

        // Insert a test child and balance row
        diesel::sql_query(
            "INSERT INTO children (id, display_name) VALUES ('child1', 'Test Child')",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "INSERT INTO balances (child_id, minutes_remaining, account_balance) VALUES ('child1', 0, 0)",
        )
        .execute(&mut conn)
        .unwrap();
        conn
    }

    /// Helper: insert a reward and apply balance logic, returning (remaining, account_balance).
    fn do_reward(
        conn: &mut SqliteConnection,
        child_id: &str,
        mins: i32,
        is_borrowed: bool,
    ) -> (i32, i32) {
        use schema::{balances, rewards};

        // Read current account_balance
        let acct: i32 = balances::table
            .filter(balances::child_id.eq(child_id))
            .select(balances::account_balance)
            .first(conn)
            .unwrap();

        // Insert reward row
        let new_reward = models::NewReward {
            child_id,
            task_id: None,
            minutes: mins,
            description: Some("test"),
            is_borrowed, // display flag for "(lent)" labels in reward history
        };
        diesel::insert_into(rewards::table)
            .values(&new_reward)
            .execute(conn)
            .unwrap();

        // Get the inserted reward id
        let reward_id: i32 = diesel::sql_query("SELECT last_insert_rowid() as id")
            .load::<LastInsertRowId>(conn)
            .unwrap()
            .first()
            .unwrap()
            .id;

        let (rem_delta, bal_delta) =
            apply_reward_to_balance(conn, child_id, mins, is_borrowed, acct, reward_id).unwrap();

        diesel::update(balances::table.filter(balances::child_id.eq(child_id)))
            .set((
                balances::minutes_remaining.eq(balances::minutes_remaining + rem_delta),
                balances::account_balance.eq(balances::account_balance + bal_delta),
            ))
            .execute(conn)
            .unwrap();

        let (r, b): (i32, i32) = balances::table
            .filter(balances::child_id.eq(child_id))
            .select((balances::minutes_remaining, balances::account_balance))
            .first(conn)
            .unwrap();
        (r, b)
    }

    // Helper struct for last_insert_rowid query
    #[derive(QueryableByName)]
    struct LastInsertRowId {
        #[diesel(sql_type = diesel::sql_types::Integer)]
        id: i32,
    }

    #[test]
    fn earn_with_no_debt_full_to_remaining() {
        let mut conn = setup_test_db();
        let (rem, bal) = do_reward(&mut conn, "child1", 10, false);
        assert_eq!(rem, 10, "all earned minutes go to remaining");
        assert_eq!(bal, 0, "no debt, balance stays 0");
    }

    #[test]
    fn earn_with_partial_debt_split() {
        let mut conn = setup_test_db();
        // Create debt by lending 15 minutes
        let (rem, bal) = do_reward(&mut conn, "child1", 15, true);
        assert_eq!(rem, 15, "lent time added to remaining");
        assert_eq!(bal, -15, "debt created");

        // Earn 20 minutes — 15 repays debt, 5 goes to remaining
        let (rem, bal) = do_reward(&mut conn, "child1", 20, false);
        assert_eq!(rem, 20, "remaining = 15 + 5 surplus");
        assert_eq!(bal, 0, "debt fully repaid");
    }

    #[test]
    fn earn_with_debt_exceeding_earnings() {
        let mut conn = setup_test_db();
        // Create debt by lending 20 minutes
        let (rem, bal) = do_reward(&mut conn, "child1", 20, true);
        assert_eq!(rem, 20);
        assert_eq!(bal, -20);

        // Earn 5 minutes — all goes to repayment, no remaining change
        let (rem, bal) = do_reward(&mut conn, "child1", 5, false);
        assert_eq!(rem, 20, "remaining unchanged, all earnings to repayment");
        assert_eq!(bal, -15, "debt partially repaid");
    }

    #[test]
    fn lend_increases_remaining_decreases_balance() {
        let mut conn = setup_test_db();
        let (rem, bal) = do_reward(&mut conn, "child1", 10, true);
        assert_eq!(rem, 10, "lent time goes to remaining");
        assert_eq!(bal, -10, "debt created");
    }

    #[test]
    fn penalty_reduces_remaining_balance_unchanged() {
        let mut conn = setup_test_db();
        // First earn some time
        let (rem, bal) = do_reward(&mut conn, "child1", 20, false);
        assert_eq!(rem, 20);
        assert_eq!(bal, 0);

        // Apply penalty
        let (rem, bal) = do_reward(&mut conn, "child1", -5, false);
        assert_eq!(rem, 15, "penalty reduces remaining");
        assert_eq!(bal, 0, "penalty does not affect balance");
    }

    #[test]
    fn penalty_during_debt_remaining_down_balance_unchanged() {
        let mut conn = setup_test_db();
        // Lend 10 (remaining=10, balance=-10)
        let (rem, bal) = do_reward(&mut conn, "child1", 10, true);
        assert_eq!(rem, 10);
        assert_eq!(bal, -10);

        // Penalty of 3 (remaining=7, balance still -10)
        let (rem, bal) = do_reward(&mut conn, "child1", -3, false);
        assert_eq!(rem, 7, "penalty reduces remaining even during debt");
        assert_eq!(bal, -10, "penalty never touches balance");
    }

    #[test]
    fn full_scenario_lend_earn_penalty_sequence() {
        let mut conn = setup_test_db();

        // Earn 20 (remaining=20, balance=0)
        let (rem, bal) = do_reward(&mut conn, "child1", 20, false);
        assert_eq!((rem, bal), (20, 0));

        // Penalty -15 (remaining=5, balance=0)
        let (rem, bal) = do_reward(&mut conn, "child1", -15, false);
        assert_eq!((rem, bal), (5, 0));

        // Lend 10 (remaining=15, balance=-10)
        let (rem, bal) = do_reward(&mut conn, "child1", 10, true);
        assert_eq!((rem, bal), (15, -10));

        // Earn 5 (all to repayment, remaining=15, balance=-5)
        let (rem, bal) = do_reward(&mut conn, "child1", 5, false);
        assert_eq!((rem, bal), (15, -5));

        // Earn 8 (5 to repayment, 3 to remaining, remaining=18, balance=0)
        let (rem, bal) = do_reward(&mut conn, "child1", 8, false);
        assert_eq!((rem, bal), (18, 0));

        // Penalty -3 during zero balance (remaining=15, balance=0)
        let (rem, bal) = do_reward(&mut conn, "child1", -3, false);
        assert_eq!((rem, bal), (15, 0));
    }

    // ── Task CRUD tests ──────────────────────────────────────────────

    /// Setup a test DB with the full schema including new task columns and task_assignments.
    fn setup_task_crud_db() -> SqliteConnection {
        let mut conn =
            SqliteConnection::establish(":memory:").expect("Failed to create test database");
        diesel::sql_query(
            "CREATE TABLE children (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE balances (
                child_id TEXT PRIMARY KEY REFERENCES children(id),
                minutes_remaining INTEGER NOT NULL DEFAULT 0,
                account_balance INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                minutes INTEGER NOT NULL,
                required INTEGER NOT NULL DEFAULT 0,
                priority INTEGER NOT NULL DEFAULT 2,
                mandatory_days INTEGER NOT NULL DEFAULT 0,
                mandatory_start_time TEXT,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                deleted_at TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE task_assignments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                child_id TEXT NOT NULL REFERENCES children(id) ON DELETE CASCADE,
                UNIQUE(task_id, child_id)
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE task_completions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                by_username TEXT NOT NULL,
                done_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE task_submissions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                submitted_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE rewards (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL REFERENCES children(id),
                task_id TEXT REFERENCES tasks(id),
                minutes INTEGER NOT NULL,
                description TEXT,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                is_borrowed INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE balance_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                child_id TEXT NOT NULL REFERENCES children(id),
                amount INTEGER NOT NULL,
                description TEXT,
                related_reward_id INTEGER REFERENCES rewards(id),
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE usage_minutes (
                child_id TEXT NOT NULL,
                minute_ts BIGINT NOT NULL,
                device_id TEXT NOT NULL,
                PRIMARY KEY (child_id, minute_ts, device_id)
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE sessions (
                jti TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                issued_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                last_used_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(
            "CREATE TABLE push_subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tenant_id TEXT NOT NULL,
                child_id TEXT NOT NULL REFERENCES children(id),
                endpoint TEXT NOT NULL UNIQUE,
                p256dh TEXT NOT NULL,
                auth TEXT NOT NULL,
                created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                last_success_at TIMESTAMP,
                last_error TEXT
            )",
        )
        .execute(&mut conn)
        .unwrap();
        // Seed test children
        diesel::sql_query("INSERT INTO children (id, display_name) VALUES ('child1', 'Alice')")
            .execute(&mut conn)
            .unwrap();
        diesel::sql_query("INSERT INTO children (id, display_name) VALUES ('child2', 'Bob')")
            .execute(&mut conn)
            .unwrap();
        diesel::sql_query("INSERT INTO balances (child_id) VALUES ('child1')")
            .execute(&mut conn)
            .unwrap();
        diesel::sql_query("INSERT INTO balances (child_id) VALUES ('child2')")
            .execute(&mut conn)
            .unwrap();
        conn
    }

    #[tokio::test]
    async fn create_task_with_all_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");
        // Seed children
        let children = vec![gamiscreen_shared::domain::Child {
            id: "alice".into(),
            display_name: "Alice".into(),
        }];
        store.seed_children_from_config(&children).await.unwrap();

        let task = store
            .create_task(
                "Homework",
                30,
                1,
                31,
                Some("15:00"),
                Some(vec!["alice".into()]),
            )
            .await
            .expect("create_task");

        assert!(task.id.starts_with("homework-"));
        assert_eq!(task.name, "Homework");
        assert_eq!(task.minutes, 30);
        assert_eq!(task.priority, 1);
        assert_eq!(task.mandatory_days, 31);
        assert_eq!(task.mandatory_start_time.as_deref(), Some("15:00"));
        assert!(task.required); // mandatory_days != 0
        assert!(task.deleted_at.is_none());
    }

    #[tokio::test]
    async fn create_task_with_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let task = store
            .create_task("Play", 15, 2, 0, None, None)
            .await
            .expect("create_task");

        assert_eq!(task.priority, 2);
        assert_eq!(task.mandatory_days, 0);
        assert!(task.mandatory_start_time.is_none());
        assert!(!task.required); // mandatory_days == 0
    }

    #[tokio::test]
    async fn update_task_changes_persisted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let task = store
            .create_task("Old Name", 10, 2, 0, None, None)
            .await
            .expect("create");

        let updated = store
            .update_task(&task.id, "New Name", 20, 1, 127, Some("08:00"), None)
            .await
            .expect("update");

        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.minutes, 20);
        assert_eq!(updated.priority, 1);
        assert_eq!(updated.mandatory_days, 127);
        assert_eq!(updated.mandatory_start_time.as_deref(), Some("08:00"));
        assert!(updated.required);
        assert!(updated.updated_at > task.updated_at || updated.updated_at == task.updated_at);
    }

    #[tokio::test]
    async fn soft_delete_excludes_from_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let task = store
            .create_task("To Delete", 5, 2, 0, None, None)
            .await
            .expect("create");

        let deleted = store.soft_delete_task(&task.id).await.expect("delete");
        assert!(deleted);

        let tasks = store.list_tasks().await.expect("list");
        assert!(
            tasks.is_empty(),
            "soft-deleted task should not appear in list"
        );

        // Deleting again returns false
        let deleted_again = store
            .soft_delete_task(&task.id)
            .await
            .expect("delete again");
        assert!(!deleted_again);
    }

    #[tokio::test]
    async fn list_tasks_ordered_by_priority_then_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        // Create tasks with varying priorities and names
        store
            .create_task("Zebra", 5, 3, 0, None, None)
            .await
            .unwrap();
        store
            .create_task("Apple", 5, 1, 0, None, None)
            .await
            .unwrap();
        store
            .create_task("Banana", 5, 1, 0, None, None)
            .await
            .unwrap();
        store
            .create_task("Cherry", 5, 2, 0, None, None)
            .await
            .unwrap();

        let tasks = store.list_tasks().await.unwrap();
        let names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Apple", "Banana", "Cherry", "Zebra"]);
    }

    #[tokio::test]
    async fn task_assignments_specific_children() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");
        let children = vec![
            gamiscreen_shared::domain::Child {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            gamiscreen_shared::domain::Child {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ];
        store.seed_children_from_config(&children).await.unwrap();

        // Create task assigned to alice only
        let task = store
            .create_task("Math", 20, 2, 0, None, Some(vec!["alice".into()]))
            .await
            .unwrap();

        let (_, assigned) = store
            .get_task_with_assignments(&task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(assigned, vec!["alice"]);

        // Update to all children (None)
        store
            .update_task(&task.id, "Math", 20, 2, 0, None, None)
            .await
            .unwrap();

        let (_, assigned) = store
            .get_task_with_assignments(&task.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            assigned.is_empty(),
            "None means all children -> no assignment rows"
        );
    }

    #[tokio::test]
    async fn list_tasks_for_child_filters_by_assignment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");
        let children = vec![
            gamiscreen_shared::domain::Child {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            gamiscreen_shared::domain::Child {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ];
        store.seed_children_from_config(&children).await.unwrap();

        // Task for all children
        store
            .create_task("Brush Teeth", 5, 1, 0, None, None)
            .await
            .unwrap();
        // Task for alice only
        store
            .create_task("Piano", 30, 2, 0, None, Some(vec!["alice".into()]))
            .await
            .unwrap();
        // Task for bob only
        store
            .create_task("Soccer", 45, 2, 0, None, Some(vec!["bob".into()]))
            .await
            .unwrap();

        let alice_tasks = store.list_tasks_for_child("alice").await.unwrap();
        let alice_names: Vec<&str> = alice_tasks.iter().map(|(t, _)| t.name.as_str()).collect();
        assert!(
            alice_names.contains(&"Brush Teeth"),
            "all-children task visible to alice"
        );
        assert!(
            alice_names.contains(&"Piano"),
            "alice-specific task visible"
        );
        assert!(
            !alice_names.contains(&"Soccer"),
            "bob-specific task not visible to alice"
        );

        let bob_tasks = store.list_tasks_for_child("bob").await.unwrap();
        let bob_names: Vec<&str> = bob_tasks.iter().map(|(t, _)| t.name.as_str()).collect();
        assert!(bob_names.contains(&"Brush Teeth"));
        assert!(bob_names.contains(&"Soccer"));
        assert!(!bob_names.contains(&"Piano"));
    }

    #[tokio::test]
    async fn yaml_migration_skips_when_db_has_tasks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        // Create a task in DB first
        store
            .create_task("Existing", 10, 2, 0, None, None)
            .await
            .unwrap();

        // Attempt YAML migration
        let yaml_tasks = vec![gamiscreen_shared::domain::Task {
            id: "yaml-task".into(),
            name: "YAML Task".into(),
            minutes: 15,
            required: true,
        }];
        store.migrate_yaml_tasks_to_db(&yaml_tasks).await.unwrap();

        // Only the DB task should exist
        let tasks = store.list_tasks().await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "Existing");
    }

    #[tokio::test]
    async fn yaml_migration_runs_when_db_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let yaml_tasks = vec![
            gamiscreen_shared::domain::Task {
                id: "brush-teeth".into(),
                name: "Brush teeth".into(),
                minutes: 10,
                required: true,
            },
            gamiscreen_shared::domain::Task {
                id: "read".into(),
                name: "Read".into(),
                minutes: 20,
                required: false,
            },
        ];
        store.migrate_yaml_tasks_to_db(&yaml_tasks).await.unwrap();

        let tasks = store.list_tasks().await.unwrap();
        assert_eq!(tasks.len(), 2);

        // Required task should have mandatory_days=127
        let brush = tasks.iter().find(|t| t.id == "brush-teeth").unwrap();
        assert_eq!(brush.mandatory_days, 127);
        assert_eq!(brush.mandatory_start_time.as_deref(), Some("00:00"));
        assert!(brush.required);
        assert_eq!(brush.priority, 2);

        // Non-required task should have mandatory_days=0
        let read = tasks.iter().find(|t| t.id == "read").unwrap();
        assert_eq!(read.mandatory_days, 0);
        assert!(read.mandatory_start_time.is_none());
        assert!(!read.required);
    }

    #[tokio::test]
    async fn yaml_migration_preserves_original_ids() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let yaml_tasks = vec![gamiscreen_shared::domain::Task {
            id: "my-custom-id".into(),
            name: "Custom".into(),
            minutes: 5,
            required: false,
        }];
        store.migrate_yaml_tasks_to_db(&yaml_tasks).await.unwrap();

        let tasks = store.list_tasks().await.unwrap();
        assert_eq!(tasks[0].id, "my-custom-id");
    }

    #[tokio::test]
    async fn update_nonexistent_task_returns_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let result = store
            .update_task("nonexistent", "Name", 10, 2, 0, None, None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn soft_delete_removes_pending_submissions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");
        let children = vec![gamiscreen_shared::domain::Child {
            id: "alice".into(),
            display_name: "Alice".into(),
        }];
        store.seed_children_from_config(&children).await.unwrap();

        let task = store
            .create_task("Test", 10, 2, 0, None, None)
            .await
            .unwrap();
        store.submit_task("alice", &task.id).await.unwrap();

        let count_before = store.pending_submissions_count().await.unwrap();
        assert_eq!(count_before, 1);

        store.soft_delete_task(&task.id).await.unwrap();

        let count_after = store.pending_submissions_count().await.unwrap();
        assert_eq!(count_after, 0);
    }

    #[test]
    fn earn_clears_debt_surplus_goes_to_remaining() {
        let mut conn = setup_test_db();
        // Lend 3 (remaining=3, balance=-3)
        let (rem, bal) = do_reward(&mut conn, "child1", 3, true);
        assert_eq!((rem, bal), (3, -3));

        // Earn 8 (3 repays debt, 5 to remaining -> remaining=8, balance=0)
        let (rem, bal) = do_reward(&mut conn, "child1", 8, false);
        assert_eq!(rem, 8, "3 from lend + 5 surplus from earn");
        assert_eq!(bal, 0, "debt fully repaid");
    }

    /// Integration test: balance after earning 1 min, borrowing 5, and using 6.
    ///
    /// With the new system: account_balance tracks debt from borrowing,
    /// usage only affects minutes_remaining (not account_balance).
    #[tokio::test]
    async fn balance_after_loan_and_full_usage() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");
        let store = crate::storage::Store::connect_sqlite(db_path.to_str().unwrap())
            .await
            .expect("connect");

        let child = gamiscreen_shared::domain::Child {
            id: "kid1".into(),
            display_name: "Kid".into(),
        };
        store.seed_from_config(&[child], &[]).await.expect("seed");

        // 1. Earn 1 minute (remaining=1, balance=0)
        store
            .add_reward_minutes("kid1", 1, None, Some("earned"), false, None)
            .await
            .expect("earn 1 min");

        let balance = store.compute_balance("kid1").await.expect("balance");
        assert_eq!(balance, 0, "no debt after pure earning");

        // 2. Borrow 5 minutes (remaining=6, balance=-5)
        store
            .add_reward_minutes("kid1", 5, None, Some("loan"), true, None)
            .await
            .expect("borrow 5 min");

        let remaining = store.get_remaining("kid1").await.expect("remaining");
        assert_eq!(remaining, 6, "remaining after earn 1 + borrow 5");
        let balance = store.compute_balance("kid1").await.expect("balance");
        assert_eq!(balance, -5, "debt from borrowing 5");

        // 3. Use 6 minutes (remaining=0, balance=-5 — usage doesn't touch balance)
        let now_epoch_min = chrono::Utc::now().timestamp() / 60;
        let usage: Vec<i64> = (0..6).map(|i| now_epoch_min - i).collect();
        store
            .process_usage_minutes("kid1", "dev1", &usage)
            .await
            .expect("use 6 min");

        let remaining = store.get_remaining("kid1").await.expect("remaining");
        assert_eq!(remaining, 0, "remaining after using all 6 minutes");

        // 4. Balance stays at -5 (debt from borrowing, usage doesn't change it)
        let balance = store.compute_balance("kid1").await.expect("balance");
        assert_eq!(balance, -5, "balance reflects loan debt of -5");
    }

    #[test]
    fn balance_transactions_recorded_for_lend_then_earn() {
        use models::BalanceTransaction;
        use schema::balance_transactions;

        let mut conn = setup_test_db();

        // Step 1: Lend 10 minutes (creates debt)
        let (_rem, bal) = do_reward(&mut conn, "child1", 10, true);
        assert_eq!(bal, -10);

        // Step 2: Earn 15 minutes (10 repays debt, 5 surplus to remaining)
        let (_rem, bal) = do_reward(&mut conn, "child1", 15, false);
        assert_eq!(bal, 0, "debt fully repaid");

        // Query balance_transactions and verify content
        let txns: Vec<BalanceTransaction> = balance_transactions::table
            .filter(balance_transactions::child_id.eq("child1"))
            .order(balance_transactions::id.asc())
            .load(&mut conn)
            .expect("query balance_transactions");

        assert_eq!(txns.len(), 2, "expected lend + auto-repayment transactions");

        // First transaction: lend (negative amount)
        assert_eq!(txns[0].child_id, "child1");
        assert_eq!(txns[0].amount, -10, "lend creates negative transaction");
        assert_eq!(
            txns[0].description.as_deref(),
            Some("Lent time"),
            "lend description"
        );
        assert!(
            txns[0].related_reward_id.is_some(),
            "should reference a reward"
        );

        // Second transaction: auto-repayment (positive amount)
        assert_eq!(txns[1].child_id, "child1");
        assert_eq!(txns[1].amount, 10, "auto-repayment of full debt");
        assert_eq!(
            txns[1].description.as_deref(),
            Some("Auto-repayment"),
            "repayment description"
        );
        assert!(
            txns[1].related_reward_id.is_some(),
            "should reference a reward"
        );
    }
}
