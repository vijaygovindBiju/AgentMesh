use std::time::Duration;
use anyhow::{Context, Result};
use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::Context as JetStreamContext;
use async_nats::Client;
use chrono::Utc;
use tracing::{error, info, warn};
use uuid::Uuid;

use agent_protocol::{
    AgentAdapter, AgentMessage, AgentStatus, CoordinatorMessage, TaskSpec,
};
use crate::adapter::MockAgent;

pub struct MockAgentRunner;

impl MockAgentRunner {
    /// Connects to the NATS broker.
    pub async fn connect(nats_url: &str, token: Option<&str>) -> Result<(Client, JetStreamContext)> {
        let mut options = async_nats::ConnectOptions::new();
        if let Some(tok) = token {
            if !tok.is_empty() {
                options = options.token(tok.to_string());
            }
        }

        let client = options
            .connect(nats_url)
            .await
            .with_context(|| format!("Failed to connect to NATS at {nats_url}"))?;

        let jetstream = async_nats::jetstream::new(client.clone());
        Ok((client, jetstream))
    }

    /// Performs the registration handshake on `coordinator.agents.register`.
    pub async fn register(client: &Client, agent: &MockAgent) -> Result<String> {
        let reg_msg = AgentMessage::Register {
            agent_id: agent.agent_id(),
            human_owner: agent.human_owner().to_string(),
            adapter_type: agent.adapter_type().to_string(),
            capabilities: agent.capabilities().to_vec(),
            api_key: agent.api_key().to_string(),
        };

        let payload = serde_json::to_vec(&reg_msg)?;
        let reply = client
            .request("coordinator.agents.register".to_string(), payload.into())
            .await
            .context("Registration request failed")?;

        let response: CoordinatorMessage = serde_json::from_slice(&reply.payload)
            .context("Failed to parse registration response")?;

        match response {
            CoordinatorMessage::RegisterResponse {
                status,
                nats_subject,
                error,
            } => {
                if status == "ok" {
                    let subject = nats_subject
                        .unwrap_or_else(|| format!("agents.{}.events", agent.agent_id()));
                    info!(agent_id = %agent.agent_id(), %subject, "Agent successfully registered");
                    Ok(subject)
                } else {
                    let err_msg = error.unwrap_or_else(|| "Unknown registration error".to_string());
                    anyhow::bail!("Registration rejected: {err_msg}");
                }
            }
            other => anyhow::bail!("Unexpected response to registration: {other:?}"),
        }
    }

    /// Sends a periodic heartbeat to `coordinator.agents.heartbeat.{agent_id}`.
    pub async fn send_heartbeat(
        client: &Client,
        agent_id: Uuid,
        status: AgentStatus,
        current_task_id: Option<Uuid>,
    ) -> Result<()> {
        let hb = AgentMessage::Heartbeat {
            agent_id,
            status,
            current_task_id,
            timestamp: Utc::now(),
        };
        let payload = serde_json::to_vec(&hb)?;
        let subject = format!("coordinator.agents.heartbeat.{agent_id}");
        client.publish(subject, payload.into()).await?;
        Ok(())
    }

    /// Helper to publish an AgentMessage event.
    pub async fn publish_event(
        client: &Client,
        event_subject: &str,
        msg: &AgentMessage,
    ) -> Result<()> {
        let payload = serde_json::to_vec(msg)?;
        client.publish(event_subject.to_string(), payload.into()).await?;
        Ok(())
    }

    /// Runs the full task execution lifecycle: Started -> Progress -> (Blocked) -> Completed/Failed.
    pub async fn execute_task(
        client: &Client,
        agent: &MockAgent,
        spec: &TaskSpec,
        event_subject: &str,
    ) -> Result<()> {
        info!(task_id = %spec.task_id, short_id = %spec.short_id, "Beginning simulated task execution");

        // 1. Report TaskStarted
        Self::publish_event(
            client,
            event_subject,
            &AgentMessage::TaskStarted {
                agent_id: agent.agent_id(),
                task_id: spec.task_id,
                idempotency_key: spec.idempotency_key.clone(),
                timestamp: Utc::now(),
            },
        )
        .await?;

        tokio::time::sleep(agent.task_delay / 2).await;

        // 2. Report ProgressUpdate
        Self::publish_event(
            client,
            event_subject,
            &AgentMessage::ProgressUpdate {
                agent_id: agent.agent_id(),
                task_id: spec.task_id,
                message: format!("Processing task {}", spec.short_id),
                percent: 50,
                timestamp: Utc::now(),
            },
        )
        .await?;

        // 3. Optional simulated Blocked state
        if let Some(blocker_id) = agent.simulate_blocker {
            warn!(task_id = %spec.task_id, %blocker_id, "Simulating blocked state");
            Self::publish_event(
                client,
                event_subject,
                &AgentMessage::Blocked {
                    agent_id: agent.agent_id(),
                    task_id: spec.task_id,
                    reason: "Waiting for prerequisite task".to_string(),
                    blocking_task_id: Some(blocker_id),
                    timestamp: Utc::now(),
                },
            )
            .await?;

            tokio::time::sleep(agent.task_delay / 2).await;
        }

        // 4. Report Final State: Failed or Completed
        if agent.simulate_failure {
            error!(task_id = %spec.task_id, "Simulating task failure");
            Self::publish_event(
                client,
                event_subject,
                &AgentMessage::Failed {
                    agent_id: agent.agent_id(),
                    task_id: spec.task_id,
                    error: "Simulated failure during mock execution".to_string(),
                    timestamp: Utc::now(),
                },
            )
            .await?;
        } else {
            tokio::time::sleep(agent.task_delay / 2).await;
            info!(task_id = %spec.task_id, "Simulating task completion");
            Self::publish_event(
                client,
                event_subject,
                &AgentMessage::Completed {
                    agent_id: agent.agent_id(),
                    task_id: spec.task_id,
                    summary: format!("Task {} completed successfully by mock agent", spec.short_id),
                    timestamp: Utc::now(),
                },
            )
            .await?;
        }

        Ok(())
    }

    /// Sets up a consumer for this agent on the TASK_ASSIGNMENTS stream and listens for assignments.
    pub async fn create_task_consumer(
        jetstream: &JetStreamContext,
        agent_id: Uuid,
    ) -> Result<PullConsumer> {
        let stream = jetstream
            .get_stream("TASK_ASSIGNMENTS")
            .await
            .context("TASK_ASSIGNMENTS stream not found")?;

        let subject_filter = format!("coordinator.tasks.assign.{agent_id}");
        let consumer_name = format!("agent-{}", agent_id.simple());

        let consumer = stream
            .get_or_create_consumer(
                &consumer_name,
                async_nats::jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.clone()),
                    filter_subject: subject_filter,
                    ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                    max_deliver: 5,
                    ack_wait: Duration::from_secs(60),
                    ..Default::default()
                },
            )
            .await
            .context("Failed to create agent task consumer")?;

        Ok(consumer)
    }
}
