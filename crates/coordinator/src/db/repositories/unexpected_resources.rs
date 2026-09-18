use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UnexpectedResourceRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub resource_path: String,
    pub reason: String,
    pub acknowledged: bool,
    pub created_at: DateTime<Utc>,
}

pub struct UnexpectedResourceRepository;

impl UnexpectedResourceRepository {
    /// Records a new unexpected resource modification.
    pub async fn record(
        pool: &PgPool,
        task_id: Uuid,
        resource_path: &str,
        reason: &str,
    ) -> Result<UnexpectedResourceRecord> {
        let record = sqlx::query_as!(
            UnexpectedResourceRecord,
            r#"
            INSERT INTO unexpected_resource_changes (task_id, resource_path, reason)
            VALUES ($1, $2, $3)
            RETURNING id, task_id, resource_path, reason, acknowledged, created_at
            "#,
            task_id,
            resource_path,
            reason
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert unexpected resource change record")?;

        Ok(record)
    }

    /// Lists all unexpected resource changes for a specific task.
    pub async fn list_by_task(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<Vec<UnexpectedResourceRecord>> {
        let records = sqlx::query_as!(
            UnexpectedResourceRecord,
            r#"
            SELECT id, task_id, resource_path, reason, acknowledged, created_at
            FROM unexpected_resource_changes
            WHERE task_id = $1
            ORDER BY created_at ASC
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list unexpected resource changes for task")?;

        Ok(records)
    }

    /// Acknowledges an unexpected resource change.
    pub async fn acknowledge(pool: &PgPool, id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE unexpected_resource_changes
            SET acknowledged = TRUE
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await
        .context("Failed to acknowledge unexpected resource change")?;

        Ok(())
    }
}
