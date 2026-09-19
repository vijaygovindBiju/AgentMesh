//! Aggregated Chronological Task and Agent Execution Timelines.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{
    AgentEventRepository, AgentRepository, TaskDeliveryRepository, TaskRepository,
};
use crate::domain::{AgentStatus, HealthStatus, TaskStatus};
use crate::observability::events::CoordinatorEventRepository;

/// A single milestone or event along a task's lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTimelineItem {
    pub timestamp: DateTime<Utc>,
    pub stage: String,
    pub actor: String,
    pub message: String,
    pub details: serde_json::Value,
    pub elapsed_since_start_ms: Option<i64>,
}

/// Comprehensive lifecycle timeline for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTimeline {
    pub task_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub assigned_agent_id: Option<Uuid>,
    pub assigned_agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub total_duration_ms: Option<i64>,
    pub events: Vec<TaskTimelineItem>,
}

/// An activity or health record along an agent's operational history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivityItem {
    pub timestamp: DateTime<Utc>,
    pub activity_type: String,
    pub task_id: Option<Uuid>,
    pub message: String,
    pub details: serde_json::Value,
}

/// Comprehensive activity timeline for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTimeline {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub status: AgentStatus,
    pub health_status: HealthStatus,
    pub tasks_completed_count: i32,
    pub tasks_failed_count: i32,
    pub consecutive_failures: i32,
    pub total_deliveries_assigned: usize,
    pub activities: Vec<AgentActivityItem>,
}

pub struct TimelineService;

impl TimelineService {
    /// Compiles a chronological timeline of all milestones and events for a task.
    pub async fn build_task_timeline(pool: &PgPool, task_id: Uuid) -> Result<TaskTimeline> {
        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Task {task_id} not found"))?;

        let agent_name = if let Some(agent_id) = task.assigned_agent_id {
            AgentRepository::find_by_id(pool, agent_id)
                .await?
                .map(|a| a.human_owner)
        } else {
            None
        };

        let mut timeline_items = Vec::new();

        // 1. Initial task proposal / creation
        timeline_items.push(TaskTimelineItem {
            timestamp: task.created_at,
            stage: "proposed".to_string(),
            actor: "coordinator".to_string(),
            message: format!("Task proposed: {}", task.title),
            details: serde_json::json!({ "estimated_size": task.estimated_size }),
            elapsed_since_start_ms: None,
        });

        // 2. Deliveries / assignment attempts
        let deliveries = TaskDeliveryRepository::list_by_task(pool, task_id).await?;
        for del in &deliveries {
            timeline_items.push(TaskTimelineItem {
                timestamp: del.delivered_at,
                stage: "delivery_attempt".to_string(),
                actor: format!("agent:{}", del.agent_id),
                message: format!("Delivery attempt #{} ({:?})", del.attempt, del.status),
                details: serde_json::json!({
                    "attempt": del.attempt,
                    "status": del.status,
                    "idempotency_key": del.idempotency_key,
                }),
                elapsed_since_start_ms: None,
            });
        }

        // 3. Agent lifecycle events
        let agent_events = AgentEventRepository::list_by_task(pool, task_id).await?;
        let mut first_start_time = None;

        for ev in &agent_events {
            let stage = match ev.event_type {
                crate::domain::AgentEventType::TaskStarted => {
                    if first_start_time.is_none() {
                        first_start_time = Some(ev.received_at);
                    }
                    "started".to_string()
                }
                crate::domain::AgentEventType::ProgressUpdate => "progress".to_string(),
                crate::domain::AgentEventType::Blocked => "blocked".to_string(),
                crate::domain::AgentEventType::Completed => "completed".to_string(),
                crate::domain::AgentEventType::Failed => "failed".to_string(),
            };

            let elapsed = first_start_time.map(|st| ev.received_at.signed_duration_since(st).num_milliseconds());

            timeline_items.push(TaskTimelineItem {
                timestamp: ev.received_at,
                stage,
                actor: format!("agent:{}", ev.agent_id),
                message: ev.message.clone().unwrap_or_else(|| format!("{:?}", ev.event_type)),
                details: ev.payload.clone(),
                elapsed_since_start_ms: elapsed,
            });
        }

        // 4. Coordinator events
        let coord_events = CoordinatorEventRepository::find_by_task(pool, task_id).await?;
        for cev in coord_events {
            timeline_items.push(TaskTimelineItem {
                timestamp: cev.timestamp,
                stage: format!("coordinator:{}", cev.event_type),
                actor: "coordinator".to_string(),
                message: cev.message,
                details: cev.payload,
                elapsed_since_start_ms: None,
            });
        }

        // Sort all items chronologically
        timeline_items.sort_by_key(|item| item.timestamp);

        let completed_at = if matches!(task.status, TaskStatus::Completed | TaskStatus::Failed) {
            Some(task.updated_at)
        } else {
            None
        };

        let total_duration_ms = match (first_start_time, completed_at) {
            (Some(st), Some(ct)) => Some(ct.signed_duration_since(st).num_milliseconds().max(0)),
            _ => None,
        };

        Ok(TaskTimeline {
            task_id,
            short_id: task.short_id,
            title: task.title,
            status: task.status,
            assigned_agent_id: task.assigned_agent_id,
            assigned_agent_name: agent_name,
            created_at: task.created_at,
            started_at: first_start_time,
            completed_at,
            total_duration_ms,
            events: timeline_items,
        })
    }

    /// Compiles a chronological timeline of all activities and state transitions for a specific agent.
    pub async fn build_agent_timeline(pool: &PgPool, agent_id: Uuid) -> Result<AgentTimeline> {
        let agent = AgentRepository::find_by_id(pool, agent_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Agent {agent_id} not found"))?;

        let mut activities = Vec::new();

        // 1. Initial registration
        activities.push(AgentActivityItem {
            timestamp: agent.created_at,
            activity_type: "registered".to_string(),
            task_id: None,
            message: format!("Agent registered with adapter {:?}", agent.adapter_type),
            details: serde_json::json!({
                "capabilities": agent.capabilities_list(),
                "role": agent.role,
            }),
        });

        // 2. Deliveries assigned to this agent
        let deliveries = TaskDeliveryRepository::list_by_agent(pool, agent_id).await?;
        let total_deliveries_assigned = deliveries.len();
        for del in deliveries {
            activities.push(AgentActivityItem {
                timestamp: del.delivered_at,
                activity_type: "task_delivery".to_string(),
                task_id: Some(del.task_id),
                message: format!("Delivery attempt #{} for task {}", del.attempt, del.task_id),
                details: serde_json::json!({ "status": del.status }),
            });
        }

        // 3. Agent reported events
        let events = AgentEventRepository::list_by_agent(pool, agent_id).await?;
        for ev in events {
            activities.push(AgentActivityItem {
                timestamp: ev.received_at,
                activity_type: format!("{:?}", ev.event_type).to_lowercase(),
                task_id: Some(ev.task_id),
                message: ev.message.unwrap_or_default(),
                details: ev.payload,
            });
        }

        activities.sort_by_key(|item| item.timestamp);

        Ok(AgentTimeline {
            agent_id,
            human_owner: agent.human_owner,
            status: agent.status,
            health_status: agent.health_status,
            tasks_completed_count: agent.tasks_completed_count,
            tasks_failed_count: agent.tasks_failed_count,
            consecutive_failures: agent.consecutive_failures,
            total_deliveries_assigned,
            activities,
        })
    }
}
