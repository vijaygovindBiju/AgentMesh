use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Full specification of a task assigned to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub task_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub description: String,
    pub affected_resources: Vec<String>,
    pub depends_on: Vec<Uuid>,
    pub idempotency_key: String,
    pub assigned_at: DateTime<Utc>,
}
