use anyhow::{bail, Result};
use async_nats::jetstream::Context as JetStreamContext;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::coordinator::assignment::{AssignmentResult, AssignmentService};
use crate::coordinator::commands::{
    ApprovalGateError, CommandHandler, CoordinatorCommand, CoordinatorEvent,
};
use crate::coordinator::state::CoordinatorState;
use crate::db::repositories::TaskRepository;
use crate::domain::TaskStatus;
use crate::messaging::subscriber::EventSubscriber;
use agent_protocol::AgentMessage;

/// Central coordinator core orchestrating project decomposition, human approval gating,
/// agent assignment, and execution tracking.
pub struct CoordinatorCore {
    state: CoordinatorState,
    pool: PgPool,
    jetstream: Option<JetStreamContext>,
    active_project_id: Option<Uuid>,
}

impl CoordinatorCore {
    pub fn new(pool: PgPool, jetstream: Option<JetStreamContext>) -> Self {
        Self {
            state: CoordinatorState::Idle,
            pool,
            jetstream,
            active_project_id: None,
        }
    }

    pub fn state(&self) -> CoordinatorState {
        self.state
    }

    pub fn set_active_project(&mut self, project_id: Uuid) {
        self.active_project_id = Some(project_id);
    }

    pub fn active_project_id(&self) -> Option<Uuid> {
        self.active_project_id
    }

    /// Transitions the coordinator's operational state machine.
    pub fn transition_state(&mut self, next: CoordinatorState) -> Result<()> {
        if !self.state.can_transition_to(&next) {
            bail!(
                "Invalid coordinator state transition from {:?} to {:?}",
                self.state,
                next
            );
        }
        info!(from = ?self.state, to = ?next, "Coordinator state changed");
        self.state = next;
        Ok(())
    }

