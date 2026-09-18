use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{NewProject, Project, ProjectStatus};

pub struct ProjectRepository;

impl ProjectRepository {
    /// Inserts a new project with status 'draft' and returns the created record.
    pub async fn create(pool: &PgPool, new_project: &NewProject) -> Result<Project> {
        let project = sqlx::query_as!(
            Project,
            r#"
            INSERT INTO projects (name, description, status)
            VALUES ($1, $2, $3)
            RETURNING id, name, description, status AS "status: ProjectStatus", created_at, updated_at
            "#,
            new_project.name,
            new_project.description,
            ProjectStatus::Draft as ProjectStatus
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert project")?;

        Ok(project)
    }

    /// Finds a project by its primary key ID.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Project>> {
        let project = sqlx::query_as!(
            Project,
            r#"
            SELECT id, name, description, status AS "status: ProjectStatus", created_at, updated_at
            FROM projects
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query project by id")?;

        Ok(project)
    }

    /// Lists all projects ordered by creation time descending.
    pub async fn list(pool: &PgPool) -> Result<Vec<Project>> {
        let projects = sqlx::query_as!(
            Project,
            r#"
            SELECT id, name, description, status AS "status: ProjectStatus", created_at, updated_at
            FROM projects
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to list projects")?;

        Ok(projects)
    }

    /// Updates the status of a project.
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: ProjectStatus,
    ) -> Result<Option<Project>> {
        let project = sqlx::query_as!(
            Project,
            r#"
            UPDATE projects
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, name, description, status AS "status: ProjectStatus", created_at, updated_at
            "#,
            id,
            status as ProjectStatus
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update project status")?;

        Ok(project)
    }

    /// Deletes a project by ID. Returns true if a row was deleted.
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows_affected = sqlx::query!(
            r#"
            DELETE FROM projects
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await
        .context("Failed to delete project")?
        .rows_affected();

        Ok(rows_affected > 0)
    }

    /// Updates project Git repository identity.
    pub async fn update_git_identity(
        pool: &PgPool,
        project_id: Uuid,
        repo_path: &str,
        base_branch: &str,
        current_commit_sha: Option<&str>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE projects
            SET repo_path = $2, base_branch = $3, current_commit_sha = $4, updated_at = NOW()
            WHERE id = $1
            "#,
            project_id,
            repo_path,
            base_branch,
            current_commit_sha
        )
        .execute(pool)
        .await
        .context("Failed to update project git identity")?;

        Ok(())
    }

    /// Fetches project Git repository identity (repo_path, base_branch, current_commit_sha).
    pub async fn get_git_identity(
        pool: &PgPool,
        project_id: Uuid,
    ) -> Result<Option<(Option<String>, String, Option<String>)>> {
        let row = sqlx::query!(
            r#"
            SELECT repo_path, base_branch, current_commit_sha
            FROM projects
            WHERE id = $1
            "#,
            project_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query project git identity")?;

        Ok(row.map(|r| (r.repo_path, r.base_branch, r.current_commit_sha)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};

    async fn setup_test_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&db_url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_project_crud_lifecycle() {
        let Some(pool) = setup_test_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // 1. Create
        let new_proj = NewProject {
            name: "AgentMesh Test".to_string(),
            description: "Testing repository layer".to_string(),
        };
        let created = ProjectRepository::create(&pool, &new_proj)
            .await
            .expect("Should create project");
        assert_eq!(created.name, "AgentMesh Test");
        assert_eq!(created.status, ProjectStatus::Draft);

        // 2. Find by id
        let found = ProjectRepository::find_by_id(&pool, created.id)
            .await
            .expect("Should query project")
            .expect("Project should exist");
        assert_eq!(found.id, created.id);

        // 3. Update status
        let updated = ProjectRepository::update_status(&pool, created.id, ProjectStatus::Planning)
            .await
            .expect("Should update status")
            .expect("Updated project returned");
        assert_eq!(updated.status, ProjectStatus::Planning);

        // 4. List
        let list = ProjectRepository::list(&pool).await.expect("Should list projects");
        assert!(list.iter().any(|p| p.id == created.id));

        // 5. Delete
        let deleted = ProjectRepository::delete(&pool, created.id)
            .await
            .expect("Should delete project");
        assert!(deleted);

        let not_found = ProjectRepository::find_by_id(&pool, created.id)
            .await
            .expect("Query should succeed");
        assert!(not_found.is_none());
    }
}
