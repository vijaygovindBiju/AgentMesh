//! NATS JetStream Task Delivery Visibility and Redelivery Diagnostics.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{AgentRepository, TaskDeliveryRepository};
use crate::domain::DeliveryStatus;

/// Detailed status of an individual task delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryAttemptInfo {
    pub attempt: i32,
    pub agent_id: Uuid,
    pub agent_name: Option<String>,
    pub status: DeliveryStatus,
    pub delivered_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub is_expired: bool,
    pub idempotency_key: String,
}

/// Diagnostic breakdown of all delivery attempts for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryDiagnostics {
    pub task_id: Uuid,
    pub total_attempts: usize,
    pub current_attempt: Option<DeliveryAttemptInfo>,
    pub prior_attempts: Vec<DeliveryAttemptInfo>,
    pub has_reassigned: bool,
    pub is_terminal: bool,
    pub summary: String,
}

impl DeliveryDiagnostics {
    /// Queries the full history of delivery attempts for a given task.
    pub async fn inspect(pool: &PgPool, task_id: Uuid) -> Result<Self> {
        let deliveries = TaskDeliveryRepository::list_by_task(pool, task_id).await?;
        let now = Utc::now();

        let mut attempt_infos = Vec::new();
        for del in deliveries {
            let agent_name = AgentRepository::find_by_id(pool, del.agent_id)
                .await?
                .map(|a| a.human_owner);

            let is_expired = del.status == DeliveryStatus::Pending && del.expires_at < now;

            attempt_infos.push(DeliveryAttemptInfo {
                attempt: del.attempt,
                agent_id: del.agent_id,
                agent_name,
                status: del.status,
                delivered_at: del.delivered_at,
                acknowledged_at: del.acknowledged_at,
                expires_at: del.expires_at,
                is_expired,
                idempotency_key: del.idempotency_key,
            });
        }

        attempt_infos.sort_by_key(|a| a.attempt);

        let total_attempts = attempt_infos.len();
        let (current_attempt, prior_attempts) = if let Some(last) = attempt_infos.pop() {
            (Some(last), attempt_infos)
        } else {
            (None, Vec::new())
        };

        let has_reassigned = prior_attempts.iter().any(|a| a.status == DeliveryStatus::Reassigned);
        let is_terminal = current_attempt.as_ref().map_or(false, |c| c.status == DeliveryStatus::Terminal);

        let summary = match &current_attempt {
            Some(curr) => match curr.status {
                DeliveryStatus::Acknowledged => {
                    format!("Successfully acknowledged on attempt #{} by {:?}", curr.attempt, curr.agent_name)
                }
                DeliveryStatus::Pending if curr.is_expired => {
                    format!("Attempt #{} timed out/expired, awaiting recovery/reassignment", curr.attempt)
                }
                DeliveryStatus::Pending => {
                    format!("Attempt #{} pending acknowledgment (expires in {}s)", curr.attempt, (curr.expires_at - now).num_seconds().max(0))
                }
                DeliveryStatus::NakRequeued => {
                    format!("Attempt #{} was NAKed and requeued", curr.attempt)
                }
                DeliveryStatus::Terminal => {
                    format!("Delivery permanently failed after {} attempts", total_attempts)
                }
                DeliveryStatus::Reassigned => {
                    format!("Task was reassigned across {} total attempts", total_attempts)
                }
                DeliveryStatus::Delivered => {
                    format!("Attempt #{} delivered over NATS", curr.attempt)
                }
            },
            None => "No delivery attempts recorded for this task".to_string(),
        };

        Ok(Self {
            task_id,
            total_attempts,
            current_attempt,
            prior_attempts,
            has_reassigned,
            is_terminal,
            summary,
        })
    }
}
