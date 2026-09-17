use std::time::Duration;
use anyhow::{Context, Result};
use async_nats::Client;
use chrono::Utc;
use futures::StreamExt;
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use agent_protocol::AgentMessage;
use crate::db::repositories::AgentRepository;

pub const HEARTBEAT_SUBJECT: &str = "coordinator.agents.heartbeat.*";

pub struct HeartbeatMonitor;

impl HeartbeatMonitor {
    /// Checks for agents that have not sent a heartbeat within `timeout` and marks them Offline.
    /// Returns the list of agent IDs that transitioned to Offline.
    pub async fn check_timeouts(pool: &PgPool, timeout: Duration) -> Result<Vec<Uuid>> {
        let threshold = Utc::now() - chrono::Duration::from_std(timeout).unwrap_or(chrono::Duration::seconds(30));

        let rows = sqlx::query!(
            r#"
            UPDATE agents
            SET status = 'offline'
            WHERE status NOT IN ('offline')
              AND last_seen IS NOT NULL
              AND last_seen < $1
            RETURNING id
            "#,
            threshold
        )
        .fetch_all(pool)
        .await
        .context("Failed to mark timed-out agents as offline")?;

        let marked_ids: Vec<Uuid> = rows.into_iter().map(|r| r.id).collect();
        if !marked_ids.is_empty() {
            warn!(count = marked_ids.len(), ?marked_ids, "Agents marked offline due to heartbeat timeout");
        }

        Ok(marked_ids)
    }

    /// Listens for heartbeat messages on `coordinator.agents.heartbeat.*` and updates `last_seen`.
    pub async fn start_listener(client: Client, pool: PgPool) -> Result<()> {
        let mut subscriber = client
            .subscribe(HEARTBEAT_SUBJECT.to_string())
            .await
            .with_context(|| format!("Failed to subscribe to {HEARTBEAT_SUBJECT}"))?;

        info!("Heartbeat listener started on {HEARTBEAT_SUBJECT}");

        while let Some(msg) = subscriber.next().await {
            if let Ok(AgentMessage::Heartbeat {
                agent_id,
                status: _,
                current_task_id: _,
                timestamp: _,
            }) = serde_json::from_slice::<AgentMessage>(&msg.payload)
            {
                if let Err(e) = AgentRepository::record_heartbeat(&pool, agent_id).await {
                    error!(%agent_id, error = %e, "Failed to record agent heartbeat");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::domain::{AdapterType, AgentStatus, NewAgent};

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_heartbeat_timeout_detection() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // Create an agent with an old last_seen
        let agent = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "TimeoutTester".to_string(),
                api_key_hash: "hash".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec![],
                nats_subject: "agents.timeout.events".to_string(),
            },
        )
        .await
        .unwrap();

        // Mark agent as Idle
        AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle).await.unwrap();

        // Artificially age the agent's last_seen
        let old_time = Utc::now() - chrono::Duration::seconds(60);
        sqlx::query!(
            "UPDATE agents SET last_seen = $1 WHERE id = $2",
            old_time,
            agent.id
        )
        .execute(&pool)
        .await
        .unwrap();

        // Run check_timeouts with a 10s timeout
        let timed_out = HeartbeatMonitor::check_timeouts(&pool, Duration::from_secs(10))
            .await
            .unwrap();

        assert!(timed_out.contains(&agent.id));

        let updated_agent = AgentRepository::find_by_id(&pool, agent.id).await.unwrap().unwrap();
        assert_eq!(updated_agent.status, AgentStatus::Offline);

        // Clean up
        AgentRepository::delete(&pool, agent.id).await.unwrap();
    }
}
