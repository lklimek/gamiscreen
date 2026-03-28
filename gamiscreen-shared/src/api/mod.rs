use serde::{Deserialize, Serialize};

/// URL prefix for all versioned API endpoints.
pub const API_V1_PREFIX: &str = "/api/v1";
/// URL prefix for family-scoped (tenant-scoped) endpoints.
pub const FAMILY_SCOPE_PREFIX: &str = "/api/v1/family";

/// Build the URL prefix for a specific family/tenant scope.
pub fn tenant_scope(tenant_id: &str) -> String {
    format!("{}/{}", FAMILY_SCOPE_PREFIX, tenant_id)
}

pub mod endpoints;
#[cfg(feature = "rest-client")]
pub mod rest;
#[cfg(feature = "ts")]
pub mod ts_export;

/// Credentials submitted by a parent to obtain a session token.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuthReq {
    pub username: String,
    pub password: String,
}

/// Session token returned after successful parent authentication.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuthResp {
    /// JWT bearer token for subsequent authenticated requests.
    pub token: String,
}

/// Summary of a child profile, used in list responses.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChildDto {
    pub id: String,
    /// Human-readable name shown in the UI.
    pub display_name: String,
}

/// A task definition that can earn screen time when completed.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskDto {
    pub id: String,
    /// Short label shown to the child (e.g. "Brush teeth").
    pub name: String,
    /// Screen-time minutes awarded when this task is completed.
    pub minutes: i32,
    /// When true, the child must complete this task daily before any screen time unlocks.
    pub required: bool,
    /// Priority level: 1 (high), 2 (normal), 3 (low).
    pub priority: i32,
    /// 7-bit bitmask of mandatory days (bit 0 = Mon .. bit 6 = Sun). 0 = optional task.
    pub mandatory_days: i32,
    /// Start time for mandatory tasks in "HH:MM" format (family timezone). `None` for optional tasks.
    pub mandatory_start_time: Option<String>,
}

/// A task enriched with the child's most recent completion timestamp.
///
/// Returned by the per-child tasks endpoint so the UI can show completion state.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskWithStatusDto {
    pub id: String,
    pub name: String,
    /// Screen-time minutes awarded on completion.
    pub minutes: i32,
    /// When true, this task blocks screen time until completed today.
    pub required: bool,
    /// RFC 3339 UTC timestamp of the most recent completion, or `None` if never done.
    pub last_done: Option<String>,
    /// Priority level: 1 (high), 2 (normal), 3 (low).
    pub priority: i32,
    /// 7-bit bitmask of mandatory days (bit 0 = Mon .. bit 6 = Sun). 0 = optional task.
    pub mandatory_days: i32,
    /// Start time for mandatory tasks in "HH:MM" format (family timezone). `None` for optional tasks.
    pub mandatory_start_time: Option<String>,
    /// True when this mandatory task is currently due and not yet completed today.
    pub is_currently_blocking: bool,
}

/// Request body for creating a new task (parent-only).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CreateTaskReq {
    /// Task name (1-100 chars, non-empty after trim).
    pub name: String,
    /// Screen-time minutes awarded on completion (non-zero).
    pub minutes: i32,
    /// Priority level: 1 (high), 2 (normal), 3 (low). Defaults to 2.
    pub priority: Option<i32>,
    /// 7-bit bitmask of mandatory days (bit 0 = Mon .. bit 6 = Sun). Defaults to 0 (optional).
    pub mandatory_days: Option<i32>,
    /// Start time in "HH:MM" format (family timezone). Required when mandatory_days > 0.
    pub mandatory_start_time: Option<String>,
    /// Child IDs this task is assigned to. `None` = all children.
    pub assigned_children: Option<Vec<String>>,
}

/// Request body for updating an existing task (full replacement, parent-only).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UpdateTaskReq {
    /// Task name (1-100 chars, non-empty after trim).
    pub name: String,
    /// Screen-time minutes awarded on completion (non-zero).
    pub minutes: i32,
    /// Priority level: 1 (high), 2 (normal), 3 (low). Defaults to 2.
    pub priority: Option<i32>,
    /// 7-bit bitmask of mandatory days (bit 0 = Mon .. bit 6 = Sun). Defaults to 0 (optional).
    pub mandatory_days: Option<i32>,
    /// Start time in "HH:MM" format (family timezone). Required when mandatory_days > 0.
    pub mandatory_start_time: Option<String>,
    /// Child IDs this task is assigned to. `None` = all children.
    pub assigned_children: Option<Vec<String>>,
}

