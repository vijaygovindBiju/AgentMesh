//! Stale Task Sweeper and Reclamation Service.

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::repositories::{AgentRepository, TaskDeliveryRepository, TaskRepository};
use crate::domain::{AckKind, DeliveryStatus, TaskStatus};
use crate::messaging::HeartbeatMonitor;
use crate::observability::CoordinatorEventRepository;

/// Result summary of a stale task sweep cycle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SweepResult {
    pub agents_timed_out: Vec<Uuid>,
    pub tasks_reclaimed: Vec<Uuid>,
    pub deliveries_expired: Vec<Uuid>,
}

pub struct StaleTaskSweeper;

impl StaleTaskSweeper {
    /// Executes a single sweep pass across agents, tasks, and task deliveries.
    pub async fn sweep(pool: &PgPool, heartbeat_timeout: Duration) -> Result<SweepResult> {
        let mut result = SweepResult::default();

        // 1. Mark unresponsive agents as Offline
        let timed_out_agents = HeartbeatMonitor::check_timeouts(
            pool,
            std::time::Duration::from_millis(heartbeat_timeout.num_milliseconds().max(1000) as u64),
        )
        .await?;
        result.agents_timed_out = timed_out_agents;

        // 2. Scan for stuck / orphan tasks assigned to Offline agents or with stale heartbeats
        let threshold = Utc::now() - heartbeat_timeout;
        let stale_tasks = sqlx::query!(
            r#"
            SELECT t.id as "id!", t.short_id as "short_id!", t.assigned_agent_id as "assigned_agent_id!"
            FROM tasks t
            JOIN agents a ON t.assigned_agent_id = a.id
            WHERE t.status IN ('assigned', 'executing')
              AND (
                    a.status = 'offline'
                 OR a.last_seen IS NULL
                 OR a.last_seen < $1
              )
            "#,
            threshold
        )
        .fetch_all(pool)
        .await
        .context("Failed to query stale active tasks")?;

        for row in stale_tasks {
            let task_id = row.id;
            let agent_id = row.assigned_agent_id;
            let short_id = row.short_id;

            warn!(
                %task_id,
                %short_id,
                %agent_id,
                "Reclaiming stale task from unresponsive/offline agent"
            );

            // Fetch delivery attempts to check retry threshold
            let deliveries = TaskDeliveryRepository::list_by_task(pool, task_id).await.unwrap_or_default();
            let attempts = deliveries.len();

            let target_status = if attempts >= 3 {
                TaskStatus::HumanReview
            } else {
                TaskStatus::Approved
            };

            // Unassign agent and revert task status
            TaskRepository::assign_agent(pool, task_id, None).await?;
            TaskRepository::update_status(pool, task_id, target_status).await?;

            // Increment consecutive failures for the failing agent
            let _ = AgentRepository::record_task_failure(
                pool,
                agent_id,
                &format!("Task {} reclaimed due to heartbeat timeout", short_id),
            )
            .await;

            // Record coordinator observability event
            let _ = CoordinatorEventRepository::record(
                pool,
                "task.reclaimed",
                None,
                Some(task_id),
                Some(agent_id),
                format!(
                    "Task {short_id} reclaimed from agent {agent_id}. Reverted to status {:?}",
                    target_status
                ),
                serde_json::json!({
                    "task_id": task_id,
                    "agent_id": agent_id,
                    "attempts": attempts,
                    "new_status": target_status,
                }),
            )
            .await;

            result.tasks_reclaimed.push(task_id);
        }

        // 3. Scan for expired deliveries in 'delivered' state without ACK
        let expired_deliveries = sqlx::query_as!(
            crate::domain::TaskDelivery,
            r#"
            SELECT id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                   idempotency_key, delivered_at, acknowledged_at, ack_kind AS "ack_kind: AckKind",
                   expires_at, status AS "status: DeliveryStatus", failure_reason, reassigned_to
            FROM task_deliveries
            WHERE status = 'delivered'
              AND expires_at < NOW()
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to query expired deliveries")?;

        for del in expired_deliveries {
            warn!(delivery_id = %del.id, task_id = %del.task_id, "Expiring unacknowledged delivery");

            // Mark delivery Terminal
            let _ = TaskDeliveryRepository::record_ack(
                pool,
                del.id,
                AckKind::Term,
                DeliveryStatus::Terminal,
            )
            .await;

            // Free task back to Approved
            let _ = TaskRepository::assign_agent(pool, del.task_id, None).await;
            let _ = TaskRepository::update_status(pool, del.task_id, TaskStatus::Approved).await;

            let _ = CoordinatorEventRepository::record(
                pool,
                "delivery.expired",
                None,
                Some(del.task_id),
                Some(del.agent_id),
                format!("Delivery attempt #{} expired before agent ACK", del.attempt),
                serde_json::json!({
                    "delivery_id": del.id,
                    "attempt": del.attempt,
                    "expires_at": del.expires_at,
                }),
            )
            .await;

            result.deliveries_expired.push(del.id);
        }

        if !result.tasks_reclaimed.is_empty() || !result.deliveries_expired.is_empty() {
            info!(
                reclaimed = result.tasks_reclaimed.len(),
                expired = result.deliveries_expired.len(),
                "Stale sweep cycle complete"
            );
        }

        Ok(result)
    }
}
