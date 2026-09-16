use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ─── TaskStatus ───────────────────────────────────────────────────────────────

/// Task lifecycle status.
///
/// The state machine is defined in `docs/domain-model.md`.
/// Only `Proposed` can be created by LLM output.
/// Only human action in the TUI advances from `HumanReview`.
/// The coordinator's deterministic logic handles `Approved → Assigned`.
///
/// PostgreSQL enum type: `task_status`
/// Values: "proposed", "human_review", "approved", "rejected",
///         "assigned", "executing", "blocked", "completed", "failed", "cancelled"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "task_status", rename_all = "snake_case")]
pub enum TaskStatus {
    /// LLM has generated this task. Only entry point for AI output.
    Proposed,
    /// Task is queued for human review in the TUI approval screen.
    HumanReview,
    /// Human has approved (or edited+approved) this task.
    Approved,
    /// Human has rejected this task. Terminal state.
    Rejected,
    /// Coordinator has assigned task to an agent; `TaskDelivery` record created.
    Assigned,
    /// Agent has acknowledged the task and is executing.
    Executing,
    /// Agent reported a blocker; waiting for a dependency to complete.
    Blocked,
    /// Agent reported successful completion. Terminal state.
    Completed,
    /// Task failed (agent error or max redelivery exceeded). Terminal state.
    /// Coordinator may propose reassignment with human approval.
    Failed,
    /// Task was explicitly cancelled. Terminal state.
    Cancelled,
}

impl TaskStatus {
    /// Returns `true` if transitioning from `self` to `next` is permitted
    /// by the domain state machine in `docs/domain-model.md`.
    ///
    /// This is the canonical definition of allowed transitions.
    /// Coordinator logic MUST call this before persisting any status change.
    pub fn can_transition_to(&self, next: &TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, next),
            // Planning → Review → Decision
            (Proposed, HumanReview)
                | (HumanReview, Approved)
                | (HumanReview, Rejected)
                // Execution path
                | (Approved, Assigned)
                | (Assigned, Executing)
                | (Assigned, Failed)
                | (Executing, Blocked)
                | (Executing, Completed)
                | (Executing, Failed)
                | (Blocked, Executing)
                // Reassignment path (human approves reassignment of failed task)
                | (Failed, Approved)
                // Cancellation (allowed from non-terminal active states)
                | (Approved, Cancelled)
                | (Assigned, Cancelled)
                | (Executing, Cancelled)
                | (Blocked, Cancelled)
        )
    }

    /// Returns `true` if this status admits no further transitions.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Rejected | TaskStatus::Completed | TaskStatus::Cancelled
        )
    }

    /// Returns `true` if this task requires human input to advance.
    pub fn requires_human_action(&self) -> bool {
        matches!(self, TaskStatus::HumanReview)
    }
}

// ─── DependencyKind ───────────────────────────────────────────────────────────

/// How one task relates to another.
///
/// PostgreSQL enum type: `dependency_kind`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "dependency_kind", rename_all = "snake_case")]
pub enum DependencyKind {
    /// The dependent task cannot start until `depends_on` is `Completed`.
    Blocks,
    /// Informational relationship; does not gate execution.
    RelatesTo,
}

// ─── Task ─────────────────────────────────────────────────────────────────────

/// A unit of work within a project, proposed by the coordinator and
/// approved by a human before being assigned to an agent.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    /// Human-readable short identifier, e.g. "TASK-003". Unique within a project.
    pub short_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub assigned_agent_id: Option<Uuid>,
    /// JSONB array of file paths, module names, or API names this task touches.
    /// Use `Task::resources()` to get a typed `Vec<String>`.
    pub affected_resources: Value,
    /// Rough size estimate: "S", "M", or "L". Optional.
    pub estimated_size: Option<String>,
    pub proposal_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Extracts `affected_resources` as a `Vec<String>`.
    /// Silently skips non-string array elements.
    pub fn resources(&self) -> Vec<String> {
        match &self.affected_resources {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    }
}

// ─── TaskDependency ───────────────────────────────────────────────────────────

/// An explicit dependency edge between two tasks.
///
/// `dependent_id` must wait for `depends_on_id` to reach `Completed`
/// before the coordinator advances it past `Approved`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskDependency {
    pub dependent_id: Uuid,
    pub depends_on_id: Uuid,
    pub kind: DependencyKind,
}

// ─── Creation types ───────────────────────────────────────────────────────────

