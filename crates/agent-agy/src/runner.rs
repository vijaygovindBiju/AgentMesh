use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Context, Result};
use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::Context as JetStreamContext;
use async_nats::Client;
use chrono::Utc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use agent_protocol::{
    AgentAdapter, AgentMessage, AgentStatus, CoordinatorMessage, TaskSpec,
};
use crate::adapter::AgyAgent;
use crate::parser::AgyStreamEvent;
use crate::process::{AgyProcess, AgyProcessEvent};

pub struct AgyAgentRunner;

impl AgyAgentRunner {
    /// Connects to the NATS broker with optional authentication token.
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
    pub async fn register(client: &Client, agent: &AgyAgent) -> Result<String> {
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
                    info!(agent_id = %agent.agent_id(), %subject, "agy agent successfully registered");
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

    /// Helper to publish an AgentMessage event to JetStream/NATS.
    pub async fn publish_event(
        client: &Client,
        event_subject: &str,
        msg: &AgentMessage,
    ) -> Result<()> {
        let payload = serde_json::to_vec(msg)?;
        client.publish(event_subject.to_string(), payload.into()).await?;
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
        let consumer_name = format!("agent-agy-{}", agent_id.simple());

        let consumer = stream
            .get_or_create_consumer(
                &consumer_name,
                async_nats::jetstream::consumer::pull::Config {
                    durable_name: Some(consumer_name.clone()),
                    filter_subject: subject_filter,
                    ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                    max_deliver: 5,
                    ack_wait: Duration::from_secs(600), // 10 min ack wait for real agents
                    ..Default::default()
                },
            )
            .await
            .context("Failed to create agy agent task consumer")?;

        Ok(consumer)
    }

    /// Builds the execution prompt for the `agy` CLI from the assigned `TaskSpec`.
    pub fn build_task_prompt(spec: &TaskSpec) -> String {
        let resources_str = if spec.affected_resources.is_empty() {
            "None explicitly specified".to_string()
        } else {
            spec.affected_resources.join(", ")
        };

        format!(
            "TASK IDENTIFIER: [{short_id}]\n\
             TITLE: {title}\n\n\
             DESCRIPTION:\n{description}\n\n\
             AFFECTED RESOURCES:\n{resources_str}\n\n\
             INSTRUCTIONS:\n\
             Execute and implement the requested changes for this task. \
             Ensure all code is written to disk and verified before completion.",
            short_id = spec.short_id,
            title = spec.title,
            description = spec.description,
            resources_str = resources_str
        )
    }

    /// Executes a single assigned task by spawning `agy`, translating stream events,
    /// and reporting lifecycle updates back to the coordinator.
    pub async fn execute_task(
        client: &Client,
        agent: &AgyAgent,
        spec: &TaskSpec,
        event_subject: &str,
    ) -> Result<()> {
        info!(
            task_id = %spec.task_id,
            short_id = %spec.short_id,
            title = %spec.title,
            "Dispatching task assignment to agy subprocess"
        );

        let prompt = Self::build_task_prompt(spec);
        let mut event_rx = AgyProcess::run(agent, &prompt).await?;

        let started_reported = Arc::new(AtomicBool::new(false));
        let mut final_reported = false;
        let mut last_progress_step = 0;

        while let Some(proc_event) = event_rx.recv().await {
            match proc_event {
                AgyProcessEvent::Stream(stream_event) => match stream_event {
                    AgyStreamEvent::Init { conversation_id, .. } => {
                        if !started_reported.swap(true, Ordering::SeqCst) {
                            info!(
                                task_id = %spec.task_id,
                                ?conversation_id,
                                "agy session initialized, reporting TaskStarted"
                            );
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
                        }
                    }

                    AgyStreamEvent::StepUpdate { step_update } => {
                        // Ensure TaskStarted has been sent
                        if !started_reported.swap(true, Ordering::SeqCst) {
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
                        }

                        let step_num = step_update.step_index.unwrap_or(0);
                        let snippet = step_update
                            .text_delta
                            .as_deref()
                            .unwrap_or("Executing step");

                        let percent = std::cmp::min(10 + (step_num * 15), 90) as u8;

                        if step_num > last_progress_step || !snippet.trim().is_empty() {
                            last_progress_step = step_num;
                            let msg_summary = if snippet.len() > 120 {
                                format!("{}...", &snippet[..120])
                            } else {
                                snippet.to_string()
                            };

                            Self::publish_event(
                                client,
                                event_subject,
                                &AgentMessage::ProgressUpdate {
                                    agent_id: agent.agent_id(),
                                    task_id: spec.task_id,
                                    message: format!("Step {step_num}: {msg_summary}"),
                                    percent,
                                    timestamp: Utc::now(),
                                },
                            )
                            .await?;
                        }
                    }

                    AgyStreamEvent::Result { result } => {
                        if result.status == "SUCCESS" {
                            info!(task_id = %spec.task_id, "agy returned SUCCESS result");
                            let summary = result
                                .response
                                .unwrap_or_else(|| "Task completed successfully by agy".to_string());

                            Self::publish_event(
                                client,
                                event_subject,
                                &AgentMessage::Completed {
                                    agent_id: agent.agent_id(),
                                    task_id: spec.task_id,
                                    summary,
                                    timestamp: Utc::now(),
                                },
                            )
                            .await?;
                            final_reported = true;
                        } else {
                            warn!(
                                task_id = %spec.task_id,
                                status = %result.status,
                                "agy returned non-success result"
                            );
                            Self::publish_event(
                                client,
                                event_subject,
                                &AgentMessage::Failed {
                                    agent_id: agent.agent_id(),
                                    task_id: spec.task_id,
                                    error: result
                                        .response
                                        .unwrap_or_else(|| format!("agy failed with status {}", result.status)),
                                    timestamp: Utc::now(),
                                },
                            )
                            .await?;
                            final_reported = true;
                        }
                    }

                    AgyStreamEvent::Unknown => {}
                },

                AgyProcessEvent::Stderr(line) => {
                    debug!(task_id = %spec.task_id, %line, "agy stderr");
                }

                AgyProcessEvent::Completed { exit_code: _, result } => {
                    if !final_reported {
                        let summary = result
                            .and_then(|r| r.response)
                            .unwrap_or_else(|| format!("Task {} completed by agy", spec.short_id));

                        Self::publish_event(
                            client,
                            event_subject,
                            &AgentMessage::Completed {
                                agent_id: agent.agent_id(),
                                task_id: spec.task_id,
                                summary,
                                timestamp: Utc::now(),
                            },
                        )
                        .await?;
                        final_reported = true;
                    }
                }

                AgyProcessEvent::Failed {
                    reason,
                    exit_code,
                    stderr,
                } => {
                    if !final_reported {
                        error!(
                            task_id = %spec.task_id,
                            ?exit_code,
                            %reason,
                            %stderr,
                            "agy task execution failed"
                        );

                        Self::publish_event(
                            client,
                            event_subject,
                            &AgentMessage::Failed {
                                agent_id: agent.agent_id(),
                                task_id: spec.task_id,
                                error: format!("{reason}. Stderr: {stderr}"),
                                timestamp: Utc::now(),
                            },
                        )
                        .await?;
                        final_reported = true;
                    }
                }
            }
        }

        // Fallback: If channel closed without any final event emitted, report Failure
        if !final_reported {
            warn!(task_id = %spec.task_id, "agy process terminated without emitting a final result");
            Self::publish_event(
                client,
                event_subject,
                &AgentMessage::Failed {
                    agent_id: agent.agent_id(),
                    task_id: spec.task_id,
                    error: "Subprocess exited prematurely without completion status".to_string(),
                    timestamp: Utc::now(),
                },
            )
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_build_task_prompt() {
        let spec = TaskSpec {
            task_id: Uuid::new_v4(),
            short_id: "TASK-001".to_string(),
            title: "Implement User Authentication".to_string(),
            description: "Add JWT token validation endpoint".to_string(),
            affected_resources: vec!["src/auth.rs".to_string(), "migrations/002.sql".to_string()],
            depends_on: vec![],
            idempotency_key: "idem-key-1".to_string(),
            assigned_at: Utc::now(),
        };

        let prompt = AgyAgentRunner::build_task_prompt(&spec);
        assert!(prompt.contains("[TASK-001]"));
        assert!(prompt.contains("Implement User Authentication"));
        assert!(prompt.contains("src/auth.rs, migrations/002.sql"));
    }
}
