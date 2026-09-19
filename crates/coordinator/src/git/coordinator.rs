use anyhow::{Context, Result};
use sqlx::PgPool;
use std::path::Path;
use uuid::Uuid;

use crate::db::repositories::{
    GitConflictRepository, TaskRepository, UnexpectedResourceRepository,
};
use crate::git::branch::BranchStrategy;
use crate::git::changes::{ResourceTracker, UnexpectedChange};
use crate::git::completion::{CompletionGitResult, CompletionManager};
use crate::git::conflict::{ConcurrencySafety, ConflictDetector, GitConflictReport};
use crate::git::workspace::AgentWorkspace;

/// Orchestrates Git repository operations, agent workspace lifecycles,
/// resource modification tracking, and cross-agent conflict prevention.
#[derive(Clone)]
pub struct GitCoordinator {
    pool: PgPool,
}

impl GitCoordinator {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Prepares an isolated Git workspace for an assigned task:
    /// 1. Formats the task branch name `agentmesh/<short_id>`
    /// 2. Spawns an isolated Git worktree pointing to `base_commit_or_branch`
    /// 3. Records `task_branch` and `base_commit_sha` on the task in PostgreSQL
    pub async fn prepare_task_workspace(
        &self,
        repo_root: &Path,
        _project_id: Uuid,
        task_id: Uuid,
        short_id: &str,
        base_commit_or_branch: &str,
        custom_base_dir: Option<&Path>,
    ) -> Result<AgentWorkspace> {
        let task_branch = BranchStrategy::task_branch_name(short_id);

        let workspace = AgentWorkspace::create(
            repo_root,
            task_id,
            short_id,
            &task_branch,
            base_commit_or_branch,
            custom_base_dir,
        )
        .await
        .with_context(|| format!("Failed to create agent workspace for task '{short_id}'"))?;

        // Update database with git branch & base commit
        TaskRepository::update_git_branch(
            &self.pool,
            task_id,
            &workspace.task_branch,
            &workspace.base_commit_sha,
        )
        .await
        .context("Failed to record task git branch in database")?;

        Ok(workspace)
    }

    /// Audits modified resources for a running or completing task.
    ///
    /// Detects all files modified by the agent, compares against planned `affected_resources`,
    /// persists any unexpected file changes into `unexpected_resource_changes`, and updates
    /// `actual_modified_resources` on the task.
    pub async fn audit_task_resources(
        &self,
        task_id: Uuid,
        worktree_path: &Path,
        base_commit_sha: &str,
        planned_affected_resources: &[String],
    ) -> Result<Vec<UnexpectedChange>> {
        let modified =
            ResourceTracker::detect_modified_resources(worktree_path, base_commit_sha).await?;

        // Persist actual modified resources to database
        TaskRepository::update_actual_modified_resources(&self.pool, task_id, &modified).await?;

        // Detect unexpected changes
        let unexpected =
            ResourceTracker::detect_unexpected_changes(&modified, planned_affected_resources);

        // Persist unexpected changes
        for item in &unexpected {
            let _ = UnexpectedResourceRepository::record(
                &self.pool,
                task_id,
                &item.resource_path,
                &item.reason,
            )
            .await;
        }

        Ok(unexpected)
    }

    /// Finalizes a task upon agent completion:
    /// 1. Stages & commits uncommitted changes conforming to commit conventions
    /// 2. Records `completion_commit_sha` and `actual_modified_resources` in PostgreSQL
    /// 3. Validates clean mergeability into the repository's `base_branch`
    /// 4. Cleans up the worktree
    pub async fn finalize_task(
        &self,
        worktree: &AgentWorkspace,
        short_id: &str,
        task_title: &str,
        base_branch: &str,
    ) -> Result<CompletionGitResult> {
        let result =
            CompletionManager::finalize_task_git_state(worktree, short_id, task_title, base_branch)
                .await?;

        // Persist completion state in PostgreSQL
        TaskRepository::record_completion_git_state(
            &self.pool,
            worktree.task_id,
            &result.completion_commit_sha,
            &result.actual_modified_resources,
        )
        .await
        .context("Failed to record task completion git state in database")?;

        Ok(result)
    }

    /// Checks whether two task branches have cross-agent Git merge conflicts,
    /// recording any conflicts in the `git_conflicts` table.
    pub async fn check_cross_task_conflicts(
        &self,
        repo_root: &Path,
        project_id: Uuid,
        task_id_a: Uuid,
        task_branch_a: &str,
        task_id_b: Uuid,
        task_branch_b: &str,
    ) -> Result<Option<GitConflictReport>> {
        let report =
            ConflictDetector::detect_cross_agent_conflicts(repo_root, task_branch_a, task_branch_b)
                .await?;

        if let Some(ref rep) = report {
            for file in &rep.conflicting_files {
                let _ = GitConflictRepository::record_conflict(
                    &self.pool,
                    project_id,
                    task_id_a,
                    task_id_b,
                    file,
                    &rep.description,
                )
                .await;
            }
        }

        Ok(report)
    }

    /// Evaluates whether two tasks can safely run concurrently based on resource footprints.
    pub fn evaluate_concurrency_safety(
        &self,
        resources_a: &[String],
        resources_b: &[String],
    ) -> ConcurrencySafety {
        ConflictDetector::check_concurrency_safety(resources_a, resources_b)
    }
}
