pub mod models;
pub mod schema;

use chrono::Utc;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use models::{
    Child, NewChild, NewPushSubscription, NewReward, NewSession, NewTask, PushSubscription,
    Session, Task,
};
use tracing::trace;

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

    pub async fn seed_from_config(
        &self,
        cfg_children: &[gamiscreen_shared::domain::Child],
        cfg_tasks: &[gamiscreen_shared::domain::Task],
    ) -> Result<(), StorageError> {
        use schema::{children, tasks};

        let pool = self.pool.clone();
        let children_owned = cfg_children.to_owned();
        let tasks_owned = cfg_tasks.to_owned();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;

            // Upsert children
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

                // No balances table anymore; remaining is computed dynamically
            }

            // Upsert tasks
            for t in &tasks_owned {
                let new_task = NewTask {
                    id: &t.id,
                    name: &t.name,
                    minutes: t.minutes,
                };
                diesel::insert_into(tasks::table)
                    .values(&new_task)
                    .on_conflict(tasks::id)
                    .do_update()
                    .set((
                        tasks::name.eq(new_task.name),
                        tasks::minutes.eq(new_task.minutes),
                    ))
                    .execute(&mut conn)?;
            }

            Ok(())
        })
        .await?
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
            Ok(tasks.order(name.asc()).load::<Task>(&mut conn)?)
        })
        .await?
    }

    pub async fn record_task_done(
        &self,
        child: &str,
        task: &str,
        by_username: &str,
    ) -> Result<(), StorageError> {
        use models::NewTaskCompletion;
        use schema::task_completions;
        let pool = self.pool.clone();
        let child = child.to_string();
        let task = task.to_string();
        let user = by_username.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let rec = NewTaskCompletion {
                child_id: &child,
                task_id: &task,
                by_username: &user,
            };
            diesel::insert_into(task_completions::table)
                .values(&rec)
                .execute(&mut conn)?;
            Ok(())
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
                        (
                            task_submissions::id,
                            task_submissions::child_id,
                            task_submissions::task_id,
                            task_submissions::submitted_at,
                        ),
                        (children::id, children::display_name),
                        (tasks::id, tasks::name, tasks::minutes),
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

    pub async fn approve_submission(
        &self,
        submission_id: i32,
        approver: &str,
    ) -> Result<Option<String>, StorageError> {
        let pool = self.pool.clone();
        let approver = approver.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>, StorageError> {
            use crate::storage::schema::{rewards, task_completions, task_submissions, tasks};
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let mut approved_child: Option<String> = None;
            conn.immediate_transaction(|conn| -> Result<(), StorageError> {
                // Fetch submission with task info
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
                    // Treat missing submission as success (idempotent)
                    return Ok(());
                };
                // remember child for cache invalidation by caller
                approved_child = Some(child_id.clone());
                // Insert reward with description = task name
                let new_reward = NewReward {
                    child_id: &child_id,
                    task_id: Some(&task_id),
                    minutes: mins,
                    description: Some(&task_name),
                };
                diesel::insert_into(rewards::table)
                    .values(&new_reward)
                    .execute(conn)?;
                // Record task completion
                let tc = models::NewTaskCompletion {
                    child_id: &child_id,
                    task_id: &task_id,
                    by_username: &approver,
                };
                diesel::insert_into(task_completions::table)
                    .values(&tc)
                    .execute(conn)?;
                // Delete submission
                diesel::delete(
                    task_submissions::table.filter(task_submissions::id.eq(submission_id)),
                )
                .execute(conn)?;
                Ok(())
            })?;
            Ok(approved_child)
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

    pub async fn add_reward_minutes(
        &self,
        child_id: &str,
        mins: i32,
        task: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), StorageError> {
        use schema::rewards;
        let pool = self.pool.clone();
        let child = child_id.to_string();
        let task_opt = task.map(|s| s.to_string());
        let description_opt = description.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            // Insert reward row only; remaining is computed dynamically
            let new_reward = NewReward {
                child_id: &child,
                task_id: task_opt.as_deref(),
                minutes: mins,
                description: description_opt.as_deref(),
            };
            diesel::insert_into(rewards::table)
                .values(&new_reward)
                .execute(&mut conn)?;
            Ok(())
        })
        .await?
    }

    pub async fn process_usage_minutes(
        &self,
        child: &str,
        device: &str,
        minutes: &[i64],
    ) -> Result<(), StorageError> {
        use schema::usage_minutes;

        use crate::storage::models::NewUsageMinute;
        if minutes.is_empty() {
            // this is an error condition
            return Err(StorageError::InvalidInput(
                "no minutes provided".to_string(),
            ));
        }
        let pool = self.pool.clone();
        let child_owned = child.to_string();
        let device_owned = device.to_string();
        let minutes_vec = minutes.to_vec();
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            for m in minutes_vec.into_iter() {
                let row = NewUsageMinute {
                    child_id: &child_owned,
                    minute_ts: m,
                    device_id: &device_owned,
                };
                // Use INSERT OR IGNORE equivalent
                let _ = diesel::insert_into(usage_minutes::table)
                    .values(&row)
                    .on_conflict_do_nothing()
                    .execute(&mut conn)?;
            }
            Ok(())
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

    pub async fn compute_remaining(&self, child: &str) -> Result<i32, StorageError> {
        use diesel::dsl::sum;
        let pool = self.pool.clone();
        let child_owned = child.to_string();
        tokio::task::spawn_blocking(move || -> Result<i32, StorageError> {
            let mut conn = pool.get()?;
            configure_sqlite_conn(&mut conn)?;
            let rewards_sum: Option<i64> = schema::rewards::dsl::rewards
                .filter(schema::rewards::dsl::child_id.eq(&child_owned))
                .select(sum(schema::rewards::dsl::minutes))
                .first::<Option<i64>>(&mut conn)?;
            let used: i64 = schema::usage_minutes::dsl::usage_minutes
                .filter(schema::usage_minutes::dsl::child_id.eq(&child_owned))
                .select(schema::usage_minutes::dsl::minute_ts)
                .distinct()
                .count()
                .get_result::<i64>(&mut conn)?;
            // Allow remaining time to go negative when usage exceeds rewards
            let remaining = (rewards_sum.unwrap_or(0) - used) as i32;
            Ok(remaining)
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

fn configure_sqlite_conn(conn: &mut SqliteConnection) -> Result<(), diesel::result::Error> {
    // Enable WAL for better read/write concurrency and set a busy timeout
    // Ignore the result rows; Diesel's execute is fine for PRAGMAs
    diesel::sql_query("PRAGMA journal_mode=WAL;").execute(conn)?;
    diesel::sql_query("PRAGMA synchronous=NORMAL;").execute(conn)?;
    diesel::sql_query("PRAGMA busy_timeout=5000;").execute(conn)?;
    Ok(())
}
