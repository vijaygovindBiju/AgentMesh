use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    NewProposal, NewTaskApproval, Proposal, ProposalStatus, TaskApproval, ApprovalStatus,
};

pub struct ProposalRepository;

impl ProposalRepository {
    /// Inserts a new proposal with status 'pending'.
    pub async fn create(pool: &PgPool, new_proposal: &NewProposal) -> Result<Proposal> {
        let proposal = sqlx::query_as!(
            Proposal,
            r#"
            INSERT INTO proposals (project_id, ai_provider, ai_model, raw_prompt, raw_response, status)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, project_id, ai_provider, ai_model, raw_prompt, raw_response,
                      status AS "status: ProposalStatus", created_at
            "#,
            new_proposal.project_id,
            new_proposal.ai_provider,
            new_proposal.ai_model,
            new_proposal.raw_prompt,
            new_proposal.raw_response,
            ProposalStatus::Pending as ProposalStatus
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert proposal")?;

        Ok(proposal)
    }

    /// Finds a proposal by its primary key ID.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Proposal>> {
        let proposal = sqlx::query_as!(
            Proposal,
            r#"
            SELECT id, project_id, ai_provider, ai_model, raw_prompt, raw_response,
                   status AS "status: ProposalStatus", created_at
            FROM proposals
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query proposal by id")?;

        Ok(proposal)
    }

    /// Lists all proposals for a specific project.
    pub async fn list_by_project(pool: &PgPool, project_id: Uuid) -> Result<Vec<Proposal>> {
        let proposals = sqlx::query_as!(
            Proposal,
            r#"
            SELECT id, project_id, ai_provider, ai_model, raw_prompt, raw_response,
                   status AS "status: ProposalStatus", created_at
            FROM proposals
            WHERE project_id = $1
            ORDER BY created_at DESC
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list proposals by project")?;

        Ok(proposals)
    }

    /// Updates the status of a proposal.
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: ProposalStatus,
    ) -> Result<Option<Proposal>> {
        let proposal = sqlx::query_as!(
            Proposal,
            r#"
            UPDATE proposals
            SET status = $2
            WHERE id = $1
            RETURNING id, project_id, ai_provider, ai_model, raw_prompt, raw_response,
                      status AS "status: ProposalStatus", created_at
            "#,
            id,
            status as ProposalStatus
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update proposal status")?;

        Ok(proposal)
    }

    /// Records a human approval decision for a task (Human Approval Boundary).
    /// Upserts the approval if one already exists for (task_id, proposal_id).
    pub async fn record_approval(
        pool: &PgPool,
        approval: &NewTaskApproval,
    ) -> Result<TaskApproval> {
        let rec = sqlx::query_as!(
            TaskApproval,
            r#"
            INSERT INTO task_approvals (task_id, proposal_id, status, edited_desc, approved_by)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (task_id, proposal_id)
            DO UPDATE SET status = EXCLUDED.status,
                          edited_desc = EXCLUDED.edited_desc,
                          approved_by = EXCLUDED.approved_by,
                          approved_at = NOW()
            RETURNING task_id, proposal_id, status AS "status: ApprovalStatus",
                      edited_desc, approved_by, approved_at
            "#,
            approval.task_id,
            approval.proposal_id,
            approval.status as ApprovalStatus,
            approval.edited_desc,
            approval.approved_by
        )
        .fetch_one(pool)
        .await
        .context("Failed to record task approval")?;

        Ok(rec)
    }

    /// Finds an approval record by task and proposal ID.
    pub async fn find_approval(
        pool: &PgPool,
        task_id: Uuid,
        proposal_id: Uuid,
    ) -> Result<Option<TaskApproval>> {
        let approval = sqlx::query_as!(
            TaskApproval,
            r#"
            SELECT task_id, proposal_id, status AS "status: ApprovalStatus",
                   edited_desc, approved_by, approved_at
            FROM task_approvals
            WHERE task_id = $1 AND proposal_id = $2
            "#,
            task_id,
            proposal_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to find task approval")?;

        Ok(approval)
    }
}
