use std::time::Duration;
use anyhow::{Context, Result};
use async_nats::jetstream::stream::{Config, RetentionPolicy, StorageType};
use async_nats::jetstream::Context as JetStreamContext;

pub const TASK_ASSIGNMENTS_STREAM: &str = "TASK_ASSIGNMENTS";
pub const TASK_ASSIGNMENTS_SUBJECT: &str = "coordinator.tasks.assign.*";

pub const AGENT_EVENTS_STREAM: &str = "AGENT_EVENTS";
pub const AGENT_EVENTS_SUBJECT: &str = "agents.*.events";

/// Ensures that both JetStream streams (TASK_ASSIGNMENTS and AGENT_EVENTS) exist with the correct configurations.
pub async fn ensure_streams(jetstream: &JetStreamContext) -> Result<()> {
    // 1. TASK_ASSIGNMENTS: WorkQueue retention
    let task_config = Config {
        name: TASK_ASSIGNMENTS_STREAM.to_string(),
        subjects: vec![TASK_ASSIGNMENTS_SUBJECT.to_string()],
        retention: RetentionPolicy::WorkQueue,
        storage: StorageType::File,
        ..Default::default()
    };

    jetstream
        .get_or_create_stream(task_config)
        .await
        .with_context(|| format!("Failed to create/get JetStream stream {TASK_ASSIGNMENTS_STREAM}"))?;

    // 2. AGENT_EVENTS: Limits retention (24h or 100k messages)
    let events_config = Config {
        name: AGENT_EVENTS_STREAM.to_string(),
        subjects: vec![AGENT_EVENTS_SUBJECT.to_string()],
        retention: RetentionPolicy::Limits,
        storage: StorageType::File,
        max_messages: 100_000,
        max_age: Duration::from_secs(24 * 3600),
        ..Default::default()
    };

    jetstream
        .get_or_create_stream(events_config)
        .await
        .with_context(|| format!("Failed to create/get JetStream stream {AGENT_EVENTS_STREAM}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::client::connect;

    #[tokio::test]
    async fn test_ensure_streams() {
        let _ = dotenvy::dotenv();
        let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

        let Ok((_client, jetstream)) = connect(&nats_url, Some(&nats_token)).await else {
            eprintln!("Skipping test: NATS not reachable");
            return;
        };

        let res = ensure_streams(&jetstream).await;
        assert!(res.is_ok(), "ensure_streams failed: {:?}", res.err());

        // Verify stream info
        let task_stream = jetstream.get_stream(TASK_ASSIGNMENTS_STREAM).await;
        assert!(task_stream.is_ok(), "TASK_ASSIGNMENTS stream must exist");

        let events_stream = jetstream.get_stream(AGENT_EVENTS_STREAM).await;
        assert!(events_stream.is_ok(), "AGENT_EVENTS stream must exist");
    }
}
