use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};

/// Strongly-typed identifier for a child profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChildId(pub String);

impl fmt::Display for ChildId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for ChildId {
    fn from(value: &str) -> Self {
        ChildId(value.to_string())
    }
}

impl FromStr for ChildId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ChildId(s.to_string()))
    }
}

/// Strongly-typed identifier for a task definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for TaskId {
    fn from(value: &str) -> Self {
        TaskId(value.to_string())
    }
}

/// Screen-time duration in whole minutes. Can be negative when representing debt.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Minutes(pub i32);

impl Minutes {
    /// A zero-minute duration.
    pub fn zero() -> Self {
        Minutes(0)
    }
}

/// A child profile as stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Child {
    pub id: String,
    /// Human-readable name shown in the UI.
    pub display_name: String,
}

/// A task that a child can complete to earn screen time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// Short label shown to the child (e.g. "Brush teeth").
    pub name: String,
    /// Screen-time minutes awarded on completion.
    pub minutes: i32,
    /// When true, this task must be completed daily before screen time unlocks.
    #[serde(default)]
    pub required: bool,
}

/// A recorded reward: minutes granted to a child, optionally tied to a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub child_id: ChildId,
    /// The task that was completed, or `None` for ad-hoc grants.
    pub task_id: Option<TaskId>,
    /// Minutes awarded (always positive at creation time).
    pub minutes: Minutes,
    /// UTC timestamp when the reward was granted.
    pub created_at: OffsetDateTime,
}

/// A single minute of observed screen usage on a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTick {
    pub child_id: ChildId,
    /// Device that reported this tick (machine-id or similar).
    pub device_id: String,
    /// UTC timestamp of the observed active minute.
    pub occurred_at: OffsetDateTime,
}

/// Returns the current UTC time.
pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(UtcOffset::UTC)
}