/// Parent-facing task with full management details (response for GET/POST/PUT).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskManagementDto {
    pub id: String,
    pub name: String,
    /// Screen-time minutes awarded on completion.
    pub minutes: i32,
    /// Priority level: 1 (high), 2 (normal), 3 (low).
    pub priority: i32,
    /// 7-bit bitmask of mandatory days (bit 0 = Mon .. bit 6 = Sun). 0 = optional task.
    pub mandatory_days: i32,
    /// Start time for mandatory tasks in "HH:MM" format (family timezone).
    pub mandatory_start_time: Option<String>,
    /// Child IDs this task is assigned to. `None` = all children.
    pub assigned_children: Option<Vec<String>>,
    /// RFC 3339 timestamp when the task was created.
    pub created_at: String,
    /// RFC 3339 timestamp when the task was last updated.
    pub updated_at: String,
}

/// Response for task deletion.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DeleteTaskResp {
    /// True when the task was successfully soft-deleted.
    pub deleted: bool,
}

/// Current screen-time state for a child.
///
/// Returned by the remaining endpoint. The UI uses this to display
/// how many minutes the child can use and whether access is blocked.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RemainingDto {
    pub child_id: String,
    /// Actual usable screen-time minutes (stored in DB, updated transactionally).
    /// May include borrowed time. When `blocked_by_tasks` is true, effective remaining is 0.
    pub remaining_minutes: i32,
    /// Virtual bank account balance. Zero when no debt exists.
    /// Negative when borrowing creates debt. Earned minutes repay debt before
    /// adding to remaining. Penalties and usage do not affect this value.
    pub balance: i32,
    /// True when required daily tasks have not been completed.
    /// While blocked, effective remaining is 0 even if `remaining_minutes` > 0.
    pub blocked_by_tasks: bool,
}

/// Request to grant screen-time minutes to a child. Called by a parent.
///
/// Either `task_id` (task completion) or `minutes` + `description` (ad-hoc grant)
/// must be provided. When `is_borrowed` is true, the minutes are added to remaining
/// but create a negative balance (debt) that must be repaid through future earnings.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RewardReq {
    pub child_id: String,
    /// If set, reward is for completing this task (minutes taken from task definition).
    pub task_id: Option<String>,
    /// Ad-hoc minutes to grant (used when `task_id` is absent).
    pub minutes: Option<i32>,
    /// Free-text reason for the ad-hoc grant.
    pub description: Option<String>,
    /// When true, granted minutes add to remaining but create debt (negative balance).
    /// Subsequent earned minutes repay the debt before increasing remaining.
    #[serde(default)]
    pub is_borrowed: Option<bool>,
}

/// Updated screen-time totals returned after a reward is granted.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RewardResp {
    /// New remaining minutes after the reward.
    pub remaining_minutes: i32,
    /// New account balance after the reward (negative = debt from borrowing).
    pub balance: i32,
}

/// Batch of active-use timestamps sent by a device client.
///
/// The device sends one epoch-minute value for each minute the screen was active.
/// The server deduplicates and decrements `remaining_minutes` for each new minute.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HeartbeatReq {
    /// UTC epoch-minute timestamps (seconds since epoch / 60).
    #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
    pub minutes: Vec<i64>,
}

/// Updated screen-time state returned after processing a heartbeat.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HeartbeatResp {
    /// Remaining minutes after deducting newly reported usage.
    pub remaining_minutes: i32,
    /// Current account balance (negative = debt from borrowing). Unaffected by usage.
    pub balance: i32,
    /// Whether required tasks still block screen time.
    pub blocked_by_tasks: bool,
}

/// Web Push subscription request. Called by a child's browser to receive notifications.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PushSubscribeReq {
    /// Push service endpoint URL provided by the browser.
    pub endpoint: String,
    /// P-256 ECDH public key for payload encryption (base64url).
    pub p256dh: String,
    /// Authentication secret for payload encryption (base64url).
    pub auth: String,
}

/// Confirmation of a new push subscription.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PushSubscribeResp {
    /// Server-assigned ID for managing this subscription.
    pub subscription_id: i32,
}

/// Request to remove a push subscription by its endpoint URL.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PushUnsubscribeReq {
    /// The push service endpoint URL to unsubscribe.
    pub endpoint: String,
}

