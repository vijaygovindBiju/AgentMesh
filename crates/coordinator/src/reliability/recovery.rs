//! Coordinator Startup Recovery and Agent Reconnect Reconciliation.

use anyhow::{Context, Result};
use async_nats::jetstream::Context as JetStreamContext;
use chrono::Duration;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::coordinator::assignment::AssignmentService;
use crate::domain::TaskStatus;
use crate::observability::CoordinatorEventRepository;
use crate::reliability::stale_sweeper::{StaleTaskSweeper, SweepResult};

/// Summary report generated after coordinator crash / restart recovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub pending_deliveries_republished: usize,
    pub sweep_summary: SweepResult,
}

pub struct CoordinatorRecoveryService;

impl CoordinatorRecoveryService {
    /// Executes comprehensive crash recovery on coordinator restart.
    ///
    /// Scans the database for:
    /// 1. Unconfirmed 'Pending' deliveries created before crash -> republishes to NATS.
    /// 2. Active tasks assigned to agents that crashed while coordinator was down -> reclaims them.
    /// 3. Expired deliveries awaiting ACK -> terminates and frees tasks.
    pub async fn recover_on_startup(
        pool: &PgPool,
        jetstream: Option<&JetStreamContext>,
    ) -> Result<RecoveryReport> {
        info!("Executing coordinator startup crash-recovery scan...");

        // 1. Reconcile stuck pending deliveries
        let republished = AssignmentService::reconcile_pending_deliveries(pool, jetstream).await?;

        // 2. Run stale task sweeper for tasks orphaned during downtime (30s threshold)
        let sweep_result = StaleTaskSweeper::sweep(pool, Duration::seconds(30)).await?;

        let report = RecoveryReport {
            pending_deliveries_republished: republished,
            sweep_summary: sweep_result,
        };

        // 3. Record coordinator startup recovery event
        let _ = CoordinatorEventRepository::record(
            pool,
            "coordinator.recovery_completed",
            None,
            None,
            None,
            format!(
                "Startup recovery finished: {} pending republished, {} tasks reclaimed",
                report.pending_deliveries_republished,
                report.sweep_summary.tasks_reclaimed.len()
            ),
            serde_json::to_value(&report).unwrap_or_default(),
        )
        .await;

        info!(
            republished = report.pending_deliveries_republished,
            reclaimed = report.sweep_summary.tasks_reclaimed.len(),
            "Coordinator startup recovery completed successfully"
        );

        Ok(report)
    }

    /// Checks if a reconnecting agent has an unfinished active task that needs resumption.
    /// Returns `Some(task_id)` if there is an active assigned task, allowing task re-delivery.
    pub async fn handle_agent_reconnect(pool: &PgPool, agent_id: Uuid) -> Result<Option<Uuid>> {
        let active_task = sqlx::query!(
            r#"
            SELECT id, short_id, status as "status: TaskStatus"
            FROM tasks
            WHERE assigned_agent_id = $1
              AND status IN ('assigned', 'executing')
            LIMIT 1
            "#,
            agent_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query active task for reconnecting agent")?;

        if let Some(task) = active_task {
            info!(
                %agent_id,
                task_id = %task.id,
                short_id = %task.short_id,
                "Agent reconnected with active in-progress task"
            );
            Ok(Some(task.id))
        } else {
            Ok(None)
        }
    }
}