    /// Dispatches and processes an incoming command.
    pub async fn handle_command(&mut self, cmd: CoordinatorCommand) -> Result<CoordinatorEvent> {
        match cmd {
            CoordinatorCommand::SubmitProject { .. } => {
                self.transition_state(CoordinatorState::ProjectInput)?;
                let project_id = self.active_project_id.unwrap_or_else(Uuid::new_v4);
                Ok(CoordinatorEvent::ProjectSubmitted { project_id })
            }

            CoordinatorCommand::ApproveTask {
                task_id,
                approved_by,
            } => {
                match CommandHandler::execute_approve_task(&self.pool, task_id, &approved_by).await
                {
                    Ok(_) => {
                        // Advance coordinator state to Assigning/Executing if in HumanReview
                        if self.state == CoordinatorState::HumanReview {
                            let _ = self.transition_state(CoordinatorState::Assigning);
                        }
                        Ok(CoordinatorEvent::TaskApproved { task_id })
                    }
                    Err(ApprovalGateError::BlockedByCriticalOverlap { task_id, resources }) => {
                        Ok(CoordinatorEvent::TaskApprovalBlocked {
                            task_id,
                            reason: format!(
                                "Blocked by unacknowledged critical conflict on: {}",
                                resources.join(", ")
                            ),
                        })
                    }
                    Err(e) => Ok(CoordinatorEvent::CommandFailed {
                        message: format!("ApproveTask failed: {e}"),
                    }),
                }
            }

            CoordinatorCommand::EditAndApproveTask {
                task_id,
                new_description,
                approved_by,
            } => {
                match CommandHandler::execute_edit_and_approve_task(
                    &self.pool,
                    task_id,
                    &new_description,
                    &approved_by,
                )
                .await
                {
                    Ok(_) => {
                        if self.state == CoordinatorState::HumanReview {
                            let _ = self.transition_state(CoordinatorState::Assigning);
                        }
                        Ok(CoordinatorEvent::TaskApproved { task_id })
                    }
                    Err(ApprovalGateError::BlockedByCriticalOverlap { task_id, resources }) => {
                        Ok(CoordinatorEvent::TaskApprovalBlocked {
                            task_id,
                            reason: format!(
                                "Blocked by unacknowledged critical conflict on: {}",
                                resources.join(", ")
                            ),
                        })
                    }
                    Err(e) => Ok(CoordinatorEvent::CommandFailed {
                        message: format!("EditAndApproveTask failed: {e}"),
                    }),
                }
            }

            CoordinatorCommand::RejectTask {
                task_id,
                rejected_by,
            } => {
                CommandHandler::execute_reject_task(&self.pool, task_id, &rejected_by).await?;
                Ok(CoordinatorEvent::TaskRejected { task_id })
            }

            CoordinatorCommand::AcknowledgeOverlap { warning_id, .. } => {
                CommandHandler::execute_acknowledge_overlap(&self.pool, warning_id).await?;
                Ok(CoordinatorEvent::OverlapAcknowledged { warning_id })
            }

            CoordinatorCommand::ReassignTask {
                task_id,
                new_agent_id,
                approved_by,
            } => {
                let task = CommandHandler::execute_reassign_task(
                    &self.pool,
                    task_id,
                    new_agent_id,
                    &approved_by,
                )
                .await?;
                Ok(CoordinatorEvent::TaskStatusChanged {
                    task_id,
                    old_status: task.status,
                    new_status: TaskStatus::Approved,
                })
            }

            CoordinatorCommand::CancelTask { task_id, reason } => {
                let task_before = TaskRepository::find_by_id(&self.pool, task_id).await?;
                let old_status = task_before
                    .as_ref()
                    .map(|t| t.status)
                    .unwrap_or(TaskStatus::Assigned);
                let assigned_agent = task_before.and_then(|t| t.assigned_agent_id);

                let _ = CommandHandler::execute_cancel_task(&self.pool, task_id, &reason).await?;

                if let (Some(agent_id), Some(js)) = (assigned_agent, self.jetstream.as_ref()) {
                    let _ = crate::messaging::publisher::TaskPublisher::publish_cancellation(
                        js, agent_id, task_id, &reason,
                    )
                    .await;
                }

                Ok(CoordinatorEvent::TaskStatusChanged {
                    task_id,
                    old_status,
                    new_status: TaskStatus::Cancelled,
                })
            }

            CoordinatorCommand::TriggerReplanning { project_id, reason } => {
                let _ = self.trigger_replanning(project_id, &reason, None).await?;
                Ok(CoordinatorEvent::StateChanged(self.state))
            }

            CoordinatorCommand::RequestRefresh => Ok(CoordinatorEvent::StateChanged(self.state)),
        }
    }

    /// Triggers the dynamic re-planning workflow for a project when a task fails,
    /// a cross-agent conflict is detected, or the operator requests an adjustment.
    ///
    /// Preserves the strict human approval gate: all newly generated tasks from replanning
    /// are stored in `TaskStatus::HumanReview` and cannot execute until approved.
    pub async fn trigger_replanning(
        &mut self,
        project_id: Uuid,
        reason: &str,
        provider_override: Option<&dyn crate::ai::LlmProvider>,
    ) -> Result<Uuid> {
        tracing::info!(%project_id, %reason, "Triggering dynamic replanning");

        // 1. Gather replan context across tasks, agents, conflicts, and unexpected changes
        let replan_req =
            crate::ai::ReplanEngine::gather_replan_context(&self.pool, project_id, None).await?;

        // 2. Resolve LLM provider
        let env_provider;
        let provider: &dyn crate::ai::LlmProvider = match provider_override {
            Some(p) => p,
            None => {
                env_provider = crate::ai::create_provider_from_env()?;
                env_provider.as_ref()
            }
        };

        // 3. Execute replan and persist new proposal & tasks (all tasks created in HumanReview status)
        let proposal_id =
            crate::ai::ReplanEngine::execute_replan(&self.pool, provider, &replan_req).await?;

        // 4. Record coordinator event
        let _ = crate::observability::CoordinatorEventRepository::record(
            &self.pool,
            "project.replanned",
            Some(project_id),
            None,
            None,
            format!("Dynamic replanning generated proposal {proposal_id}: {reason}"),
            serde_json::json!({
                "proposal_id": proposal_id,
                "reason": reason,
                "failed_tasks": replan_req.failed_tasks.len(),
                "completed_tasks": replan_req.completed_tasks.len(),
            }),
        )
        .await;

        // 5. Update coordinator state to HumanReview so the operator can review the plan
        let _ = self.transition_state(CoordinatorState::HumanReview);

        Ok(proposal_id)
    }

