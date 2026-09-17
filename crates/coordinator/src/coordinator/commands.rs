use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProposalRepository, TaskRepository,
};
use crate::domain::{
    AgentStatus, ApprovalStatus, NewTaskApproval, Task, TaskStatus,
};

/// Commands sent to the coordinator core (e.g. from TUI or API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorCommand {
    SubmitProject {
        name: String,
        description: String,
    },
    ApproveTask {
        task_id: Uuid,
        approved_by: String,
    },
    EditAndApproveTask {
        task_id: Uuid,
        new_description: String,
        approved_by: String,
    },
    RejectTask {
        task_id: Uuid,
        rejected_by: String,
    },
    AcknowledgeOverlap {
        warning_id: Uuid,
        acknowledged_by: String,
    },
    ReassignTask {
        task_id: Uuid,
        new_agent_id: Uuid,
        approved_by: String,
    },
    CancelTask {
        task_id: Uuid,
        reason: String,
    },
    RequestRefresh,
}

/// Events emitted by the coordinator core to update observers (e.g. TUI, metrics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorEvent {
    StateChanged(crate::coordinator::state::CoordinatorState),
    ProjectSubmitted { project_id: Uuid },
    TaskApproved { task_id: Uuid },
    TaskApprovalBlocked { task_id: Uuid, reason: String },
    TaskRejected { task_id: Uuid },
    TaskAssigned { task_id: Uuid, agent_id: Uuid },
    TaskStatusChanged { task_id: Uuid, old_status: TaskStatus, new_status: TaskStatus },
    OverlapAcknowledged { warning_id: Uuid },
    CommandFailed { message: String },
}

/// Errors specific to human approval gating.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalGateError {
    #[error("Task {task_id} cannot be approved because of unacknowledged critical overlap(s) on resource(s): {resources:?}")]
    BlockedByCriticalOverlap {
        task_id: Uuid,
        resources: Vec<String>,
    },
    #[error("Task {task_id} has status {current:?}, expected HumanReview or Failed")]
    InvalidTaskStatus {
        task_id: Uuid,
        current: TaskStatus,
    },
    #[error("Task {0} not found")]
    TaskNotFound(Uuid),
    #[error("Database error: {0}")]
    Database(#[from] anyhow::Error),
}

pub struct CommandHandler;

impl CommandHandler {
    /// Executes the ApproveTask command with strict approval gating and critical overlap enforcement.
    pub async fn execute_approve_task(
        pool: &PgPool,
        task_id: Uuid,
        approved_by: &str,
    ) -> Result<Task, ApprovalGateError> {
        // 1. Check for unacknowledged critical overlaps
        let critical_overlaps = OverlapWarningRepository::list_unacknowledged_critical_for_task(pool, task_id)
            .await
            .map_err(ApprovalGateError::Database)?;

        if !critical_overlaps.is_empty() {
            let resources = critical_overlaps.into_iter().map(|w| w.resource).collect();
            return Err(ApprovalGateError::BlockedByCriticalOverlap { task_id, resources });
        }

        // 2. Fetch task and check status
        let task = TaskRepository::find_by_id(pool, task_id)
            .await
            .map_err(ApprovalGateError::Database)?
            .ok_or(ApprovalGateError::TaskNotFound(task_id))?;

        if task.status != TaskStatus::HumanReview && task.status != TaskStatus::Failed {
            return Err(ApprovalGateError::InvalidTaskStatus {
                task_id,
                current: task.status,
            });
        }

        // 3. Record approval in task_approvals
        ProposalRepository::record_approval(
            pool,
            &NewTaskApproval {
                task_id,
                proposal_id: task.proposal_id,
                status: ApprovalStatus::Approved,
                edited_desc: None,
                approved_by: approved_by.to_string(),
            },
        )
        .await
        .map_err(ApprovalGateError::Database)?;

        // 4. Update task status to Approved
        let updated_task = TaskRepository::update_status(pool, task_id, TaskStatus::Approved)
            .await
            .map_err(ApprovalGateError::Database)?
            .ok_or(ApprovalGateError::TaskNotFound(task_id))?;

        Ok(updated_task)
    }

