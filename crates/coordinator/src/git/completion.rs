use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::git::changes::ResourceTracker;
use crate::git::conflict::ConflictDetector;
use crate::git::workspace::AgentWorkspace;

/// Result of final task completion Git transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionGitResult {
    pub task_branch: String,
    pub completion_commit_sha: String,
    pub actual_modified_resources: Vec<String>,
    pub can_merge_cleanly: bool,
    pub merge_conflict_warning: Option<String>,
}

/// Coordinates the transition from task completion to persistent Git state.
pub struct CompletionManager;

impl CompletionManager {
    /// Finalizes Git state when an agent reports a task `Completed`:
    /// 1. Detects all actual modified resources
    /// 2. Commits any uncommitted agent work in the workspace
    /// 3. Obtains final HEAD commit SHA on the task branch
    /// 4. Tests clean mergeability into `base_branch`
    /// 5. Cleans up the isolated workspace worktree
    pub async fn finalize_task_git_state(
        worktree: &AgentWorkspace,
        short_id: &str,
        task_title: &str,
        base_branch: &str,
    ) -> Result<CompletionGitResult> {
        let worktree_path = &worktree.worktree_path;

        // 1. Detect all modified resources prior to committing
        let actual_modified =
            ResourceTracker::detect_modified_resources(worktree_path, &worktree.base_commit_sha)
                .await?;

        // 2. Check for uncommitted working tree changes
        let status_out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree_path)
            .output()
            .await
            .context("Failed to check worktree status")?;

        let is_dirty = !String::from_utf8_lossy(&status_out.stdout)
            .trim()
            .is_empty();

        if is_dirty {
            // Stage all files
            let add_out = Command::new("git")
                .args(["add", "-A"])
                .current_dir(worktree_path)
                .output()
                .await?;

            if !add_out.status.success() {
                let stderr = String::from_utf8_lossy(&add_out.stderr);
                anyhow::bail!("git add failed during task completion: {stderr}");
            }

            // Create commit conforming to project commit convention
            let commit_msg = format!("feat({short_id}): {task_title}\n\nTask completion commit coordinated by AgentMesh.");
            let commit_out = Command::new("git")
                .args(["commit", "-m", &commit_msg])
                .current_dir(worktree_path)
                .output()
                .await?;

            if !commit_out.status.success() {
                let stderr = String::from_utf8_lossy(&commit_out.stderr);
                anyhow::bail!("git commit failed during task completion: {stderr}");
            }
        }

        // 3. Obtain final HEAD commit SHA
        let sha_out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .await?;
        let completion_commit_sha = String::from_utf8_lossy(&sha_out.stdout).trim().to_string();

        // 4. Test mergeability against base branch
        let conflict_report = ConflictDetector::detect_cross_agent_conflicts(
            &worktree.repo_root,
            &worktree.task_branch,
            base_branch,
        )
        .await?;

        let (can_merge_cleanly, merge_conflict_warning) = match conflict_report {
            Some(report) => (false, Some(report.description)),
            None => (true, None),
        };

        // 5. Clean up isolated worktree cleanly
        worktree
            .cleanup()
            .await
            .context("Failed to cleanup worktree")?;

        Ok(CompletionGitResult {
            task_branch: worktree.task_branch.clone(),
            completion_commit_sha,
            actual_modified_resources: actual_modified,
            can_merge_cleanly,
            merge_conflict_warning,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::identity::RepositoryIdentity;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_finalize_task_git_state() {
        let temp_dir =
            std::env::temp_dir().join(format!("agentmesh_completion_test_{}", Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.unwrap();

        let ws = AgentWorkspace::create(
            &id.repo_root,
            Uuid::new_v4(),
            "TASK-200",
            "agentmesh/task-200",
            "main",
            None,
        )
        .await
        .unwrap();

        // Simulate agent making changes
        tokio::fs::write(
            ws.worktree_path.join("completed_work.rs"),
            "pub fn done() -> bool { true }",
        )
        .await
        .unwrap();

        let result = CompletionManager::finalize_task_git_state(
            &ws,
            "TASK-200",
            "Implement completed work",
            "main",
        )
        .await
        .expect("Finalization must succeed");

        assert_eq!(result.task_branch, "agentmesh/task-200");
        assert_eq!(result.completion_commit_sha.len(), 40);
        assert_ne!(result.completion_commit_sha, ws.base_commit_sha);
        assert!(result
            .actual_modified_resources
            .contains(&"completed_work.rs".to_string()));
        assert!(result.can_merge_cleanly);
        assert!(result.merge_conflict_warning.is_none());

        // Worktree path must be removed
        assert!(!ws.worktree_path.exists());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
