use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use futures::StreamExt;
use tracing::{error, info};
use uuid::Uuid;

use agent_agy::{AgyAgent, AgyAgentRunner};
use agent_protocol::CoordinatorMessage;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());
    let owner = std::env::var("AGY_AGENT_OWNER").unwrap_or_else(|_| "AgyDev".to_string());
    let api_key = std::env::var("AGY_AGENT_API_KEY").unwrap_or_else(|_| "agentmesh_agy_key".to_string());

    let mut agent = AgyAgent::new(owner, api_key);

    if let Ok(id_str) = std::env::var("AGY_AGENT_ID") {
        if let Ok(parsed) = Uuid::parse_str(&id_str) {
            agent = agent.with_id(parsed);
        }
    }

    if let Ok(bin_path) = std::env::var("AGY_BIN_PATH") {
        agent = agent.with_agy_path(PathBuf::from(bin_path));
    }

    if let Ok(model) = std::env::var("AGY_MODEL") {
        agent = agent.with_model(model);
    }

    if let Ok(effort) = std::env::var("AGY_EFFORT") {
        agent = agent.with_effort(effort);
    }

    if let Ok(workspace_dir) = std::env::var("AGY_WORKSPACE_DIR") {
        agent = agent.with_workspace_dir(PathBuf::from(workspace_dir));
    }

    if let Ok(timeout_secs) = std::env::var("AGY_TIMEOUT_SECS") {
        if let Ok(secs) = timeout_secs.parse::<u64>() {
            agent = agent.with_timeout(Duration::from_secs(secs));
        }
    }

    info!(
        agent_id = %agent.id,
        owner = %agent.human_owner,
        agy_bin = %agent.agy_path.display(),
        model = ?agent.model,
        effort = ?agent.effort,
        "Starting AgentMesh agy Adapter"
    );

    let (client, jetstream) = AgyAgentRunner::connect(&nats_url, Some(&nats_token)).await?;

    // 1. Register with coordinator
    let event_subject = AgyAgentRunner::register(&client, &agent).await?;

    // 2. Spawn background heartbeat loop
    let hb_client = client.clone();
    let hb_agent_id = agent.id;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(e) = AgyAgentRunner::send_heartbeat(
                &hb_client,
                hb_agent_id,
                agent_protocol::AgentStatus::Idle,
                None,
            )
            .await
            {
                error!(error = %e, "Failed to send agy heartbeat");
            }
        }
    });

    // 3. Create JetStream consumer for task assignments
    let consumer = AgyAgentRunner::create_task_consumer(&jetstream, agent.id).await?;
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
                        info!(
                            task_id = %spec.task_id,
                            short_id = %spec.short_id,
                            "Received task assignment"
                        );
                        if let Err(e) = AgyAgentRunner::execute_task(
                            &client,
                            &agent,
                            &spec,
                            &event_subject,
                        )
                        .await
                        {
                            error!(error = %e, "agy task execution failed");
                        }
                    }
                    Ok(other) => {
                        info!(message = ?other, "Received non-assignment coordinator message");
                    }
                    Err(e) => {
                        error!(error = %e, "Malformed coordinator message payload");
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
