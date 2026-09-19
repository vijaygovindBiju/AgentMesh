use anyhow::{Context, Result};
use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::Context as JetStreamContext;
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::db::repositories::{
    AgentEventRepository, AgentRepository, TaskDeliveryRepository, TaskRepository,
};
use crate::domain::{
    AckKind, AgentEventType, AgentStatus, DeliveryStatus, NewAgentEvent, TaskStatus,
};
use crate::git::{AgentWorkspace, GitCoordinator};
use crate::messaging::publisher::TaskPublisher;
use crate::messaging::streams::{AGENT_EVENTS_STREAM, AGENT_EVENTS_SUBJECT};
use crate::observability::CoordinatorEventRepository;
use crate::security::audit::{AuditEvent, AuditLogger};
use crate::security::task_auth::TaskAuthorizer;
use crate::security::SecurityError;
use agent_protocol::AgentMessage;
use uuid::Uuid;

static DEDUPLICATOR: std::sync::OnceLock<crate::reliability::EventDeduplicator> =
    std::sync::OnceLock::new();

pub struct EventSubscriber;

impl EventSubscriber {
    pub fn get_deduplicator() -> &'static crate::reliability::EventDeduplicator {
        DEDUPLICATOR.get_or_init(crate::reliability::EventDeduplicator::default)
    }

    /// Validates that an agent has authority to act on a task before processing its events.
    async fn check_task_auth(
        pool: &PgPool,
        agent_id: Uuid,
        task_id: Uuid,
        action: &str,
    ) -> Result<bool> {
        match TaskAuthorizer::authorize_agent_for_task(pool, agent_id, task_id, action).await {
            Ok(()) => Ok(true),
            Err(SecurityError::TaskImpersonation {
                actor_agent_id,
                task_id,
                assigned_to,
                action,
            }) => {
                let _ = AuditLogger::log_task_impersonation(
                    pool,
                    actor_agent_id,
                    task_id,
                    assigned_to,
                    &action,
                )
                .await;
                warn!(%actor_agent_id, %task_id, ?assigned_to, %action, "Rejected unauthorized task event: task is assigned to another agent");
                Ok(false)
            }
            Err(SecurityError::AgentRevoked(agent_id)) => {
                let _ = AuditLogger::record(
                    pool,
                    &AuditEvent::new(
                        "agent",
                        Some(agent_id.to_string()),
                        "revoked_agent_action",
                        "task",
                        Some(task_id.to_string()),
                        "denied",
                        json!({ "action": action }),
                    ),
                )
                .await;
                warn!(%agent_id, %task_id, %action, "Rejected task event: agent key is revoked");
                Ok(false)
            }
            Err(e) => {
                warn!(%agent_id, %task_id, error = %e, "Security authorization error");
                Ok(false)
            }
        }
    }

    /// Creates or retrieves a durable consumer for the AGENT_EVENTS stream.
    pub async fn create_consumer(jetstream: &JetStreamContext) -> Result<PullConsumer> {
        let stream = jetstream
            .get_stream(AGENT_EVENTS_STREAM)
            .await
            .context("Failed to get AGENT_EVENTS stream")?;

        let consumer = stream
            .get_or_create_consumer(
                "coordinator-events",
                async_nats::jetstream::consumer::pull::Config {
                    durable_name: Some("coordinator-events".to_string()),
                    filter_subject: AGENT_EVENTS_SUBJECT.to_string(),
                    ..Default::default()
                },
            )
            .await
            .context("Failed to create coordinator-events consumer")?;

        Ok(consumer)
    }

    /// Handles a single incoming AgentMessage and updates PostgreSQL state accordingly.
    pub async fn handle_agent_message(pool: &PgPool, msg: AgentMessage) -> Result<()> {
        Self::handle_agent_message_with_jetstream(pool, msg, None).await
    }

    /// Handles an incoming AgentMessage with optional JetStream context for publishing follow-up protocol messages.
    pub async fn handle_agent_message_with_jetstream(
        pool: &PgPool,
        msg: AgentMessage,
        jetstream: Option<&JetStreamContext>,
    ) -> Result<()> {
        let dedup_key = match &msg {
            AgentMessage::TaskStarted {
                agent_id, task_id, ..
            } => Some(crate::reliability::EventDeduplicator::compute_event_key(
                *agent_id,
                *task_id,
                "TaskStarted",
                None,
            )),
            AgentMessage::ProgressUpdate {
                agent_id,
                task_id,
                percent,
                ..
            } => Some(crate::reliability::EventDeduplicator::compute_event_key(
                *agent_id,
                *task_id,
                "ProgressUpdate",
                Some(&percent.to_string()),
            )),
            AgentMessage::Completed {
                agent_id, task_id, ..
            } => Some(crate::reliability::EventDeduplicator::compute_event_key(
                *agent_id,
                *task_id,
                "Completed",
                None,
            )),
            AgentMessage::Failed {
                agent_id, task_id, ..
            } => Some(crate::reliability::EventDeduplicator::compute_event_key(
                *agent_id, *task_id, "Failed", None,
            )),
            AgentMessage::Blocked {
                agent_id, task_id, ..
            } => Some(crate::reliability::EventDeduplicator::compute_event_key(
                *agent_id, *task_id, "Blocked", None,
            )),
            _ => None,
        };

        if let Some(key) = dedup_key {
            if !Self::get_deduplicator().check_or_record(&key) {
                warn!(key = %key, "Dropping duplicate agent lifecycle message");
                return Ok(());
            }
        }

        match msg {
            AgentMessage::TaskStarted {
                agent_id,
                task_id,
                idempotency_key,
                timestamp: _,
            } => {
                if !Self::check_task_auth(pool, agent_id, task_id, "task_started").await? {
                    return Ok(());
                }

                info!(%agent_id, %task_id, %idempotency_key, "Agent reported TaskStarted");

                // 1. Record agent event
                AgentEventRepository::create(
                    pool,
                    &NewAgentEvent {
                        agent_id,
                        task_id,
                        event_type: AgentEventType::TaskStarted,
                        message: Some("Task started".to_string()),
                        payload: json!({ "idempotency_key": idempotency_key }),
                    },
                )
                .await?;

                // 2. Update TaskDelivery if exists
                if let Some(delivery) =
                    TaskDeliveryRepository::find_by_idempotency_key(pool, &idempotency_key).await?
                {
                    TaskDeliveryRepository::record_ack(
                        pool,
                        delivery.id,
                        AckKind::Ack,
                        DeliveryStatus::Acknowledged,
                    )
                    .await?;
                }

                // 3. Update task status (Assigned -> Executing)
                TaskRepository::update_status(pool, task_id, TaskStatus::Executing).await?;

                // 4. Update agent status to Busy
                AgentRepository::set_current_task(pool, agent_id, Some(task_id), AgentStatus::Busy)
                    .await?;

                // 5. Record coordinator observability event
                let task = TaskRepository::find_by_id(pool, task_id)
                    .await
                    .ok()
                    .flatten();
                let short_id = task.as_ref().map(|t| t.short_id.as_str()).unwrap_or("TASK");
                let project_id = task.as_ref().map(|t| t.project_id);

                let _ = CoordinatorEventRepository::record(
                    pool,
                    "task.started",
                    project_id,
                    Some(task_id),
                    Some(agent_id),
                    format!("Task {short_id} execution started by agent {agent_id}"),
                    json!({ "idempotency_key": idempotency_key }),
                )
                .await;
            }

            AgentMessage::ProgressUpdate {
                agent_id,
                task_id,
                message,
                percent,
                timestamp: _,
            } => {
                if !Self::check_task_auth(pool, agent_id, task_id, "progress_update").await? {
                    return Ok(());
                }

                info!(%agent_id, %task_id, percent, %message, "Agent reported ProgressUpdate");
                AgentEventRepository::create(
                    pool,
                    &NewAgentEvent {
                        agent_id,
                        task_id,
                        event_type: AgentEventType::ProgressUpdate,
                        message: Some(message.clone()),
                        payload: json!({ "percent": percent, "message": message }),
                    },
                )
                .await?;

                let task = TaskRepository::find_by_id(pool, task_id)
                    .await
                    .ok()
                    .flatten();
                let short_id = task.as_ref().map(|t| t.short_id.as_str()).unwrap_or("TASK");
                let project_id = task.as_ref().map(|t| t.project_id);

                let _ = CoordinatorEventRepository::record(
                    pool,
                    "task.progress",
                    project_id,
                    Some(task_id),
                    Some(agent_id),
                    format!("Task {short_id} progress ({percent}%): {message}"),
                    json!({ "percent": percent, "message": message }),
                )
                .await;
            }

            AgentMessage::Blocked {
                agent_id,
                task_id,
                reason,
                blocking_task_id,
                timestamp: _,
            } => {
                if !Self::check_task_auth(pool, agent_id, task_id, "blocked").await? {
                    return Ok(());
                }

                warn!(%agent_id, %task_id, %reason, ?blocking_task_id, "Agent reported Blocked");

                // 1. Record event
                AgentEventRepository::create(
                    pool,
                    &NewAgentEvent {
                        agent_id,
                        task_id,
                        event_type: AgentEventType::Blocked,
                        message: Some(reason.clone()),
                        payload: json!({ "reason": reason, "blocking_task_id": blocking_task_id }),
                    },
                )
                .await?;

                // 2. Update Task to Blocked
                TaskRepository::update_status(pool, task_id, TaskStatus::Blocked).await?;

                // 3. Update Agent status to Blocked
                AgentRepository::update_status(pool, agent_id, AgentStatus::Blocked).await?;

                let task = TaskRepository::find_by_id(pool, task_id)
                    .await
                    .ok()
                    .flatten();
                let short_id = task.as_ref().map(|t| t.short_id.as_str()).unwrap_or("TASK");
                let project_id = task.as_ref().map(|t| t.project_id);

                let _ = CoordinatorEventRepository::record(
                    pool,
                    "task.blocked",
                    project_id,
                    Some(task_id),
                    Some(agent_id),
                    format!("Task {short_id} blocked: {reason}"),
                    json!({ "reason": reason, "blocking_task_id": blocking_task_id }),
                )
                .await;

                // If agent is blocked on a dependency task, publish WaitForDependency over JetStream
                if let (Some(blocker_id), Some(js)) = (blocking_task_id, jetstream) {
                    let _ = TaskPublisher::publish_wait_for_dependency(
                        js, agent_id, task_id, blocker_id, &reason,
                    )
                    .await;
                }
            }

            AgentMessage::Completed {
                agent_id,
                task_id,
                summary,
                timestamp: _,
            } => {
                if !Self::check_task_auth(pool, agent_id, task_id, "completed").await? {
                    return Ok(());
                }

                info!(%agent_id, %task_id, %summary, "Agent reported Completed");

                // 1. Record event
                AgentEventRepository::create(
                    pool,
                    &NewAgentEvent {
                        agent_id,
                        task_id,
                        event_type: AgentEventType::Completed,
                        message: Some(summary.clone()),
                        payload: json!({ "summary": summary }),
                    },
                )
                .await?;

                // 2. Update Task to Completed
                TaskRepository::update_status(pool, task_id, TaskStatus::Completed).await?;

                // 3. Free agent -> Idle and record completion metrics
                AgentRepository::set_current_task(pool, agent_id, None, AgentStatus::Idle).await?;
                let _ = AgentRepository::record_task_completion(pool, agent_id).await?;

                let task = TaskRepository::find_by_id(pool, task_id)
                    .await
                    .ok()
                    .flatten();
                let short_id = task.as_ref().map(|t| t.short_id.as_str()).unwrap_or("TASK");
                let project_id = task.as_ref().map(|t| t.project_id);

                // Finalize Git workspace if worktree exists
                if let Ok(Some(gc)) = TaskRepository::find_git_context(pool, task_id).await {
                    if let (Some(ref repo_path_str), Some(ref task_branch)) =
                        (gc.repo_path, gc.task_branch)
                    {
                        let repo_root = std::path::Path::new(&repo_path_str);
                        let worktree_path =
                            AgentWorkspace::expected_worktree_path(repo_root, short_id);
                        if worktree_path.exists() {
                            let ws = AgentWorkspace {
                                task_id,
                                short_id: short_id.to_string(),
                                repo_root: repo_root.to_path_buf(),
                                worktree_path,
                                task_branch: task_branch.clone(),
                                base_commit_sha: gc.base_commit_sha.unwrap_or_default(),
                            };
                            let base_branch = gc.base_branch.as_deref().unwrap_or("main");
                            let title = task
                                .as_ref()
                                .map(|t| t.title.as_str())
                                .unwrap_or("completed task");
                            let git_coord = GitCoordinator::new(pool.clone());
                            match git_coord
                                .finalize_task(&ws, short_id, title, base_branch)
                                .await
                            {
                                Ok(git_res) => {
                                    info!(
                                        task_id = %task_id,
                                        sha = %git_res.completion_commit_sha,
                                        clean = git_res.can_merge_cleanly,
                                        "Finalized task git workspace"
                                    );
                                    let _ = TaskRepository::record_completion_git_state(
                                        pool,
                                        task_id,
                                        &git_res.completion_commit_sha,
                                        &git_res.actual_modified_resources,
                                    )
                                    .await;
                                }
                                Err(e) => {
                                    warn!(
                                        task_id = %task_id,
                                        error = %e,
                                        "Failed to finalize task git state; preserving worktree for diagnostics"
                                    );
                                }
                            }
                        }
                    }
                }

                let _ = CoordinatorEventRepository::record(
                    pool,
                    "task.completed",
                    project_id,
                    Some(task_id),
                    Some(agent_id),
                    format!("Task {short_id} completed successfully by agent {agent_id}"),
                    json!({ "summary": summary }),
                )
                .await;

                // 4. Check if any blocked tasks in this project can now be unblocked
                if let Some(proj_id) = project_id {
                    let blocked_tasks = sqlx::query_as!(
                        crate::domain::Task,
                        r#"
                        SELECT id, project_id, short_id, title, description,
                               status AS "status: TaskStatus", assigned_agent_id,
                               affected_resources, estimated_size, proposal_id,
                               created_at, updated_at
                        FROM tasks
                        WHERE project_id = $1 AND status = 'blocked'
                        "#,
                        proj_id
                    )
                    .fetch_all(pool)
                    .await
                    .unwrap_or_default();

                    for bt in blocked_tasks {
                        let unsatisfied_deps = sqlx::query!(
                            r#"
                            SELECT count(*) as count
                            FROM task_dependencies td
                            JOIN tasks b ON td.depends_on_id = b.id
                            WHERE td.dependent_id = $1
                              AND td.kind = 'blocks'
                              AND b.status <> 'completed'
                            "#,
                            bt.id
                        )
                        .fetch_one(pool)
                        .await
                        .map(|r| r.count.unwrap_or(0))
                        .unwrap_or(1);

                        if unsatisfied_deps == 0 {
                            info!(task_id = %bt.id, short_id = %bt.short_id, "Prerequisites complete; unblocking task to Approved");
                            let _ =
                                TaskRepository::update_status(pool, bt.id, TaskStatus::Approved)
                                    .await;
                            let _ = CoordinatorEventRepository::record(
                                pool,
                                "task.unblocked",
                                Some(proj_id),
                                Some(bt.id),
                                bt.assigned_agent_id,
                                format!(
                                    "Task {} unblocked after completion of prerequisite task {}",
                                    bt.short_id, short_id
                                ),
                                json!({ "unblocked_by": task_id }),
                            )
                            .await;
                        }
                    }
                }
            }

            AgentMessage::Failed {
                agent_id,
                task_id,
                error,
                timestamp: _,
            } => {
                if !Self::check_task_auth(pool, agent_id, task_id, "failed").await? {
                    return Ok(());
                }

                error!(%agent_id, %task_id, %error, "Agent reported Failed");

                // 1. Record event
                AgentEventRepository::create(
                    pool,
                    &NewAgentEvent {
                        agent_id,
                        task_id,
                        event_type: AgentEventType::Failed,
                        message: Some(error.clone()),
                        payload: json!({ "error": error }),
                    },
                )
                .await?;

                // 2. Update Task to Failed
                TaskRepository::update_status(pool, task_id, TaskStatus::Failed).await?;

                // 3. Agent to Error and record failure in health metrics
                AgentRepository::set_current_task(pool, agent_id, None, AgentStatus::Error).await?;
                let _ = AgentRepository::record_task_failure(pool, agent_id, &error).await?;

                let task = TaskRepository::find_by_id(pool, task_id)
                    .await
                    .ok()
                    .flatten();
                let short_id = task.as_ref().map(|t| t.short_id.as_str()).unwrap_or("TASK");
                let project_id = task.as_ref().map(|t| t.project_id);

                let _ = CoordinatorEventRepository::record(
                    pool,
                    "task.failed",
                    project_id,
                    Some(task_id),
                    Some(agent_id),
                    format!("Task {short_id} execution failed on agent {agent_id}: {error}"),
                    json!({ "error": error }),
                )
                .await;
            }

            AgentMessage::Heartbeat {
                agent_id,
                status,
                current_task_id: _,
                health,
                timestamp,
            } => {
                let latency_ms = health
                    .as_ref()
                    .and_then(|h| h.heartbeat_latency_ms.map(|l| l as i64))
                    .or_else(|| {
                        let elapsed = Utc::now()
                            .signed_duration_since(timestamp)
                            .num_milliseconds();
                        if elapsed >= 0 {
                            Some(elapsed)
                        } else {
                            None
                        }
                    });

                AgentRepository::record_heartbeat_with_latency(pool, agent_id, latency_ms).await?;
                let _ = AgentRepository::update_status(
                    pool,
                    agent_id,
                    match status {
                        agent_protocol::AgentStatus::Offline => AgentStatus::Offline,
                        agent_protocol::AgentStatus::Idle => AgentStatus::Idle,
                        agent_protocol::AgentStatus::Busy => AgentStatus::Busy,
                        agent_protocol::AgentStatus::Blocked => AgentStatus::Blocked,
                        agent_protocol::AgentStatus::Error => AgentStatus::Error,
                    },
                )
                .await?;

                if let Some(h) = health {
                    let _ = AgentRepository::update_health(
                        pool,
                        agent_id,
                        h.status.into(),
                        h.consecutive_failures as i32,
                        h.last_error.as_deref(),
                        latency_ms,
                    )
                    .await;
                }
            }

            AgentMessage::UpdateCapabilities {
                agent_id,
                profile,
                timestamp: _,
            } => {
                info!(%agent_id, "Agent reported updated capability profile");
                let _ =
                    AgentRepository::update_capability_profile(pool, agent_id, &profile).await?;
            }

            AgentMessage::Register { .. } => {
                // Registration is handled on coordinator.agents.register request-reply
            }
        }

        Ok(())
    }

    /// Polls and processes a single message from JetStream, updating PostgreSQL state
    /// and forwarding the event to an async channel for real-time live TUI dashboard updates.
    pub async fn process_one_event(
        pool: &PgPool,
        consumer: &PullConsumer,
        update_tx: Option<&tokio::sync::mpsc::UnboundedSender<AgentMessage>>,
    ) -> Result<Option<AgentMessage>> {
        use futures::StreamExt;
        let mut messages = consumer
            .messages()
            .await
            .context("Failed to get message stream")?;

        if let Some(msg_result) = messages.next().await {
            let msg = msg_result.context("Failed to receive message from stream")?;
            let agent_msg: AgentMessage = serde_json::from_slice(&msg.payload)
                .context("Failed to deserialize AgentMessage payload")?;

            msg.ack()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to ack message: {e}"))?;

            Self::handle_agent_message(pool, agent_msg.clone()).await?;

            if let Some(tx) = update_tx {
                let _ = tx.send(agent_msg.clone());
            }

            return Ok(Some(agent_msg));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::projects::ProjectRepository;
    use crate::db::repositories::proposals::ProposalRepository;
    use crate::domain::{AdapterType, NewAgent, NewProject, NewProposal, NewTask};
    use chrono::Utc;
    use uuid::Uuid;

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
        });
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_handle_agent_lifecycle_events() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Subscriber Test".to_string(),
                description: "Testing subscriber".to_string(),
            },
        )
        .await
        .unwrap();

        let proposal = ProposalRepository::create(
            &pool,
            &NewProposal {
                project_id: project.id,
                ai_provider: "anthropic".to_string(),
                ai_model: "claude-3-5-sonnet".to_string(),
                raw_prompt: "p".to_string(),
                raw_response: "{}".to_string(),
            },
        )
        .await
        .unwrap();

        let agent = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "Tester".to_string(),
                api_key_hash: "hash".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.test.events".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let task = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "SUB-001".to_string(),
                title: "Subscriber Task".to_string(),
                description: "Test".to_string(),
                affected_resources: vec![],
                estimated_size: None,
                proposal_id: proposal.id,
            },
        )
        .await
        .unwrap();

        // Put task into Assigned
        TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview)
            .await
            .unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Approved)
            .await
            .unwrap();
        TaskRepository::assign_agent(&pool, task.id, Some(agent.id))
            .await
            .unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned)
            .await
            .unwrap();

        // 1. Send TaskStarted
        EventSubscriber::handle_agent_message(
            &pool,
            AgentMessage::TaskStarted {
                agent_id: agent.id,
                task_id: task.id,
                idempotency_key: format!("{}:1", task.id),
                timestamp: Utc::now(),
            },
        )
        .await
        .unwrap();

        let t = TaskRepository::find_by_id(&pool, task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t.status, TaskStatus::Executing);

        let a = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.status, AgentStatus::Busy);
        assert_eq!(a.current_task_id, Some(task.id));

        // 2. Send Blocked
        EventSubscriber::handle_agent_message(
            &pool,
            AgentMessage::Blocked {
                agent_id: agent.id,
                task_id: task.id,
                reason: "Blocked by TASK-000".to_string(),
                blocking_task_id: Some(Uuid::new_v4()),
                timestamp: Utc::now(),
            },
        )
        .await
        .unwrap();

        let t = TaskRepository::find_by_id(&pool, task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t.status, TaskStatus::Blocked);

        // Resume to Executing
        TaskRepository::update_status(&pool, task.id, TaskStatus::Executing)
            .await
            .unwrap();

        // 3. Send Completed
        EventSubscriber::handle_agent_message(
            &pool,
            AgentMessage::Completed {
                agent_id: agent.id,
                task_id: task.id,
                summary: "All done".to_string(),
                timestamp: Utc::now(),
            },
        )
        .await
        .unwrap();

        let t = TaskRepository::find_by_id(&pool, task.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t.status, TaskStatus::Completed);

        let a = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a.status, AgentStatus::Idle);
        assert!(a.current_task_id.is_none());

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
        AgentRepository::delete(&pool, agent.id).await.unwrap();
    }
}
