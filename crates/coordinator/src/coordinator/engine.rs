use anyhow::{bail, Result};
use async_nats::jetstream::Context as JetStreamContext;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use agent_protocol::AgentMessage;
use crate::coordinator::assignment::{AssignmentResult, AssignmentService};
use crate::coordinator::commands::{ApprovalGateError, CommandHandler, CoordinatorCommand, CoordinatorEvent};
use crate::coordinator::state::CoordinatorState;
use crate::db::repositories::TaskRepository;
use crate::domain::TaskStatus;
use crate::messaging::subscriber::EventSubscriber;

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

            CoordinatorCommand::ApproveTask { task_id, approved_by } => {
                match CommandHandler::execute_approve_task(&self.pool, task_id, &approved_by).await {
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
                            reason: format!("Blocked by unacknowledged critical conflict on: {}", resources.join(", ")),
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
                            reason: format!("Blocked by unacknowledged critical conflict on: {}", resources.join(", ")),
                        })
                    }
                    Err(e) => Ok(CoordinatorEvent::CommandFailed {
                        message: format!("EditAndApproveTask failed: {e}"),
                    }),
                }
            }

            CoordinatorCommand::RejectTask { task_id, rejected_by } => {
                CommandHandler::execute_reject_task(&self.pool, task_id, &rejected_by).await?;
                Ok(CoordinatorEvent::TaskRejected { task_id })
            }

            CoordinatorCommand::AcknowledgeOverlap { warning_id, .. } => {
                CommandHandler::execute_acknowledge_overlap(&self.pool, warning_id).await?;
                Ok(CoordinatorEvent::OverlapAcknowledged { warning_id })
            }

            CoordinatorCommand::ReassignTask { task_id, new_agent_id, approved_by } => {
                let task = CommandHandler::execute_reassign_task(&self.pool, task_id, new_agent_id, &approved_by).await?;
                Ok(CoordinatorEvent::TaskStatusChanged {
                    task_id,
                    old_status: task.status,
                    new_status: TaskStatus::Approved,
                })
            }

            CoordinatorCommand::CancelTask { task_id, .. } => {
                TaskRepository::update_status(&self.pool, task_id, TaskStatus::Cancelled).await?;
                Ok(CoordinatorEvent::TaskStatusChanged {
                    task_id,
                    old_status: TaskStatus::Assigned,
                    new_status: TaskStatus::Cancelled,
                })
            }

            CoordinatorCommand::RequestRefresh => {
                Ok(CoordinatorEvent::StateChanged(self.state))
            }
        }
    }

    /// Triggers an assignment cycle for ready, approved tasks whose blockers are completed.
    pub async fn run_assignment_cycle(&mut self) -> Result<Vec<AssignmentResult>> {
        let results = AssignmentService::assign_ready_tasks(&self.pool, self.active_project_id, self.jetstream.as_ref()).await?;

        if !results.is_empty() {
            if self.state == CoordinatorState::Assigning || self.state == CoordinatorState::HumanReview {
                let _ = self.transition_state(CoordinatorState::Executing);
            }
        }

        Ok(results)
    }

    /// Runs out-of-band reconciliation for pending deliveries.
    pub async fn run_reconciliation_cycle(&self) -> Result<usize> {
        AssignmentService::reconcile_pending_deliveries(&self.pool, self.jetstream.as_ref()).await
    }

    /// Runs full crash recovery and stale sweep on coordinator startup.
    pub async fn recover_on_startup(&self) -> Result<crate::reliability::RecoveryReport> {
        crate::reliability::CoordinatorRecoveryService::recover_on_startup(&self.pool, self.jetstream.as_ref()).await
    }

    /// Runs periodic stale task sweep and reclamation cycle.
    pub async fn run_stale_sweep(&self, timeout: chrono::Duration) -> Result<crate::reliability::SweepResult> {
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

        // 1. Process in event subscriber (persists to PostgreSQL)
        EventSubscriber::handle_agent_message(&self.pool, msg).await?;

        // 2. Check for project completion if a task completed
        if let Some(tid) = task_id {
            if let Some(task) = TaskRepository::find_by_id(&self.pool, tid).await? {
                if let Some(proj_id) = self.active_project_id {
                    if task.project_id == proj_id {
                        let project_tasks = TaskRepository::list_by_project(&self.pool, proj_id).await?;
                        let all_completed = project_tasks.iter().all(|t| t.status == TaskStatus::Completed);
                        if all_completed && !project_tasks.is_empty() && self.state == CoordinatorState::Executing {
                            let _ = self.transition_state(CoordinatorState::Done);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
