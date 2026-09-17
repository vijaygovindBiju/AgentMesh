use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Read-only context provided to the LLM for plan decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningRequest {
    pub project_id: Uuid,
    pub project_name: String,
    pub project_description: String,
    /// Registered agents available for work and their capability profiles.
    pub available_agents: Vec<AvailableAgentContext>,
    /// Existing tasks in the project (for incremental planning or dependency resolution).
    pub existing_tasks: Vec<ExistingTaskContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableAgentContext {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub adapter_type: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExistingTaskContext {
    pub task_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub status: String,
    pub affected_resources: Vec<String>,
}

/// The structured output produced by the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningResponse {
    /// High-level reasoning and architectural strategy behind the plan.
    pub reasoning: String,
    /// Proposed decomposition into discrete tasks. Must contain at least one task.
    pub proposed_tasks: Vec<ProposedTask>,
    /// Explicit dependencies between proposed and/or existing tasks.
    #[serde(default)]
    pub proposed_dependencies: Vec<ProposedDependency>,
}

/// A single task proposed by the LLM.
///
/// NOTE: Status is intentionally excluded from this schema. The LLM has no mechanism
/// to emit status transitions; the coordinator deterministically initializes all tasks
/// to `TaskStatus::Proposed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedTask {
    /// Short identifier unique within this plan (e.g. "TASK-1", "AUTH-01").
    pub short_id: String,
    /// Brief title describing the deliverable.
    pub title: String,
    /// Actionable description and acceptance criteria for the agent.
    pub description: String,
    /// Suggested agent ID from available_agents (advisory only; human can override).
    #[serde(default)]
    pub suggested_agent_id: Option<Uuid>,
    /// Explicit list of files, directories, or modules this task will modify.
    #[serde(default)]
    pub affected_resources: Vec<String>,
    /// Relative size estimate: "XS", "S", "M", "L", "XL".
    #[serde(default)]
    pub estimated_size: Option<String>,
}

/// An explicit dependency constraint proposed by the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedDependency {
    /// The short_id of the task that is blocked.
    pub dependent_short_id: String,
    /// The short_id of the prerequisite task that must finish first.
    pub depends_on_short_id: String,
    /// Whether this dependency is a strict blocker ("blocks") or advisory ("relates_to").
    #[serde(default = "default_dependency_kind")]
    pub kind: String,
    /// Rationale for the dependency constraint.
    #[serde(default)]
    pub reason: String,
}

fn default_dependency_kind() -> String {
    "blocks".to_string()
}
