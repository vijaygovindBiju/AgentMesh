//! Failure Diagnostics and Remediation Guidance.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{
    AgentEventRepository, AgentRepository, GitConflictRepository, TaskDeliveryRepository,
    TaskRepository, UnexpectedResourceRepository,
};
use crate::domain::HealthStatus;

/// An actionable remediation step suggested to resolve a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationAdvice {
    pub title: String,
    pub description: String,
    pub suggested_action: String,
}

/// Comprehensive failure diagnostic report for a failed or degraded task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDiagnostics {
    pub task_id: Uuid,
    pub short_id: String,
    pub task_title: String,
    pub failed_at: DateTime<Utc>,
    pub error_message: String,
    pub assigned_agent_id: Option<Uuid>,
    pub agent_owner: Option<String>,
    pub agent_consecutive_failures: i32,
    pub agent_health_status: Option<HealthStatus>,
    pub delivery_attempts: usize,
    pub has_git_conflicts: bool,
    pub has_unexpected_resource_changes: bool,
    pub remediation_advice: Vec<RemediationAdvice>,
}

impl FailureDiagnostics {
    /// Gathers diagnostic context from task state, agent events, git conflicts, and deliveries.
    pub async fn diagnose_task(pool: &PgPool, task_id: Uuid) -> Result<Self> {
        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task {task_id} not found"))?;

        // 1. Get last error message from events
        let events = AgentEventRepository::list_by_task(pool, task_id).await?;
        let error_msg = events
            .iter()
            .rev()
            .find(|e| matches!(e.event_type, crate::domain::AgentEventType::Failed))
            .and_then(|e| e.message.clone())
            .unwrap_or_else(|| "No specific failure message reported by agent".to_string());

        // 2. Agent diagnostics
        let (agent_owner, agent_consec, agent_health) = if let Some(agent_id) = task.assigned_agent_id {
            if let Some(agent) = AgentRepository::find_by_id(pool, agent_id).await? {
                (
                    Some(agent.human_owner),
                    agent.consecutive_failures,
                    Some(agent.health_status),
                )
            } else {
                (None, 0, None)
            }
        } else {
            (None, 0, None)
        };

        // 3. Deliveries count
        let deliveries = TaskDeliveryRepository::list_by_task(pool, task_id).await?;
        let delivery_attempts = deliveries.len();

        // 4. Git conflict checks
        let conflicts = GitConflictRepository::list_by_task(pool, task_id).await?;
        let has_git_conflicts = !conflicts.is_empty();

        // 5. Unexpected resource changes
        let unexpected = UnexpectedResourceRepository::list_by_task(pool, task_id).await?;
        let has_unexpected_resource_changes = !unexpected.is_empty();

        // 6. Formulate actionable remediation advice
        let mut remediation_advice = Vec::new();

        if has_git_conflicts {
            remediation_advice.push(RemediationAdvice {
                title: "Resolve Cross-Agent Git Merge Conflicts".to_string(),
                description: format!("Task has {} detected 3-way merge conflict(s) with base branch.", conflicts.len()),
                suggested_action: "inspect_git_conflict".to_string(),
            });
        }

        if has_unexpected_resource_changes {
            remediation_advice.push(RemediationAdvice {
                title: "Review Unexpected Resource Footprint".to_string(),
                description: "Agent modified files outside the approved task plan.".to_string(),
                suggested_action: "review_unexpected_resources".to_string(),
            });
        }

        if agent_health == Some(HealthStatus::Unhealthy) || agent_consec >= 3 {
            remediation_advice.push(RemediationAdvice {
                title: "Reassign Task to Alternative Agent".to_string(),
                description: format!("Current agent has {} consecutive failures and degraded health.", agent_consec),
                suggested_action: "reassign_to_other_agent".to_string(),
            });
        }

        if delivery_attempts >= 3 {
            remediation_advice.push(RemediationAdvice {
                title: "Trigger AI Dynamic Re-Planning".to_string(),
                description: "Task has exceeded retry limit. Re-planning can split into smaller tasks.".to_string(),
                suggested_action: "replan".to_string(),
            });
        }

        if remediation_advice.is_empty() {
            remediation_advice.push(RemediationAdvice {
                title: "Retry Task Execution".to_string(),
                description: "Transient error encountered; retry task on the same agent or trigger re-approval.".to_string(),
                suggested_action: "retry".to_string(),
            });
        }

        Ok(Self {
            task_id,
            short_id: task.short_id,
            task_title: task.title,
            failed_at: task.updated_at,
            error_message: error_msg,
            assigned_agent_id: task.assigned_agent_id,
            agent_owner,
            agent_consecutive_failures: agent_consec,
            agent_health_status: agent_health,
            delivery_attempts,
            has_git_conflicts,
            has_unexpected_resource_changes,
            remediation_advice,
        })
    }
}