    /// Executes the EditAndApproveTask command, saving the updated description and recording approval.
    pub async fn execute_edit_and_approve_task(
        pool: &PgPool,
        task_id: Uuid,
        new_description: &str,
        approved_by: &str,
    ) -> Result<Task, ApprovalGateError> {
        // 1. Check for unacknowledged critical overlaps
        let critical_overlaps = OverlapWarningRepository::list_unacknowledged_critical_for_task(pool, task_id)
            .await
            .map_err(ApprovalGateError::Database)?;

        if !critical_overlaps.is_empty() {
            let resources = critical_overlaps.into_iter().map(|w| w.resource).collect();
            return Err(ApprovalGateError::BlockedByCriticalOverlap { task_id, resources });
        }

        // 2. Fetch task and check status
        let task = TaskRepository::find_by_id(pool, task_id)
            .await
            .map_err(ApprovalGateError::Database)?
            .ok_or(ApprovalGateError::TaskNotFound(task_id))?;

        if task.status != TaskStatus::HumanReview && task.status != TaskStatus::Failed {
            return Err(ApprovalGateError::InvalidTaskStatus {
                task_id,
                current: task.status,
            });
        }

        // 3. Update task description in PostgreSQL
        sqlx::query!(
            "UPDATE tasks SET description = $2, updated_at = NOW() WHERE id = $1",
            task_id,
            new_description
        )
        .execute(pool)
        .await
        .context("Failed to update task description")
        .map_err(ApprovalGateError::Database)?;

        // 4. Record approval with status EditedAndApproved
        ProposalRepository::record_approval(
            pool,
            &NewTaskApproval {
                task_id,
                proposal_id: task.proposal_id,
                status: ApprovalStatus::EditedAndApproved,
                edited_desc: Some(new_description.to_string()),
                approved_by: approved_by.to_string(),
            },
        )
        .await
        .map_err(ApprovalGateError::Database)?;

        // 5. Update task status to Approved
        let updated_task = TaskRepository::update_status(pool, task_id, TaskStatus::Approved)
            .await
            .map_err(ApprovalGateError::Database)?
            .ok_or(ApprovalGateError::TaskNotFound(task_id))?;

        Ok(updated_task)
    }

    /// Executes the RejectTask command, recording rejection and setting task to Rejected.
    pub async fn execute_reject_task(
        pool: &PgPool,
        task_id: Uuid,
        rejected_by: &str,
    ) -> Result<Task> {
        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

        ProposalRepository::record_approval(
            pool,
            &NewTaskApproval {
                task_id,
                proposal_id: task.proposal_id,
                status: ApprovalStatus::Rejected,
                edited_desc: None,
                approved_by: rejected_by.to_string(),
            },
        )
        .await?;

        let updated = TaskRepository::update_status(pool, task_id, TaskStatus::Rejected)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

        Ok(updated)
    }

    /// Acknowledges a resource overlap warning, unblocking approval for affected tasks.
    pub async fn execute_acknowledge_overlap(
        pool: &PgPool,
        warning_id: Uuid,
    ) -> Result<bool> {
        OverlapWarningRepository::acknowledge(pool, warning_id).await
    }

    /// Allows human to override agent assignment proposal or reassign a failed task.
    pub async fn execute_reassign_task(
        pool: &PgPool,
        task_id: Uuid,
        new_agent_id: Uuid,
        approved_by: &str,
    ) -> Result<Task> {
        let agent = AgentRepository::find_by_id(pool, new_agent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Target agent not found"))?;

        if agent.status == AgentStatus::Offline {
            anyhow::bail!("Cannot reassign to offline agent");
        }

        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

        TaskRepository::assign_agent(pool, task_id, Some(new_agent_id)).await?;

        // If the task was Failed or in HumanReview, approve it for reassignment
        if task.status == TaskStatus::Failed || task.status == TaskStatus::HumanReview {
            ProposalRepository::record_approval(
                pool,
                &NewTaskApproval {
                    task_id,
                    proposal_id: task.proposal_id,
                    status: ApprovalStatus::Approved,
                    edited_desc: None,
                    approved_by: approved_by.to_string(),
                },
            )
            .await?;
            TaskRepository::update_status(pool, task_id, TaskStatus::Approved).await?;
        }

        let updated = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found after reassign"))?;

        Ok(updated)
    }
}