    /// Triggers an assignment cycle for ready, approved tasks whose blockers are completed.
    pub async fn run_assignment_cycle(&mut self) -> Result<Vec<AssignmentResult>> {
        let results = AssignmentService::assign_ready_tasks(
            &self.pool,
            self.active_project_id,
            self.jetstream.as_ref(),
        )
        .await?;

        if !results.is_empty()
            && (self.state == CoordinatorState::Assigning
                || self.state == CoordinatorState::HumanReview)
        {
            let _ = self.transition_state(CoordinatorState::Executing);
        }

        Ok(results)
    }

    /// Runs out-of-band reconciliation for pending deliveries.
    pub async fn run_reconciliation_cycle(&self) -> Result<usize> {
        AssignmentService::reconcile_pending_deliveries(&self.pool, self.jetstream.as_ref()).await
    }

    /// Runs full crash recovery and stale sweep on coordinator startup.
    pub async fn recover_on_startup(&self) -> Result<crate::reliability::RecoveryReport> {
        crate::reliability::CoordinatorRecoveryService::recover_on_startup(
            &self.pool,
            self.jetstream.as_ref(),
        )
        .await
    }

    /// Runs periodic stale task sweep and reclamation cycle.
    pub async fn run_stale_sweep(
        &self,
        timeout: chrono::Duration,
    ) -> Result<crate::reliability::SweepResult> {
        crate::reliability::StaleTaskSweeper::sweep(&self.pool, timeout).await
    }

    /// Ingests and processes an incoming agent protocol message.
    pub async fn handle_agent_message(&mut self, msg: AgentMessage) -> Result<()> {
        let task_id = match &msg {
            AgentMessage::TaskStarted { task_id, .. } => Some(*task_id),
            AgentMessage::ProgressUpdate { task_id, .. } => Some(*task_id),
            AgentMessage::Blocked { task_id, .. } => Some(*task_id),
            AgentMessage::Completed { task_id, .. } => Some(*task_id),
            AgentMessage::Failed { task_id, .. } => Some(*task_id),
            _ => None,
        };

        // 1. Process in event subscriber (persists to PostgreSQL and unblocks dependencies)
        EventSubscriber::handle_agent_message_with_jetstream(
            &self.pool,
            msg.clone(),
            self.jetstream.as_ref(),
        )
        .await?;

        // 2. Dynamic replanning trigger on task failure (behind strict human approval gate)
        if let AgentMessage::Failed {
            task_id: fail_tid,
            ref error,
            ..
        } = msg
        {
            if let Ok(Some(task)) = TaskRepository::find_by_id(&self.pool, fail_tid).await {
                tracing::info!(
                    task_id = %fail_tid,
                    project_id = %task.project_id,
                    "Task execution failed; triggering dynamic replanning behind human approval gate"
                );
                let reason = format!("Task {} execution failed: {}", task.short_id, error);
                if let Err(e) = self
                    .trigger_replanning(task.project_id, &reason, None)
                    .await
                {
                    tracing::warn!(error = %e, "Dynamic replanning on task failure skipped or failed");
                }
            }
        }

        // 3. Check for project completion if a task completed
        if let Some(tid) = task_id {
            if let Some(task) = TaskRepository::find_by_id(&self.pool, tid).await? {
                if let Some(proj_id) = self.active_project_id {
                    if task.project_id == proj_id {
                        let project_tasks =
                            TaskRepository::list_by_project(&self.pool, proj_id).await?;
                        let all_completed = project_tasks
                            .iter()
                            .all(|t| t.status == TaskStatus::Completed);
                        if all_completed
                            && !project_tasks.is_empty()
                            && self.state == CoordinatorState::Executing
                        {
                            let _ = self.transition_state(CoordinatorState::Done);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
