use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── ProposalStatus ───────────────────────────────────────────────────────────

/// Review progress of a coordinator-generated plan.
///
/// PostgreSQL enum type: `proposal_status`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "proposal_status", rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Awaiting human review. No tasks approved yet.
    Pending,
    /// At least one task has been reviewed but review is not complete.
    PartiallyApproved,
    /// All tasks in this proposal have been reviewed (approved or rejected).
    FullyApproved,
    /// Human aborted the entire proposal. All tasks remain `Proposed`.
    Rejected,
}

// ─── Proposal ─────────────────────────────────────────────────────────────────

/// A coordinator-generated task plan for a project, pending human review.
///
/// The raw prompt and response are stored verbatim for audit and debugging.
/// All tasks created from this proposal carry `proposal_id` as a foreign key.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Proposal {
    pub id: Uuid,
    pub project_id: Uuid,
    /// Name of the AI provider used, e.g. "anthropic".
    pub ai_provider: String,
    /// Model identifier, e.g. "claude-3-5-sonnet-20241022".
    pub ai_model: String,
    /// The exact prompt sent to the LLM (stored for auditability).
    pub raw_prompt: String,
    /// The exact response received from the LLM (stored for auditability).
    pub raw_response: String,
    pub status: ProposalStatus,
    pub created_at: DateTime<Utc>,
}

// ─── NewProposal ──────────────────────────────────────────────────────────────

/// Data required to create a new proposal record.
/// Id, status (defaults to `Pending`), and `created_at` are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProposal {
    pub project_id: Uuid,
    pub ai_provider: String,
    pub ai_model: String,
    pub raw_prompt: String,
    pub raw_response: String,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_status_serde_round_trip() {
        for status in [
            ProposalStatus::Pending,
            ProposalStatus::PartiallyApproved,
            ProposalStatus::FullyApproved,
            ProposalStatus::Rejected,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: ProposalStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }
}
