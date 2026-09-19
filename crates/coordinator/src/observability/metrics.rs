//! Aggregated System Execution Metrics and Statistics.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::domain::{AgentStatus, DeliveryStatus, HealthStatus, TaskStatus};

/// Real-time health, throughput, and operational metrics snapshot for the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemMetrics {
    pub total_tasks: usize,
    pub tasks_proposed: usize,
    pub tasks_approved: usize,
    pub tasks_executing: usize,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub tasks_blocked: usize,

    pub total_agents: usize,
    pub agents_idle: usize,
    pub agents_busy: usize,
    pub agents_offline: usize,
    pub agents_healthy: usize,
    pub agents_degraded: usize,
    pub agents_unhealthy: usize,

    pub total_deliveries: usize,
    pub successful_deliveries: usize,
    pub delivery_success_rate_percent: f64,

    pub total_conflicts: usize,
    pub total_audit_alerts: usize,
}

pub struct MetricsCollector;

impl MetricsCollector {
    /// Collects system-wide execution metrics across tasks, agents, deliveries, and audit tables.
    pub async fn collect(pool: &PgPool) -> Result<SystemMetrics> {
        let mut metrics = SystemMetrics::default();

        // 1. Task metrics
        let task_rows = sqlx::query!(
            r#"
            SELECT status AS "status: TaskStatus", count(*) as count
            FROM tasks
            GROUP BY status
            "#
        )
        .fetch_all(pool)
        .await?;

        for r in task_rows {
            let cnt = r.count.unwrap_or(0) as usize;
            metrics.total_tasks += cnt;
            match r.status {
                TaskStatus::Proposed => metrics.tasks_proposed += cnt,
                TaskStatus::HumanReview => metrics.tasks_proposed += cnt,
                TaskStatus::Approved => metrics.tasks_approved += cnt,
                TaskStatus::Assigned => metrics.tasks_executing += cnt,
                TaskStatus::Executing => metrics.tasks_executing += cnt,
                TaskStatus::Completed => metrics.tasks_completed += cnt,
                TaskStatus::Failed => metrics.tasks_failed += cnt,
                TaskStatus::Blocked => metrics.tasks_blocked += cnt,
                TaskStatus::Rejected | TaskStatus::Cancelled => {}
            }
        }

        // 2. Agent metrics
        let agent_rows = sqlx::query!(
            r#"
            SELECT status AS "status: AgentStatus", health_status AS "health_status: HealthStatus", count(*) as count
            FROM agents
            GROUP BY status, health_status
            "#
        )
        .fetch_all(pool)
        .await?;

        for r in agent_rows {
            let cnt = r.count.unwrap_or(0) as usize;
            metrics.total_agents += cnt;
            match r.status {
                AgentStatus::Idle => metrics.agents_idle += cnt,
                AgentStatus::Busy => metrics.agents_busy += cnt,
                AgentStatus::Offline => metrics.agents_offline += cnt,
                AgentStatus::Blocked => metrics.agents_busy += cnt,
                AgentStatus::Error => metrics.agents_offline += cnt,
            }

            match r.health_status {
                HealthStatus::Healthy => metrics.agents_healthy += cnt,
                HealthStatus::Degraded => metrics.agents_degraded += cnt,
                HealthStatus::Unhealthy => metrics.agents_unhealthy += cnt,
                HealthStatus::Offline => {}
            }
        }

        // 3. Delivery metrics
        let delivery_rows = sqlx::query!(
            r#"
            SELECT status AS "status: DeliveryStatus", count(*) as count
            FROM task_deliveries
            GROUP BY status
            "#
        )
        .fetch_all(pool)
        .await?;

        for r in delivery_rows {
            let cnt = r.count.unwrap_or(0) as usize;
            metrics.total_deliveries += cnt;
            if r.status == DeliveryStatus::Acknowledged {
                metrics.successful_deliveries += cnt;
            }
        }

        metrics.delivery_success_rate_percent = if metrics.total_deliveries > 0 {
            (metrics.successful_deliveries as f64 / metrics.total_deliveries as f64) * 100.0
        } else {
            100.0
        };

        // 4. Conflicts
        let conflict_count = sqlx::query!("SELECT count(*) as count FROM git_conflicts")
            .fetch_one(pool)
            .await?
            .count
            .unwrap_or(0) as usize;
        metrics.total_conflicts = conflict_count;

        // 5. Audit Alerts
        let alert_count = sqlx::query!(
            "SELECT count(*) as count FROM audit_logs WHERE status IN ('denied', 'failure')"
        )
        .fetch_one(pool)
        .await?
        .count
        .unwrap_or(0) as usize;
        metrics.total_audit_alerts = alert_count;

        Ok(metrics)
    }
}
