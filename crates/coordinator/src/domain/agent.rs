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

// ─── HealthStatus ─────────────────────────────────────────────────────────────

/// The operational health status of an agent.
///
/// PostgreSQL enum type: `agent_health_status`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "agent_health_status", rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Offline,
}

impl From<agent_protocol::HealthStatus> for HealthStatus {
    fn from(hs: agent_protocol::HealthStatus) -> Self {
        match hs {
            agent_protocol::HealthStatus::Healthy => HealthStatus::Healthy,
            agent_protocol::HealthStatus::Degraded => HealthStatus::Degraded,
            agent_protocol::HealthStatus::Unhealthy => HealthStatus::Unhealthy,
            agent_protocol::HealthStatus::Offline => HealthStatus::Offline,
        }
    }
}

impl From<HealthStatus> for agent_protocol::HealthStatus {
    fn from(hs: HealthStatus) -> Self {
        match hs {
            HealthStatus::Healthy => agent_protocol::HealthStatus::Healthy,
            HealthStatus::Degraded => agent_protocol::HealthStatus::Degraded,
            HealthStatus::Unhealthy => agent_protocol::HealthStatus::Unhealthy,
            HealthStatus::Offline => agent_protocol::HealthStatus::Offline,
        }
    }
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
    /// Structured capability profile (runtime, toolchains, tools, tags).
    pub capability_profile: Option<Value>,
    /// Current health status evaluated by coordinator / heartbeats.
    pub health_status: HealthStatus,
    /// Count of consecutive task failures.
    pub consecutive_failures: i32,
    /// Total tasks completed successfully by this agent.
    pub tasks_completed_count: i32,
    /// Total tasks failed by this agent.
    pub tasks_failed_count: i32,
    /// Most recent error message reported by or about this agent.
    pub last_error: Option<String>,
    /// Round-trip or reported heartbeat latency in milliseconds.
    pub heartbeat_latency_ms: Option<i64>,
    /// Maximum concurrent tasks this agent is permitted to execute.
    pub max_concurrency: i32,
    /// Number of tasks currently executing on this agent.
    pub active_tasks_count: i32,
    /// When true, agent will complete running tasks but accept no new assignments.
    pub is_draining: bool,
}

impl Agent {
    /// Extracts `capabilities` as a `Vec<String>`.
    /// Merges profile tags if a structured capability profile is present.
    pub fn capabilities_list(&self) -> Vec<String> {
        if let Some(ref profile_val) = self.capability_profile {
            if let Ok(profile) = serde_json::from_value::<agent_protocol::AgentCapabilities>(profile_val.clone()) {
                return profile.all_tags();
            }
        }
        match &self.capabilities {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    }

    /// Returns the parsed structured capability profile if present.
    pub fn capabilities_profile(&self) -> Option<agent_protocol::AgentCapabilities> {
        self.capability_profile
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Returns `true` if this agent can accept a new task assignment.
    pub fn is_available(&self) -> bool {
        !self.is_draining
            && matches!(self.status, AgentStatus::Idle)
            && self.current_task_id.is_none()
            && self.active_tasks_count < self.max_concurrency
            && !matches!(self.health_status, HealthStatus::Unhealthy | HealthStatus::Offline)
    }

    /// Returns `true` if this agent is in an acceptable health condition.
    pub fn is_healthy(&self) -> bool {
        matches!(self.health_status, HealthStatus::Healthy | HealthStatus::Degraded)
    }

    /// Returns availability information snapshot.
    pub fn availability_info(&self) -> agent_protocol::AgentAvailability {
        agent_protocol::AgentAvailability {
            status: match self.status {
                AgentStatus::Offline => agent_protocol::AgentStatus::Offline,
                AgentStatus::Idle => agent_protocol::AgentStatus::Idle,
                AgentStatus::Busy => agent_protocol::AgentStatus::Busy,
                AgentStatus::Blocked => agent_protocol::AgentStatus::Blocked,
                AgentStatus::Error => agent_protocol::AgentStatus::Error,
            },
            is_available: self.is_available(),
            max_concurrency: self.max_concurrency as usize,
            active_tasks: self.active_tasks_count as usize,
            draining: self.is_draining,
        }
    }

    /// Creates a mock or sample Agent for testing / UI initialization.
    pub fn mock(
        id: Uuid,
        human_owner: &str,
        adapter_type: AdapterType,
        capabilities: Vec<String>,
        status: AgentStatus,
    ) -> Self {
        Self {
            id,
            human_owner: human_owner.to_string(),
            api_key_hash: "mock_key".to_string(),
            adapter_type,
            capabilities: serde_json::json!(capabilities),
            nats_subject: format!("agents.{id}.events"),
            status,
            current_task_id: None,
            last_seen: Some(Utc::now()),
            created_at: Utc::now(),
            capability_profile: None,
            health_status: HealthStatus::Healthy,
            consecutive_failures: 0,
            tasks_completed_count: 0,
            tasks_failed_count: 0,
            last_error: None,
            heartbeat_latency_ms: None,
            max_concurrency: 1,
            active_tasks_count: 0,
            is_draining: false,
        }
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
    #[serde(default)]
    pub profile: Option<agent_protocol::AgentCapabilities>,
    #[serde(default)]
    pub max_concurrency: Option<i32>,
}

impl NewAgent {
    pub fn new(
        human_owner: impl Into<String>,
        api_key_hash: impl Into<String>,
        adapter_type: AdapterType,
        capabilities: Vec<String>,
        nats_subject: impl Into<String>,
    ) -> Self {
        Self {
            human_owner: human_owner.into(),
            api_key_hash: api_key_hash.into(),
            adapter_type,
            capabilities,
            nats_subject: nats_subject.into(),
            profile: None,
            max_concurrency: None,
        }
    }

    pub fn with_profile(mut self, profile: agent_protocol::AgentCapabilities) -> Self {
        self.profile = Some(profile);
        self
    }

    pub fn with_max_concurrency(mut self, max: i32) -> Self {
        self.max_concurrency = Some(max);
        self
    }
}

impl Default for NewAgent {
    fn default() -> Self {
        Self {
            human_owner: String::new(),
            api_key_hash: String::new(),
            adapter_type: AdapterType::Mock,
            capabilities: Vec::new(),
            nats_subject: String::new(),
            profile: None,
            max_concurrency: None,
        }
    }
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
