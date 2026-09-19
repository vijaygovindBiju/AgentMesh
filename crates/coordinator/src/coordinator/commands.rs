use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProposalRepository, TaskDeliveryRepository, TaskRepository,
};
use crate::domain::{
    AgentStatus, ApprovalStatus, DeliveryStatus, NewTaskApproval, Task, TaskStatus,
};
use crate::observability::CoordinatorEventRepository;

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
    TriggerReplanning {
        project_id: Uuid,
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

        let _ = CoordinatorEventRepository::record(
            pool,
            "task.approved",
            Some(task.project_id),
            Some(task.id),
            task.assigned_agent_id,
            format!("Task {} approved by {}", task.short_id, approved_by),
            serde_json::json!({ "task_id": task.id, "approved_by": approved_by }),
        )
        .await;

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

        let _ = CoordinatorEventRepository::record(
            pool,
            "task.approved",
            Some(task.project_id),
            Some(task.id),
            task.assigned_agent_id,
            format!("Task {} edited and approved by {}", task.short_id, approved_by),
            serde_json::json!({ "task_id": task.id, "approved_by": approved_by, "edited": true }),
        )
        .await;

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

        let _ = CoordinatorEventRepository::record(
            pool,
            "task.rejected",
            Some(task.project_id),
            Some(task.id),
            task.assigned_agent_id,
            format!("Task {} rejected by {}", task.short_id, rejected_by),
            serde_json::json!({ "task_id": task.id, "rejected_by": rejected_by }),
        )
        .await;

        Ok(updated)
    }

    /// Acknowledges a resource overlap warning, unblocking approval for affected tasks.
    pub async fn execute_acknowledge_overlap(
        pool: &PgPool,
        warning_id: Uuid,
    ) -> Result<bool> {
        let res = OverlapWarningRepository::acknowledge(pool, warning_id).await?;
        if res {
            let _ = CoordinatorEventRepository::record(
                pool,
                "overlap.acknowledged",
                None,
                None,
                None,
                format!("Overlap warning {warning_id} acknowledged"),
                serde_json::json!({ "warning_id": warning_id }),
            )
            .await;
        }
        Ok(res)
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

        let _ = CoordinatorEventRepository::record(
            pool,
            "task.reassigned",
            Some(task.project_id),
            Some(task.id),
            Some(new_agent_id),
            format!("Task {} reassigned to agent {} by {}", task.short_id, new_agent_id, approved_by),
            serde_json::json!({
                "task_id": task.id,
                "new_agent_id": new_agent_id,
                "approved_by": approved_by,
            }),
        )
        .await;

        Ok(updated)
    }

    /// Cancels a task, validating state, freeing agent, and marking deliveries terminal.
    pub async fn execute_cancel_task(
        pool: &PgPool,
        task_id: Uuid,
        reason: &str,
    ) -> Result<Task> {
        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found"))?;

        if task.status == TaskStatus::Completed
            || task.status == TaskStatus::Failed
            || task.status == TaskStatus::Cancelled
            || task.status == TaskStatus::Rejected
        {
            anyhow::bail!(
                "Cannot cancel task {task_id}: task is already in terminal status {:?}",
                task.status
            );
        }

        // Mark any non-terminal deliveries as Terminal
        let deliveries = TaskDeliveryRepository::list_by_task(pool, task_id).await.unwrap_or_default();
        for del in deliveries {
            if del.status == DeliveryStatus::Pending
                || del.status == DeliveryStatus::Delivered
                || del.status == DeliveryStatus::Acknowledged
            {
                let _ = TaskDeliveryRepository::mark_terminal(pool, del.id, reason).await;
            }
        }

        // Free assigned agent if active
        if let Some(agent_id) = task.assigned_agent_id {
            let _ = AgentRepository::set_current_task(pool, agent_id, None, AgentStatus::Idle).await;
        }

        // Clean up git worktree if present
        if let Ok(Some(gc)) = TaskRepository::find_git_context(pool, task_id).await {
            if let (Some(ref repo_path_str), Some(ref task_branch)) = (gc.repo_path, gc.task_branch) {
                let repo_root = std::path::Path::new(&repo_path_str);
                let worktree_path = crate::git::AgentWorkspace::expected_worktree_path(repo_root, &task.short_id);
                if worktree_path.exists() {
                    let ws = crate::git::AgentWorkspace {
                        task_id,
                        short_id: task.short_id.clone(),
                        repo_root: repo_root.to_path_buf(),
                        worktree_path,
                        task_branch: task_branch.clone(),
                        base_commit_sha: gc.base_commit_sha.unwrap_or_default(),
                    };
                    let _ = ws.cleanup().await;
                }
            }
        }

        // Update task status to Cancelled
        let updated = TaskRepository::update_status(pool, task_id, TaskStatus::Cancelled)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task not found after cancel"))?;

        // Record coordinator event
        let _ = CoordinatorEventRepository::record(
            pool,
            "task.cancelled",
            Some(task.project_id),
            Some(task.id),
            task.assigned_agent_id,
            format!("Task {} cancelled: {reason}", task.short_id),
            serde_json::json!({
                "task_id": task.id,
                "short_id": task.short_id,
                "reason": reason,
                "previous_status": task.status,
            }),
        )
        .await;

        Ok(updated)
    }
}
