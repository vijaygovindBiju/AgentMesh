use anyhow::{Context, Result};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{NewOverlapWarning, OverlapSeverity, OverlapWarning};

pub struct OverlapWarningRepository;

impl OverlapWarningRepository {
    /// Records a detected resource overlap between two or more tasks.
    pub async fn create(pool: &PgPool, new: &NewOverlapWarning) -> Result<OverlapWarning> {
        let task_ids_json = json!(new.task_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>());

        let warning = sqlx::query_as!(
            OverlapWarning,
            r#"
            INSERT INTO overlap_warnings (project_id, task_ids, resource, severity, acknowledged)
            VALUES ($1, $2, $3, $4, false)
            RETURNING
                id,
                project_id,
                task_ids,
                resource,
                severity AS "severity: OverlapSeverity",
                acknowledged,
                created_at
            "#,
            new.project_id,
            task_ids_json,
            new.resource,
            new.severity as OverlapSeverity,
        )
        .fetch_one(pool)
        .await
        .context("Failed to create overlap warning")?;

        Ok(warning)
    }

    /// Finds an overlap warning by primary key.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<OverlapWarning>> {
        let warning = sqlx::query_as!(
            OverlapWarning,
            r#"
            SELECT
                id,
                project_id,
                task_ids,
                resource,
                severity AS "severity: OverlapSeverity",
                acknowledged,
                created_at
            FROM overlap_warnings
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query overlap warning by id")?;

        Ok(warning)
    }

    /// Lists all overlap warnings for a project ordered by severity (critical first).
    pub async fn list_by_project(
        pool: &PgPool,
        project_id: Uuid,
    ) -> Result<Vec<OverlapWarning>> {
        let warnings = sqlx::query_as!(
            OverlapWarning,
            r#"
            SELECT
                id,
                project_id,
                task_ids,
                resource,
                severity AS "severity: OverlapSeverity",
                acknowledged,
                created_at
            FROM overlap_warnings
            WHERE project_id = $1
            ORDER BY
                CASE severity
                    WHEN 'critical' THEN 0
                    WHEN 'warning'  THEN 1
                    ELSE 2
                END ASC,
                created_at ASC
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list overlap warnings by project")?;

        Ok(warnings)
    }

    /// Returns unacknowledged warnings for a project (shown highlighted in TUI).
    pub async fn list_unacknowledged(
        pool: &PgPool,
        project_id: Uuid,
    ) -> Result<Vec<OverlapWarning>> {
        let warnings = sqlx::query_as!(
            OverlapWarning,
            r#"
            SELECT
                id,
                project_id,
                task_ids,
                resource,
                severity AS "severity: OverlapSeverity",
                acknowledged,
                created_at
            FROM overlap_warnings
            WHERE project_id = $1 AND acknowledged = false
            ORDER BY
                CASE severity
                    WHEN 'critical' THEN 0
                    WHEN 'warning'  THEN 1
                    ELSE 2
                END ASC
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list unacknowledged overlap warnings")?;

        Ok(warnings)
    }

    /// Marks a warning as acknowledged (human has seen and accepted the overlap).
    pub async fn acknowledge(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows = sqlx::query!(
            r#"
            UPDATE overlap_warnings
            SET acknowledged = true
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await
        .context("Failed to acknowledge overlap warning")?
        .rows_affected();

        Ok(rows > 0)
    }

    /// Returns unacknowledged critical overlap warnings that involve the specified task.
    pub async fn list_unacknowledged_critical_for_task(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<Vec<OverlapWarning>> {
        let task_id_json = json!([task_id.to_string()]);
        let warnings = sqlx::query_as!(
            OverlapWarning,
            r#"
            SELECT
                id,
                project_id,
                task_ids,
                resource,
                severity AS "severity: OverlapSeverity",
                acknowledged,
                created_at
            FROM overlap_warnings
            WHERE severity = 'critical'
              AND acknowledged = false
              AND task_ids @> $1
            ORDER BY created_at ASC
            "#,
            task_id_json
        )
        .fetch_all(pool)
        .await
        .context("Failed to list unacknowledged critical overlaps for task")?;

        Ok(warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::projects::ProjectRepository;
    use crate::domain::NewProject;

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_overlap_warnings_crud_and_ack() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Overlap Test Proj".to_string(),
                description: "Testing overlaps".to_string(),
            },
        )
        .await
        .expect("Project creation failed");

        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();

        // 1. Create a Warning severity overlap
        let w1 = OverlapWarningRepository::create(
            &pool,
            &NewOverlapWarning {
                project_id: project.id,
                task_ids: vec![task1_id, task2_id],
                resource: "src/models/user.rs".to_string(),
                severity: OverlapSeverity::Warning,
            },
        )
        .await
        .expect("Warning 1 creation failed");
        assert!(!w1.acknowledged);

        // 2. Create a Critical severity overlap
        let w2 = OverlapWarningRepository::create(
            &pool,
            &NewOverlapWarning {
                project_id: project.id,
                task_ids: vec![task1_id, task2_id],
                resource: "migrations/001_schema.sql".to_string(),
                severity: OverlapSeverity::Critical,
            },
        )
        .await
        .expect("Warning 2 creation failed");

        // 3. List by project: Critical should come first
        let list = OverlapWarningRepository::list_by_project(&pool, project.id)
            .await
            .expect("List failed");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, w2.id, "Critical should be ordered first");
        assert_eq!(list[1].id, w1.id, "Warning should be ordered second");

        // 4. List unacknowledged
        let unacked = OverlapWarningRepository::list_unacknowledged(&pool, project.id)
            .await
            .expect("List unacked failed");
        assert_eq!(unacked.len(), 2);

        // 5. Acknowledge w2
        let ack_ok = OverlapWarningRepository::acknowledge(&pool, w2.id)
            .await
            .expect("Ack failed");
        assert!(ack_ok);

        // 6. List unacknowledged again - only w1 remains
        let unacked_now = OverlapWarningRepository::list_unacknowledged(&pool, project.id)
            .await
            .expect("List unacked failed");
        assert_eq!(unacked_now.len(), 1);
        assert_eq!(unacked_now[0].id, w1.id);

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
    }
}

