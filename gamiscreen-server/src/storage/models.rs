use crate::storage::schema::{children, rewards, task_completions, tasks, usage_minutes};
use chrono::NaiveDateTime;
use diesel::prelude::*;

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = children)]
pub struct Child {
    pub id: String,
    pub display_name: String,
}

#[derive(Insertable)]
#[diesel(table_name = children)]
pub struct NewChild<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
}

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = tasks)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub minutes: i32,
}

#[derive(Insertable)]
#[diesel(table_name = tasks)]
pub struct NewTask<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub minutes: i32,
}

#[derive(Debug, Clone, Queryable, Identifiable, Associations, Selectable)]
#[diesel(table_name = rewards)]
#[diesel(belongs_to(Child, foreign_key = child_id))]
#[diesel(belongs_to(Task, foreign_key = task_id))]
pub struct Reward {
    pub id: i32,
    pub child_id: String,
    pub task_id: Option<String>,
    pub minutes: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = rewards)]
pub struct NewReward<'a> {
    pub child_id: &'a str,
    pub task_id: Option<&'a str>,
    pub minutes: i32,
}

use crate::storage::schema::sessions;

#[derive(Debug, Clone, Queryable, Identifiable, Selectable)]
#[diesel(table_name = sessions)]
#[diesel(primary_key(jti))]
pub struct Session {
    pub jti: String,
    pub username: String,
    pub issued_at: NaiveDateTime,
    pub last_used_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession<'a> {
    pub jti: &'a str,
    pub username: &'a str,
}

#[derive(Debug, Clone, Queryable, Identifiable, Associations, Selectable)]
#[diesel(table_name = task_completions)]
#[diesel(belongs_to(Child, foreign_key = child_id))]
#[diesel(belongs_to(Task, foreign_key = task_id))]
pub struct TaskCompletion {
    pub id: i32,
    pub child_id: String,
    pub task_id: String,
    pub by_username: String,
    pub done_at: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = task_completions)]
pub struct NewTaskCompletion<'a> {
    pub child_id: &'a str,
    pub task_id: &'a str,
    pub by_username: &'a str,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = usage_minutes)]
pub struct NewUsageMinute<'a> {
    pub child_id: &'a str,
    pub minute_ts: i64,
    pub device_id: &'a str,
}
