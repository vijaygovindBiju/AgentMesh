use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── ApprovalStatus ───────────────────────────────────────────────────────────

/// The outcome of a human reviewing a single task in the plan review TUI.
///
/// PostgreSQL enum type: `approval_status`
///
/// A `TaskApproval` record with one of these statuses is the **only** mechanism
/// that can advance a task from `HumanReview` to `Approved` or `Rejected`.
/// See `docs/decisions/008-human-approval-boundary.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "approval_status", rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// Human pressed [Y] — task approved as proposed.
    Approved,
    /// Human pressed [N] — task will not be assigned.
    Rejected,
    /// Human pressed [E] and saved edits — task approved with human-modified description.
    EditedAndApproved,
}

impl ApprovalStatus {
    /// Returns `true` if this approval status allows the task to be assigned to an agent.
    pub fn allows_assignment(&self) -> bool {
        matches!(
            self,
            ApprovalStatus::Approved | ApprovalStatus::EditedAndApproved
        )
    }
}

// ─── TaskApproval ─────────────────────────────────────────────────────────────

/// Immutable record of a human's decision on a single task within a proposal.
///
/// This record is the **gate** between `HumanReview` and `Approved/Rejected`.
/// The coordinator must verify a `TaskApproval` exists with `allows_assignment() == true`
/// before creating a `TaskDelivery`.
///
/// Composite primary key: `(task_id, proposal_id)`.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskApproval {
    pub task_id: Uuid,
    pub proposal_id: Uuid,
    pub status: ApprovalStatus,
    /// The human's edited task description, if they pressed [E].
    /// `None` if the task was approved or rejected without editing.
    pub edited_desc: Option<String>,
    /// Display name of the human who made this decision, e.g. "Alice".
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
}

// ─── NewTaskApproval ──────────────────────────────────────────────────────────

/// Data recorded when a human approves, rejects, or edits a task in the TUI.
/// `approved_at` is DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTaskApproval {
    pub task_id: Uuid,
    pub proposal_id: Uuid,
    pub status: ApprovalStatus,
    pub edited_desc: Option<String>,
    pub approved_by: String,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_status_serde_round_trip() {
        for status in [
            ApprovalStatus::Approved,
            ApprovalStatus::Rejected,
            ApprovalStatus::EditedAndApproved,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: ApprovalStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }

    #[test]
    fn only_approved_statuses_allow_assignment() {
        assert!(ApprovalStatus::Approved.allows_assignment());
        assert!(ApprovalStatus::EditedAndApproved.allows_assignment());
        assert!(!ApprovalStatus::Rejected.allows_assignment());
    }
}
