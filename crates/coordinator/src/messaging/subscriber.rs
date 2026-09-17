use anyhow::{Context, Result};
use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::Context as JetStreamContext;
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, info, warn};

use agent_protocol::AgentMessage;
use crate::db::repositories::{
    AgentEventRepository, AgentRepository, TaskDeliveryRepository, TaskRepository,
};
use crate::domain::{AckKind, AgentEventType, AgentStatus, DeliveryStatus, NewAgentEvent, TaskStatus};
use crate::messaging::streams::{AGENT_EVENTS_STREAM, AGENT_EVENTS_SUBJECT};

pub struct EventSubscriber;

impl EventSubscriber {
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
        match msg {
            AgentMessage::TaskStarted {
                agent_id,
                task_id,
                idempotency_key,
                timestamp: _,
            } => {
                info!(%agent_id, %task_id, %idempotency_key, "Agent reported TaskStarted");

                // 1. Record event
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
            }

            AgentMessage::ProgressUpdate {
                agent_id,
                task_id,
                message,
                percent,
                timestamp: _,
            } => {
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
            }

            AgentMessage::Blocked {
                agent_id,
                task_id,
                reason,
                blocking_task_id,
                timestamp: _,
            } => {
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
            }

            AgentMessage::Completed {
                agent_id,
                task_id,
                summary,
                timestamp: _,
            } => {
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

                // 3. Free agent -> Idle
                AgentRepository::set_current_task(pool, agent_id, None, AgentStatus::Idle).await?;
            }

            AgentMessage::Failed {
                agent_id,
                task_id,
                error,
                timestamp: _,
            } => {
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

                // 3. Agent to Error
                AgentRepository::set_current_task(pool, agent_id, None, AgentStatus::Error).await?;
            }

            AgentMessage::Heartbeat {
                agent_id,
                status,
                current_task_id: _,
                timestamp: _,
            } => {
                AgentRepository::record_heartbeat(pool, agent_id).await?;
                let _ = AgentRepository::update_status(pool, agent_id, match status {
                    agent_protocol::AgentStatus::Offline => AgentStatus::Offline,
                    agent_protocol::AgentStatus::Idle => AgentStatus::Idle,
                    agent_protocol::AgentStatus::Busy => AgentStatus::Busy,
                    agent_protocol::AgentStatus::Blocked => AgentStatus::Blocked,
                    agent_protocol::AgentStatus::Error => AgentStatus::Error,
                }).await?;
            }

            AgentMessage::Register { .. } => {
                // Registration is handled on coordinator.agents.register request-reply
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::projects::ProjectRepository;
    use crate::db::repositories::proposals::ProposalRepository;
    use crate::domain::{AdapterType, NewAgent, NewProject, NewProposal, NewTask};

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
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
        TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
        TaskRepository::assign_agent(&pool, task.id, Some(agent.id)).await.unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

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

        let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Executing);

        let a = AgentRepository::find_by_id(&pool, agent.id).await.unwrap().unwrap();
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

        let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Blocked);

        // Resume to Executing
        TaskRepository::update_status(&pool, task.id, TaskStatus::Executing).await.unwrap();

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

        let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(t.status, TaskStatus::Completed);

        let a = AgentRepository::find_by_id(&pool, agent.id).await.unwrap().unwrap();
        assert_eq!(a.status, AgentStatus::Idle);
        assert!(a.current_task_id.is_none());

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
        AgentRepository::delete(&pool, agent.id).await.unwrap();
    }
}
