use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GitConflictRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub task_id_a: Uuid,
    pub task_id_b: Uuid,
    pub conflicting_path: String,
    pub description: String,
    pub resolved: bool,
    pub created_at: DateTime<Utc>,
}

pub struct GitConflictRepository;

impl GitConflictRepository {
    /// Records a new cross-agent Git conflict.
    pub async fn record_conflict(
        pool: &PgPool,
        project_id: Uuid,
        task_id_a: Uuid,
        task_id_b: Uuid,
        conflicting_path: &str,
        description: &str,
    ) -> Result<GitConflictRecord> {
        let record = sqlx::query_as!(
            GitConflictRecord,
            r#"
            INSERT INTO git_conflicts (project_id, task_id_a, task_id_b, conflicting_path, description)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, project_id, task_id_a, task_id_b, conflicting_path, description, resolved, created_at
            "#,
            project_id,
            task_id_a,
            task_id_b,
            conflicting_path,
            description
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert git conflict record")?;

        Ok(record)
    }

    /// Lists all unresolved Git conflicts for a project.
    pub async fn list_unresolved(
        pool: &PgPool,
        project_id: Uuid,
    ) -> Result<Vec<GitConflictRecord>> {
        let records = sqlx::query_as!(
            GitConflictRecord,
            r#"
            SELECT id, project_id, task_id_a, task_id_b, conflicting_path, description, resolved, created_at
            FROM git_conflicts
            WHERE project_id = $1 AND resolved = FALSE
            ORDER BY created_at DESC
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list unresolved git conflicts")?;

        Ok(records)
    }

    /// Lists unresolved Git conflicts for a specific task.
    pub async fn list_by_task(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<Vec<GitConflictRecord>> {
        let records = sqlx::query_as!(
            GitConflictRecord,
            r#"
            SELECT id, project_id, task_id_a, task_id_b, conflicting_path, description, resolved, created_at
            FROM git_conflicts
            WHERE (task_id_a = $1 OR task_id_b = $1) AND resolved = FALSE
            ORDER BY created_at DESC
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list git conflicts by task")?;

        Ok(records)
    }

    /// Marks a conflict as resolved.
    pub async fn mark_resolved(pool: &PgPool, conflict_id: Uuid) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE git_conflicts
            SET resolved = TRUE
            WHERE id = $1
            "#,
            conflict_id
        )
        .execute(pool)
        .await
        .context("Failed to mark git conflict resolved")?;

        Ok(())
    }
}
