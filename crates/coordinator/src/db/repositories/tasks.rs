use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    DependencyKind, NewTask, NewTaskDependency, Task, TaskDependency, TaskStatus,
};

pub struct TaskRepository;

impl TaskRepository {
    /// Inserts a new task with status 'proposed'.
    pub async fn create(pool: &PgPool, new_task: &NewTask) -> Result<Task> {
        let resources_json = serde_json::to_value(&new_task.affected_resources)
            .context("Failed to serialize affected_resources to JSON")?;

        let task = sqlx::query_as!(
            Task,
            r#"
            INSERT INTO tasks (
                project_id, short_id, title, description, status,
                affected_resources, estimated_size, proposal_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, project_id, short_id, title, description,
                      status AS "status: TaskStatus", assigned_agent_id,
                      affected_resources, estimated_size, proposal_id,
                      created_at, updated_at
            "#,
            new_task.project_id,
            new_task.short_id,
            new_task.title,
            new_task.description,
            TaskStatus::Proposed as TaskStatus,
            resources_json,
            new_task.estimated_size,
            new_task.proposal_id
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert task")?;

        Ok(task)
    }

    /// Finds a task by primary key ID.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Task>> {
        let task = sqlx::query_as!(
            Task,
            r#"
            SELECT id, project_id, short_id, title, description,
                   status AS "status: TaskStatus", assigned_agent_id,
                   affected_resources, estimated_size, proposal_id,
                   created_at, updated_at
            FROM tasks
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query task by id")?;

        Ok(task)
    }

    /// Finds a task by project ID and short identifier (e.g. "TASK-001").
    pub async fn find_by_short_id(
        pool: &PgPool,
        project_id: Uuid,
        short_id: &str,
    ) -> Result<Option<Task>> {
        let task = sqlx::query_as!(
            Task,
            r#"
            SELECT id, project_id, short_id, title, description,
                   status AS "status: TaskStatus", assigned_agent_id,
                   affected_resources, estimated_size, proposal_id,
                   created_at, updated_at
            FROM tasks
            WHERE project_id = $1 AND short_id = $2
            "#,
            project_id,
            short_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query task by short_id")?;

        Ok(task)
    }

    /// Lists all tasks for a given project, ordered by created_at ascending.
    pub async fn list_by_project(pool: &PgPool, project_id: Uuid) -> Result<Vec<Task>> {
        let tasks = sqlx::query_as!(
            Task,
            r#"
            SELECT id, project_id, short_id, title, description,
                   status AS "status: TaskStatus", assigned_agent_id,
                   affected_resources, estimated_size, proposal_id,
                   created_at, updated_at
            FROM tasks
            WHERE project_id = $1
            ORDER BY created_at ASC
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list tasks by project")?;

        Ok(tasks)
    }

    /// Lists tasks with a specific status in a project.
    pub async fn list_by_status(
        pool: &PgPool,
        project_id: Uuid,
        status: TaskStatus,
    ) -> Result<Vec<Task>> {
        let tasks = sqlx::query_as!(
            Task,
            r#"
            SELECT id, project_id, short_id, title, description,
                   status AS "status: TaskStatus", assigned_agent_id,
                   affected_resources, estimated_size, proposal_id,
                   created_at, updated_at
            FROM tasks
            WHERE project_id = $1 AND status = $2
            ORDER BY created_at ASC
            "#,
            project_id,
            status as TaskStatus
        )
        .fetch_all(pool)
        .await
        .context("Failed to list tasks by status")?;

        Ok(tasks)
    }

    /// Updates task status, setting updated_at to now.
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        new_status: TaskStatus,
    ) -> Result<Option<Task>> {
        let task = sqlx::query_as!(
            Task,
            r#"
            UPDATE tasks
            SET status = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, project_id, short_id, title, description,
                      status AS "status: TaskStatus", assigned_agent_id,
                      affected_resources, estimated_size, proposal_id,
                      created_at, updated_at
            "#,
            id,
            new_status as TaskStatus
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update task status")?;

        Ok(task)
    }

    /// Assigns or unassigns an agent to a task.
    pub async fn assign_agent(
        pool: &PgPool,
        id: Uuid,
        agent_id: Option<Uuid>,
    ) -> Result<Option<Task>> {
        let task = sqlx::query_as!(
            Task,
            r#"
            UPDATE tasks
            SET assigned_agent_id = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING id, project_id, short_id, title, description,
                      status AS "status: TaskStatus", assigned_agent_id,
                      affected_resources, estimated_size, proposal_id,
                      created_at, updated_at
            "#,
            id,
            agent_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update task assigned agent")?;

        Ok(task)
    }

    /// Adds a dependency edge between two tasks.
    pub async fn add_dependency(
        pool: &PgPool,
        dep: &NewTaskDependency,
    ) -> Result<TaskDependency> {
        let dependency = sqlx::query_as!(
            TaskDependency,
            r#"
            INSERT INTO task_dependencies (dependent_id, depends_on_id, kind)
            VALUES ($1, $2, $3)
            ON CONFLICT (dependent_id, depends_on_id) DO UPDATE SET kind = EXCLUDED.kind
            RETURNING dependent_id, depends_on_id, kind AS "kind: DependencyKind"
            "#,
            dep.dependent_id,
            dep.depends_on_id,
            dep.kind as DependencyKind
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert task dependency")?;

        Ok(dependency)
    }

    /// Returns the list of tasks that `task_id` depends on.
    pub async fn list_dependencies(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<Vec<TaskDependency>> {
        let deps = sqlx::query_as!(
            TaskDependency,
            r#"
            SELECT dependent_id, depends_on_id, kind AS "kind: DependencyKind"
            FROM task_dependencies
            WHERE dependent_id = $1
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list task dependencies")?;

        Ok(deps)
    }

    /// Returns the list of tasks that depend on `task_id`.
    pub async fn list_dependents(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<Vec<TaskDependency>> {
        let deps = sqlx::query_as!(
            TaskDependency,
            r#"
            SELECT dependent_id, depends_on_id, kind AS "kind: DependencyKind"
            FROM task_dependencies
            WHERE depends_on_id = $1
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list task dependents")?;

        Ok(deps)
    }

    /// Checks if all blocking dependencies for a task have reached status 'completed'.
    /// Returns true if there are no blocking dependencies, or all of them are completed.
    pub async fn are_blocking_dependencies_completed(
        pool: &PgPool,
        task_id: Uuid,
    ) -> Result<bool> {
        let uncompleted_count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM task_dependencies td
            JOIN tasks t ON td.depends_on_id = t.id
            WHERE td.dependent_id = $1
              AND td.kind = 'blocks'
              AND t.status <> 'completed'
            "#
        )
        .bind(task_id)
        .fetch_one(pool)
        .await
        .context("Failed to check blocking dependencies")?;

        Ok(uncompleted_count.0 == 0)
    }

    /// Deletes a task by ID.
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows = sqlx::query!(
            r#"
            DELETE FROM tasks
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await
        .context("Failed to delete task")?
        .rows_affected();

        Ok(rows > 0)
    }

    /// Fetches Git coordination context for a task (branch, base branch, repo path, commits).
    pub async fn find_git_context(pool: &PgPool, task_id: Uuid) -> Result<Option<TaskGitContext>> {
        let row = sqlx::query!(
            r#"
            SELECT t.task_branch,
                   p.base_branch,
                   p.repo_path,
                   t.base_commit_sha,
                   t.completion_commit_sha,
                   t.actual_modified_resources
            FROM tasks t
            JOIN projects p ON t.project_id = p.id
            WHERE t.id = $1
            "#,
            task_id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query task git context")?;

        let Some(r) = row else { return Ok(None) };

        let actual_modified = match r.actual_modified_resources {
            serde_json::Value::Array(arr) => arr
                .into_iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        Ok(Some(TaskGitContext {
            task_branch: r.task_branch,
            base_branch: Some(r.base_branch),
            repo_path: r.repo_path,
            base_commit_sha: r.base_commit_sha,
            completion_commit_sha: r.completion_commit_sha,
            actual_modified_resources: actual_modified,
        }))
    }

    /// Updates task Git branch and starting base commit SHA.
    pub async fn update_git_branch(
        pool: &PgPool,
        task_id: Uuid,
        task_branch: &str,
        base_commit_sha: &str,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tasks
            SET task_branch = $2, base_commit_sha = $3, updated_at = NOW()
            WHERE id = $1
            "#,
            task_id,
            task_branch,
            base_commit_sha
        )
        .execute(pool)
        .await
        .context("Failed to update task git branch")?;

        Ok(())
    }

    /// Records task completion Git state (completion commit SHA and actual modified resources).
    pub async fn record_completion_git_state(
        pool: &PgPool,
        task_id: Uuid,
        completion_commit_sha: &str,
        modified_resources: &[String],
    ) -> Result<()> {
        let resources_json = serde_json::to_value(modified_resources)
            .context("Failed to serialize modified resources")?;

        sqlx::query!(
            r#"
            UPDATE tasks
            SET completion_commit_sha = $2, actual_modified_resources = $3, updated_at = NOW()
            WHERE id = $1
            "#,
            task_id,
            completion_commit_sha,
            resources_json
        )
        .execute(pool)
        .await
        .context("Failed to record task completion git state")?;

        Ok(())
    }

    /// Updates the list of actual resources modified by the agent during task execution.
    pub async fn update_actual_modified_resources(
        pool: &PgPool,
        task_id: Uuid,
        modified_resources: &[String],
    ) -> Result<()> {
        let resources_json = serde_json::to_value(modified_resources)
            .context("Failed to serialize modified resources")?;

        sqlx::query!(
            r#"
            UPDATE tasks
            SET actual_modified_resources = $2, updated_at = NOW()
            WHERE id = $1
            "#,
            task_id,
            resources_json
        )
        .execute(pool)
        .await
        .context("Failed to update task actual modified resources")?;

        Ok(())
    }
}

/// Git context associated with a task in AgentMesh.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskGitContext {
    pub task_branch: Option<String>,
    pub base_branch: Option<String>,
    pub repo_path: Option<String>,
    pub base_commit_sha: Option<String>,
    pub completion_commit_sha: Option<String>,
    pub actual_modified_resources: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::projects::ProjectRepository;
    use crate::db::repositories::proposals::ProposalRepository;
    use crate::domain::{NewProject, NewProposal};

    async fn setup_test_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&db_url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_task_crud_and_dependencies() {
        let Some(pool) = setup_test_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // 1. Setup Project & Proposal
        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Task Repo Project".to_string(),
                description: "Testing tasks".to_string(),
            },
        )
        .await
        .expect("Project creation failed");

        let proposal = ProposalRepository::create(
            &pool,
            &NewProposal {
                project_id: project.id,
                ai_provider: "anthropic".to_string(),
                ai_model: "claude-3-5-sonnet".to_string(),
                raw_prompt: "plan".to_string(),
                raw_response: "{}".to_string(),
            },
        )
        .await
        .expect("Proposal creation failed");

        // 2. Create Task 1 (Database schema)
        let t1 = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "TASK-001".to_string(),
                title: "Database schema".to_string(),
                description: "Create initial schema".to_string(),
                affected_resources: vec!["migrations/".to_string()],
                estimated_size: Some("M".to_string()),
                proposal_id: proposal.id,
            },
        )
        .await
        .expect("Task 1 creation failed");

        assert_eq!(t1.status, TaskStatus::Proposed);
        assert_eq!(t1.short_id, "TASK-001");
        assert_eq!(t1.resources(), vec!["migrations/".to_string()]);

        // 3. Create Task 2 (Backend API - depends on Task 1)
        let t2 = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "TASK-002".to_string(),
                title: "Backend API".to_string(),
                description: "Implement REST endpoints".to_string(),
                affected_resources: vec!["src/api.rs".to_string()],
                estimated_size: Some("L".to_string()),
                proposal_id: proposal.id,
            },
        )
        .await
        .expect("Task 2 creation failed");

        // 4. Add blocking dependency: Task 2 depends on Task 1
        TaskRepository::add_dependency(
            &pool,
            &NewTaskDependency {
                dependent_id: t2.id,
                depends_on_id: t1.id,
                kind: DependencyKind::Blocks,
            },
        )
        .await
        .expect("Dependency creation failed");

        // Verify dependency check: Task 1 is still 'proposed', so Task 2 cannot start
        let ready = TaskRepository::are_blocking_dependencies_completed(&pool, t2.id)
            .await
            .expect("Dependency check failed");
        assert!(!ready, "Task 2 should be blocked because Task 1 is not completed");

        // Complete Task 1
        TaskRepository::update_status(&pool, t1.id, TaskStatus::Completed)
            .await
            .expect("Update task 1 status failed");

        // Now Task 2's blocking dependencies are all completed!
        let ready_now = TaskRepository::are_blocking_dependencies_completed(&pool, t2.id)
            .await
            .expect("Dependency check failed");
        assert!(ready_now, "Task 2 should now be ready since Task 1 is completed");

        // 5. Query helpers
        let found_by_short = TaskRepository::find_by_short_id(&pool, project.id, "TASK-002")
            .await
            .expect("Query failed")
            .expect("Task 2 found");
        assert_eq!(found_by_short.id, t2.id);

        let proj_tasks = TaskRepository::list_by_project(&pool, project.id)
            .await
            .expect("List failed");
        assert_eq!(proj_tasks.len(), 2);

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
    }
}
