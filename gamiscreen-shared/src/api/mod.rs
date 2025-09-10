use serde::{Deserialize, Serialize};

pub mod endpoints;
#[cfg(feature = "rest-client")]
pub mod rest;

// Auth
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResp {
    pub token: String,
}

// Children/Tasks
#[derive(Debug, Serialize, Deserialize)]
pub struct ChildDto {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskDto {
    pub id: String,
    pub name: String,
    pub minutes: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskWithStatusDto {
    pub id: String,
    pub name: String,
    pub minutes: i32,
    pub last_done: Option<String>, // RFC3339 UTC
}

// Remaining
#[derive(Debug, Serialize, Deserialize)]
pub struct RemainingDto {
    pub child_id: String,
    pub remaining_minutes: i32,
}

// Reward
#[derive(Debug, Serialize, Deserialize)]
pub struct RewardReq {
    pub child_id: String,
    pub task_id: Option<String>,
    pub minutes: Option<i32>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RewardResp {
    pub remaining_minutes: i32,
}

// Heartbeat: batch of minute timestamps (UTC epoch minutes)
#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatReq {
    pub minutes: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatResp {
    pub remaining_minutes: i32,
}

// Client registration
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientRegisterReq {
    pub child_id: Option<String>,
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientRegisterResp {
    pub token: String,
    pub child_id: String,
    pub device_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RewardHistoryItemDto {
    pub time: String, // RFC3339 UTC
    pub description: Option<String>,
    pub minutes: i32,
}

// Submissions / Notifications
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTaskReq {
    pub child_id: String,
    pub task_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationsCountDto {
    pub count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationItemDto {
    pub id: i32,
    pub kind: String,
    pub child_id: String,
    pub child_display_name: String,
    pub task_id: String,
    pub task_name: String,
    pub submitted_at: String, // RFC3339 UTC
}

// Update manifest (public)
// schema_version 2: manifest contains multiple items, each for a
// specific package and semantic version. Clients should select the
// newest compatible version by comparing semvers.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateManifestDto {
    pub schema_version: u32,
    pub generated_at: String,
    pub items: Vec<UpdateItemDto>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateItemDto {
    pub package: String, // e.g. "gamiscreen-client"
    pub version: String, // semantic version, e.g. "1.2.3"
    pub artifacts: Vec<UpdateArtifactDto>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateArtifactDto {
    pub os: String,
    pub arch: String,
    pub url: String,
    pub sha256: String,
}

// Server version
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VersionInfoDto {
    pub version: String, // semantic version (e.g. "1.2.3")
}

// Server-sent events over WebSocket
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "pending_count")]
    PendingCount { count: u32 },
    #[serde(rename = "remaining_updated")]
    RemainingUpdated { child_id: String, remaining_minutes: i32 },
}