/// Tenant-level configuration exposed to clients.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConfigResp {
    /// VAPID public key for Web Push. `None` if push notifications are not configured.
    pub push_public_key: Option<String>,
}

/// Device client registration request. Sent by the Linux/Windows agent on first launch.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientRegisterReq {
    /// Child to associate with this device. If `None`, the server may auto-assign.
    pub child_id: Option<String>,
    /// Unique device identifier (e.g. machine-id).
    pub device_id: String,
}

/// Credentials returned after successful device registration.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ClientRegisterResp {
    /// JWT bearer token the device uses for heartbeat and remaining calls.
    pub token: String,
    /// The child this device is now bound to.
    pub child_id: String,
    pub device_id: String,
}

/// A single reward event in the child's history.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RewardHistoryItemDto {
    /// RFC 3339 UTC timestamp when the reward was granted.
    pub time: String,
    /// Free-text reason, or `None` for task-based rewards.
    pub description: Option<String>,
    /// Minutes granted (always positive).
    pub minutes: i32,
    /// True if these minutes were borrowed, creating debt.
    pub is_borrowed: bool,
}

/// A single bucket in a usage time series (e.g. one hour of a daily chart).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UsageBucketDto {
    /// RFC 3339 UTC timestamp for the start of this bucket.
    pub start: String,
    /// Total active-use minutes within this bucket.
    pub minutes: u32,
}

/// Aggregated usage over a time range, split into fixed-size buckets.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UsageSeriesDto {
    /// RFC 3339 UTC start of the requested range (inclusive).
    pub start: String,
    /// RFC 3339 UTC end of the requested range (exclusive).
    pub end: String,
    /// Duration of each bucket in minutes.
    pub bucket_minutes: u32,
    /// Ordered list of buckets covering the range.
    pub buckets: Vec<UsageBucketDto>,
    /// Sum of all bucket minutes (convenience total).
    pub total_minutes: u32,
}

/// Request from a child to submit a completed task for parent approval.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SubmitTaskReq {
    pub child_id: String,
    pub task_id: String,
}

/// Count of pending notifications (e.g. task submissions awaiting approval).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotificationsCountDto {
    pub count: u32,
}

/// A pending task-submission notification shown to the parent.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotificationItemDto {
    /// Server-assigned notification ID.
    pub id: i32,
    /// Notification type discriminator (currently always `"task_submission"`).
    pub kind: String,
    pub child_id: String,
    pub child_display_name: String,
    pub task_id: String,
    pub task_name: String,
    /// RFC 3339 UTC timestamp when the child submitted the task.
    pub submitted_at: String,
}

/// OTA update manifest listing available client packages and versions.
///
/// Schema version 2: contains multiple items, each for a specific package
/// and semantic version. Clients select the newest compatible version.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UpdateManifestDto {
    /// Manifest schema version (currently 2).
    pub schema_version: u32,
    /// RFC 3339 UTC timestamp when this manifest was generated.
    pub generated_at: String,
    /// Available update packages.
    pub items: Vec<UpdateItemDto>,
}

/// A single updatable package with its version and platform artifacts.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UpdateItemDto {
    /// Package name (e.g. `"gamiscreen-client"`).
    pub package: String,
    /// Semantic version string (e.g. `"1.2.3"`).
    pub version: String,
    /// Platform-specific downloadable artifacts.
    pub artifacts: Vec<UpdateArtifactDto>,
}

/// A downloadable binary for a specific OS/arch combination.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UpdateArtifactDto {
    /// Target operating system (e.g. `"linux"`, `"windows"`).
    pub os: String,
    /// Target CPU architecture (e.g. `"x86_64"`, `"aarch64"`).
    pub arch: String,
    /// Download URL for the artifact.
    pub url: String,
    /// SHA-256 hex digest for integrity verification.
    pub sha256: String,
}

/// Server version information.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct VersionInfoDto {
    /// Semantic version string (e.g. `"1.2.3"`).
    pub version: String,
}

/// Real-time events pushed to connected clients over WebSocket.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ServerEvent {
    /// Notifies the parent that the count of pending task submissions changed.
    #[serde(rename = "pending_count")]
    PendingCount { count: u32 },
    /// Notifies that a child's screen-time state changed (reward granted, usage reported, etc.).
    #[serde(rename = "remaining_updated")]
    RemainingUpdated {
        child_id: String,
        remaining_minutes: i32,
        balance: i32,
        blocked_by_tasks: bool,
    },
}
