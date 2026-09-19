use std::time::Duration;
use anyhow::Result;
use futures::StreamExt;
use tracing::{error, info};
use uuid::Uuid;

use agent_mock::{MockAgent, MockAgentRunner};
use agent_protocol::CoordinatorMessage;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());
    let owner = std::env::var("MOCK_AGENT_OWNER").unwrap_or_else(|_| "MockDev".to_string());
    let api_key = std::env::var("MOCK_AGENT_API_KEY").unwrap_or_else(|_| "agentmesh_mock_key".to_string());
    let delay_ms: u64 = std::env::var("MOCK_TASK_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);

    let mut agent = MockAgent::new(owner, api_key).with_delay(Duration::from_millis(delay_ms));

    if let Ok(id_str) = std::env::var("MOCK_AGENT_ID") {
        if let Ok(parsed) = Uuid::parse_str(&id_str) {
            agent.id = parsed;
        }
    }

    info!(agent_id = %agent.id, owner = %agent.human_owner, "Starting Mock Agent");

    let (client, jetstream) = MockAgentRunner::connect(&nats_url, Some(&nats_token)).await?;

    // 1. Register with coordinator
    let event_subject = MockAgentRunner::register(&client, &agent).await?;

    // 2. Spawn heartbeat task
    let hb_client = client.clone();
    let hb_agent_id = agent.id;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(e) = MockAgentRunner::send_heartbeat(
                &hb_client,
                hb_agent_id,
                agent_protocol::AgentStatus::Idle,
                None,
            )
            .await
            {
                error!(error = %e, "Failed to send heartbeat");
            }
        }
    });

    // 3. Create JetStream consumer for task assignments
    let consumer = MockAgentRunner::create_task_consumer(&jetstream, agent.id).await?;
    info!(agent_id = %agent.id, "Listening for task assignments on JetStream");

    let mut messages = consumer.messages().await?;
    while let Some(msg_result) = messages.next().await {
        match msg_result {
            Ok(msg) => {
                // Transport ACK
                if let Err(e) = msg.ack().await {
                    error!(error = %e, "Failed to ACK JetStream task assignment");
                    continue;
                }

                match serde_json::from_slice::<CoordinatorMessage>(&msg.payload) {
                    Ok(CoordinatorMessage::TaskAssignment { spec }) => {
                        info!(task_id = %spec.task_id, short_id = %spec.short_id, "Received task assignment");
                        if let Err(e) = MockAgentRunner::execute_task(
                            &client,
                            &agent,
                            &spec,
                            &event_subject,
                        )
                        .await
                        {
                            error!(error = %e, "Task execution failed");
                        }
                    }
                    Ok(CoordinatorMessage::TaskCancelled { task_id, reason, .. }) => {
                        tracing::warn!(%task_id, %reason, "Received TaskCancelled instruction from coordinator");
                    }
                    Ok(CoordinatorMessage::WaitForDependency { task_id, blocking_task_id, message, .. }) => {
                        tracing::info!(%task_id, %blocking_task_id, %message, "Received WaitForDependency notice from coordinator; pausing for blocker");
                    }
                    Ok(other) => {
                        info!(message = ?other, "Received non-assignment coordinator message");
                    }
                    Err(e) => {
                        error!(error = %e, "Malformed coordinator message");
                    }
                }
            }
            Err(e) => {
                error!(error = %e, "Error receiving message from JetStream");
            }
        }
    }

    Ok(())
}
