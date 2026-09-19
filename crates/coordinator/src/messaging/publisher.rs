use anyhow::{Context, Result};
use async_nats::jetstream::Context as JetStreamContext;
use chrono::Utc;
use uuid::Uuid;

use agent_protocol::{CoordinatorMessage, TaskSpec};

pub struct TaskPublisher;

impl TaskPublisher {
    /// Publishes a `TaskAssignment` to an agent's assigned subject in JetStream.
    /// Returns the JetStream sequence number of the published message.
    pub async fn publish_assignment(
        jetstream: &JetStreamContext,
        agent_id: Uuid,
        spec: &TaskSpec,
    ) -> Result<u64> {
        let subject = format!("coordinator.tasks.assign.{agent_id}");
        let message = CoordinatorMessage::TaskAssignment { spec: spec.clone() };
        let payload = serde_json::to_vec(&message)
            .context("Failed to serialize CoordinatorMessage::TaskAssignment")?;

        let ack = jetstream
            .publish(subject.clone(), payload.into())
            .await
            .with_context(|| format!("Failed to publish task assignment to {subject}"))?
            .await
            .with_context(|| format!("Failed to get publish ack for {subject}"))?;

        Ok(ack.sequence)
    }

    /// Publishes a `TaskCancelled` notice to an agent.
    pub async fn publish_cancellation(
        jetstream: &JetStreamContext,
        agent_id: Uuid,
        task_id: Uuid,
        reason: &str,
    ) -> Result<u64> {
        let subject = format!("coordinator.tasks.assign.{agent_id}");
        let message = CoordinatorMessage::TaskCancelled {
            task_id,
            reason: reason.to_string(),
            timestamp: Utc::now(),
        };
        let payload = serde_json::to_vec(&message)
            .context("Failed to serialize CoordinatorMessage::TaskCancelled")?;

        let ack = jetstream
            .publish(subject.clone(), payload.into())
            .await
            .with_context(|| format!("Failed to publish task cancellation to {subject}"))?
            .await
            .with_context(|| format!("Failed to get publish ack for {subject}"))?;

        Ok(ack.sequence)
    }

    /// Publishes a `WaitForDependency` notice to an agent.
    pub async fn publish_wait_for_dependency(
        jetstream: &JetStreamContext,
        agent_id: Uuid,
        task_id: Uuid,
        blocking_task_id: Uuid,
        message: &str,
    ) -> Result<u64> {
        let subject = format!("coordinator.tasks.assign.{agent_id}");
        let message = CoordinatorMessage::WaitForDependency {
            task_id,
            blocking_task_id,
            message: message.to_string(),
            timestamp: Utc::now(),
        };
        let payload = serde_json::to_vec(&message)
            .context("Failed to serialize CoordinatorMessage::WaitForDependency")?;

        let ack = jetstream
            .publish(subject.clone(), payload.into())
            .await
            .with_context(|| format!("Failed to publish wait for dependency to {subject}"))?
            .await
            .with_context(|| format!("Failed to get publish ack for {subject}"))?;

        Ok(ack.sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::client::connect;
    use crate::messaging::streams::ensure_streams;

    #[tokio::test]
    async fn test_publish_task_assignment() {
        let _ = dotenvy::dotenv();
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_token =
            std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

        let Ok((_client, jetstream)) = connect(&nats_url, Some(&nats_token)).await else {
            eprintln!("Skipping test: NATS not reachable");
            return;
        };

        ensure_streams(&jetstream).await.unwrap();

        let agent_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let spec = TaskSpec::new(
            task_id,
            "PUB-001",
            "Publisher test",
            "Testing publish_assignment",
            vec!["src/main.rs".to_string()],
            vec![],
            format!("{}:1", task_id),
        );

        let seq = TaskPublisher::publish_assignment(&jetstream, agent_id, &spec)
            .await
            .expect("Should publish assignment");

        assert!(seq > 0, "Sequence must be positive");
    }
}