/// Data required to create a new task.
/// Id, status (defaults to `Proposed`), and timestamps are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTask {
    pub project_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub description: String,
    pub affected_resources: Vec<String>,
    pub estimated_size: Option<String>,
    pub proposal_id: Uuid,
}

/// Data required to create a dependency edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTaskDependency {
    pub dependent_id: Uuid,
    pub depends_on_id: Uuid,
    pub kind: DependencyKind,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn all_statuses() -> Vec<TaskStatus> {
        vec![
            TaskStatus::Proposed,
            TaskStatus::HumanReview,
            TaskStatus::Approved,
            TaskStatus::Rejected,
            TaskStatus::Assigned,
            TaskStatus::Executing,
            TaskStatus::Blocked,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ]
    }

    #[test]
    fn task_status_serde_round_trip() {
        for status in all_statuses() {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }

    #[test]
    fn valid_transitions_are_permitted() {
        let valid = vec![
            (TaskStatus::Proposed, TaskStatus::HumanReview),
            (TaskStatus::HumanReview, TaskStatus::Approved),
            (TaskStatus::HumanReview, TaskStatus::Rejected),
            (TaskStatus::Approved, TaskStatus::Assigned),
            (TaskStatus::Assigned, TaskStatus::Executing),
            (TaskStatus::Assigned, TaskStatus::Failed),
            (TaskStatus::Executing, TaskStatus::Blocked),
            (TaskStatus::Executing, TaskStatus::Completed),
            (TaskStatus::Executing, TaskStatus::Failed),
            (TaskStatus::Failed, TaskStatus::Approved),
            (TaskStatus::Blocked, TaskStatus::Executing),
            (TaskStatus::Approved, TaskStatus::Cancelled),
            (TaskStatus::Executing, TaskStatus::Cancelled),
        ];
        for (from, to) in valid {
            assert!(
                from.can_transition_to(&to),
                "{from:?} → {to:?} should be allowed"
            );
        }
    }

    #[test]
    fn llm_cannot_bypass_human_review() {
        // LLM produces Proposed; it must go through HumanReview before Approved
        assert!(!TaskStatus::Proposed.can_transition_to(&TaskStatus::Approved));
        assert!(!TaskStatus::Proposed.can_transition_to(&TaskStatus::Assigned));
        assert!(!TaskStatus::Proposed.can_transition_to(&TaskStatus::Executing));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let invalid = vec![
            // Terminal states have no exits
            (TaskStatus::Completed, TaskStatus::Executing),
            (TaskStatus::Rejected, TaskStatus::HumanReview),
            (TaskStatus::Cancelled, TaskStatus::Approved),
            // Cannot skip states
            (TaskStatus::Approved, TaskStatus::Executing),
            (TaskStatus::HumanReview, TaskStatus::Executing),
            // Cannot go backwards
            (TaskStatus::Executing, TaskStatus::Assigned),
            (TaskStatus::Assigned, TaskStatus::Approved),
        ];
        for (from, to) in invalid {
            assert!(
                !from.can_transition_to(&to),
                "{from:?} → {to:?} should NOT be allowed"
            );
        }
    }

    #[test]
    fn terminal_states_are_identified() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Rejected.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());

        assert!(!TaskStatus::Proposed.is_terminal());
        assert!(!TaskStatus::HumanReview.is_terminal());
        assert!(!TaskStatus::Approved.is_terminal());
        assert!(!TaskStatus::Assigned.is_terminal());
        assert!(!TaskStatus::Executing.is_terminal());
        assert!(!TaskStatus::Blocked.is_terminal());
        assert!(!TaskStatus::Failed.is_terminal()); // Failed is reassignable
    }

    #[test]
    fn human_review_requires_human_action() {
        assert!(TaskStatus::HumanReview.requires_human_action());
        assert!(!TaskStatus::Proposed.requires_human_action());
        assert!(!TaskStatus::Approved.requires_human_action());
    }

    #[test]
    fn task_resources_extracts_string_array() {
        use serde_json::json;
        let task_resources = json!(["src/auth.rs", "src/middleware/"]);
        assert_eq!(
            {
                match &task_resources {
                    Value::Array(arr) => arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>(),
                    _ => vec![],
                }
            },
            vec!["src/auth.rs", "src/middleware/"]
        );
    }

    #[test]
    fn dependency_kind_serde_round_trip() {
        for kind in [DependencyKind::Blocks, DependencyKind::RelatesTo] {
            let json = serde_json::to_string(&kind).unwrap();
            let recovered: DependencyKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, recovered);
        }
    }
}
