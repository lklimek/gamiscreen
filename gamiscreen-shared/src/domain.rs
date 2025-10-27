use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Minutes(pub i32);

impl Minutes {
    pub fn zero() -> Self {
        Minutes(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Child {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub minutes: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub child_id: ChildId,
    pub task_id: Option<TaskId>,
    pub minutes: Minutes,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTick {
    pub child_id: ChildId,
    pub device_id: String,
    pub occurred_at: OffsetDateTime,
}

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc().to_offset(UtcOffset::UTC)
}
