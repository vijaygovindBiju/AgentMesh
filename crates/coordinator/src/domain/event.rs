use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ─── AgentEventType ───────────────────────────────────────────────────────────

/// The kind of lifecycle event an agent reported.
///
/// PostgreSQL enum type: `event_type`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "event_type", rename_all = "snake_case")]
pub enum AgentEventType {
    /// Agent acknowledged the task and began work.
    TaskStarted,
    /// Intermediate progress report (percentage and/or message).
    ProgressUpdate,
    /// Agent cannot proceed; waiting for an external condition or dependency.
    Blocked,
    /// Agent successfully completed the task.
    Completed,
    /// Agent encountered an error and could not complete the task.
    Failed,
}

// ─── AgentEvent ───────────────────────────────────────────────────────────────

/// An immutable record of a lifecycle event received from an agent via NATS.
///
/// Events are append-only. The coordinator reads these to drive task state
/// transitions. `payload` stores the full original NATS message for debugging.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentEvent {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub task_id: Uuid,
    pub event_type: AgentEventType,
    /// Human-readable message from the agent (progress description, error message, etc.).
    pub message: Option<String>,
    /// Full structured payload from the agent's NATS message.
    pub payload: Value,
    pub received_at: DateTime<Utc>,
}

// ─── NewAgentEvent ────────────────────────────────────────────────────────────

/// Data required to record an incoming agent event.
/// Id and `received_at` are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAgentEvent {
    pub agent_id: Uuid,
    pub task_id: Uuid,
    pub event_type: AgentEventType,
    pub message: Option<String>,
    pub payload: Value,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_event_type_serde_round_trip() {
        for event_type in [
            AgentEventType::TaskStarted,
            AgentEventType::ProgressUpdate,
            AgentEventType::Blocked,
            AgentEventType::Completed,
            AgentEventType::Failed,
        ] {
            let json = serde_json::to_string(&event_type).unwrap();
            let recovered: AgentEventType = serde_json::from_str(&json).unwrap();
            assert_eq!(event_type, recovered);
        }
    }
}
