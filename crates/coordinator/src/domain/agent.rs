use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ─── AgentStatus ──────────────────────────────────────────────────────────────

/// The operational status of a registered agent.
///
/// PostgreSQL enum type: `agent_status`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "agent_status", rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent process is not connected to NATS.
    Offline,
    /// Agent is connected and ready to receive a task.
    Idle,
    /// Agent is executing a task.
    Busy,
    /// Agent is blocked waiting for a dependency.
    Blocked,
    /// Agent encountered an unrecoverable error.
    Error,
}

// ─── AdapterType ──────────────────────────────────────────────────────────────

/// Which runtime backs this agent.
///
/// PostgreSQL enum type: `adapter_type`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "adapter_type", rename_all = "snake_case")]
pub enum AdapterType {
    /// `agent-mock` binary — real protocol, simulated work.
    Mock,
    /// `agent-agy` binary — Antigravity CLI subprocess adapter.
    Agy,
}

// ─── Agent ────────────────────────────────────────────────────────────────────

/// A registered AI coding agent controlled by a human team member.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Agent {
    pub id: Uuid,
    /// Display name of the human who owns this agent, e.g. "Alice".
    pub human_owner: String,
    /// bcrypt hash of the agent's API key. The raw key is shown once at registration.
    pub api_key_hash: String,
    pub adapter_type: AdapterType,
    /// JSONB array of capability strings, e.g. ["rust", "python"].
    /// Use `Agent::capabilities_list()` to get a typed `Vec<String>`.
    pub capabilities: Value,
    /// NATS subject this agent publishes events to, e.g. "agents.abc123.events".
    pub nats_subject: String,
    pub status: AgentStatus,
    /// The task this agent is currently executing, if any.
    pub current_task_id: Option<Uuid>,
    /// Timestamp of the most recent heartbeat received from this agent.
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Agent {
    /// Extracts `capabilities` as a `Vec<String>`.
    /// Silently skips non-string elements.
    pub fn capabilities_list(&self) -> Vec<String> {
        match &self.capabilities {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    }

    /// Returns `true` if this agent can accept a new task assignment.
    pub fn is_available(&self) -> bool {
        matches!(self.status, AgentStatus::Idle) && self.current_task_id.is_none()
    }
}

// ─── NewAgent ─────────────────────────────────────────────────────────────────

/// Data required to register a new agent.
/// Id, status (defaults to `Offline`), and `created_at` are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAgent {
    pub human_owner: String,
    /// Pre-hashed API key. The caller must hash before passing here.
    pub api_key_hash: String,
    pub adapter_type: AdapterType,
    pub capabilities: Vec<String>,
    pub nats_subject: String,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_serde_round_trip() {
        for status in [
            AgentStatus::Offline,
            AgentStatus::Idle,
            AgentStatus::Busy,
            AgentStatus::Blocked,
            AgentStatus::Error,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: AgentStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }

    #[test]
    fn adapter_type_serde_round_trip() {
        for t in [AdapterType::Mock, AdapterType::Agy] {
            let json = serde_json::to_string(&t).unwrap();
            let recovered: AdapterType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, recovered);
        }
    }
}
