//! Coordinator Events Stream and Persistence.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

/// A lifecycle or operational event produced by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CoordinatorEvent {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub message: String,
    pub payload: serde_json::Value,
}

pub struct CoordinatorEventRepository;

impl CoordinatorEventRepository {
    /// Inserts a coordinator event record into PostgreSQL.
    pub async fn insert(pool: &PgPool, event: &CoordinatorEvent) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO coordinator_events (
                id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            event.id,
            event.timestamp,
            event.event_type,
            event.project_id,
            event.task_id,
            event.agent_id,
            event.message,
            event.payload,
        )
        .execute(pool)
        .await
        .context("Failed to insert coordinator event")?;

        Ok(())
    }

    /// Records a new event and returns the created event object.
    pub async fn record(
        pool: &PgPool,
        event_type: impl Into<String>,
        project_id: Option<Uuid>,
        task_id: Option<Uuid>,
        agent_id: Option<Uuid>,
        message: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<CoordinatorEvent> {
        let event_type = event_type.into();
        let message = message.into();

        let event = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: event_type.clone(),
            project_id,
            task_id,
            agent_id,
            message: message.clone(),
            payload,
        };

        info!(
            event_type = %event_type,
            ?project_id,
            ?task_id,
            ?agent_id,
            %message,
            "Coordinator Event"
        );

        Self::insert(pool, &event).await?;
        Ok(event)
    }

    /// Fetches the most recent coordinator events across all projects.
    pub async fn find_recent(pool: &PgPool, limit: i64) -> Result<Vec<CoordinatorEvent>> {
        let events = sqlx::query_as!(
            CoordinatorEvent,
            r#"
            SELECT id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            FROM coordinator_events
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(pool)
        .await
        .context("Failed to query recent coordinator events")?;

        Ok(events)
    }

    /// Fetches events scoped to a specific project.
    pub async fn find_by_project(pool: &PgPool, project_id: Uuid, limit: i64) -> Result<Vec<CoordinatorEvent>> {
        let events = sqlx::query_as!(
            CoordinatorEvent,
            r#"
            SELECT id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            FROM coordinator_events
            WHERE project_id = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#,
            project_id,
            limit
        )
        .fetch_all(pool)
        .await
        .context("Failed to query coordinator events by project")?;

        Ok(events)
    }

    /// Fetches events scoped to a specific task.
    pub async fn find_by_task(pool: &PgPool, task_id: Uuid) -> Result<Vec<CoordinatorEvent>> {
        let events = sqlx::query_as!(
            CoordinatorEvent,
            r#"
            SELECT id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            FROM coordinator_events
            WHERE task_id = $1
            ORDER BY timestamp ASC
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to query coordinator events by task")?;

        Ok(events)
    }

    /// Fetches events scoped to a specific agent.
    pub async fn find_by_agent(pool: &PgPool, agent_id: Uuid) -> Result<Vec<CoordinatorEvent>> {
        let events = sqlx::query_as!(
            CoordinatorEvent,
            r#"
            SELECT id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            FROM coordinator_events
            WHERE agent_id = $1
            ORDER BY timestamp ASC
            "#,
            agent_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to query coordinator events by agent")?;

        Ok(events)
    }

    /// Fetches events matching a specific event_type.
    pub async fn find_by_type(pool: &PgPool, event_type: &str, limit: i64) -> Result<Vec<CoordinatorEvent>> {
        let events = sqlx::query_as!(
            CoordinatorEvent,
            r#"
            SELECT id, timestamp, event_type, project_id, task_id, agent_id, message, payload
            FROM coordinator_events
            WHERE event_type = $1
            ORDER BY timestamp DESC
            LIMIT $2
            "#,
            event_type,
            limit
        )
        .fetch_all(pool)
        .await
        .context("Failed to query coordinator events by type")?;

        Ok(events)
    }
}
