use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use tokio::process::Command;
use uuid::Uuid;

/// An isolated Git worktree dedicated to an agent working on a specific task.
///
/// Git worktrees share the same underlying repository object store and refs,
/// while providing completely separate index, HEAD, and working directory trees.
/// This guarantees zero cross-agent file pollution, zero index lock collisions,
/// and independent Git branch checkouts.
#[derive(Debug, Clone)]
pub struct AgentWorkspace {
    pub task_id: Uuid,
    pub short_id: String,
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub task_branch: String,
    pub base_commit_sha: String,
}

impl AgentWorkspace {
    /// Returns the standard path where a task's worktree is expected to be placed.
    pub fn expected_worktree_path(repo_root: &Path, short_id: &str) -> PathBuf {
        repo_root.join(".agentmesh").join("worktrees").join(short_id)
    }

    /// Creates an isolated Git worktree for the task, checked out on `task_branch`
    /// starting at `base_commit_or_ref`.
    pub async fn create(
        repo_root: &Path,
        task_id: Uuid,
        short_id: &str,
        task_branch: &str,
        base_commit_or_ref: &str,
        custom_base_dir: Option<&Path>,
    ) -> Result<Self> {
        let repo_root = repo_root.canonicalize().with_context(|| {
            format!("Failed to canonicalize repository root {}", repo_root.display())
        })?;

        // 1. Determine destination worktree directory
        let worktree_path = match custom_base_dir {
            Some(base_dir) => base_dir.join(short_id),
            None => Self::expected_worktree_path(&repo_root, short_id),
        };

        if let Some(parent) = worktree_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // If a leftover directory exists at destination, clean it up before creating worktree
        if worktree_path.exists() {
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force", &worktree_path.to_string_lossy()])
                .current_dir(&repo_root)
                .output()
                .await;
            let _ = tokio::fs::remove_dir_all(&worktree_path).await;
        }

        // 2. Add git worktree with new or reset branch pointing to base commit
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-B",
                task_branch,
                &worktree_path.to_string_lossy(),
                base_commit_or_ref,
            ])
            .current_dir(&repo_root)
            .output()
            .await
            .with_context(|| {
                format!(
                    "Failed to execute git worktree add for task '{short_id}' at {}",
                    worktree_path.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git worktree add failed for branch '{task_branch}' at {}: {stderr}",
                worktree_path.display()
            );
        }

        // 3. Obtain resolved base commit SHA
        let sha_output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .await?;
        let base_commit_sha = String::from_utf8_lossy(&sha_output.stdout).trim().to_string();

        // 4. Configure local git user inside worktree if needed
        let _ = Command::new("git")
            .args(["config", "user.name", "AgentMesh Agent"])
            .current_dir(&worktree_path)
            .output()
            .await;
        let _ = Command::new("git")
            .args(["config", "user.email", "agent@agentmesh.dev"])
            .current_dir(&worktree_path)
            .output()
            .await;

        Ok(Self {
            task_id,
            short_id: short_id.to_string(),
            repo_root,
            worktree_path,
            task_branch: task_branch.to_string(),
            base_commit_sha,
        })
    }

    /// Cleans up and deletes the worktree from Git and filesystem.
    pub async fn cleanup(&self) -> Result<()> {
        let path_str = self.worktree_path.to_string_lossy().to_string();

        let output = Command::new("git")
            .args(["worktree", "remove", "--force", &path_str])
            .current_dir(&self.repo_root)
            .output()
            .await;

        if let Ok(out) = output {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(
                    worktree = %self.worktree_path.display(),
                    stderr = %stderr,
                    "git worktree remove reported non-zero exit; attempting fallback directory purge"
                );
            }
        }

        // Fallback: Ensure directory is removed if git worktree leave remnants
        if self.worktree_path.exists() {
            let _ = tokio::fs::remove_dir_all(&self.worktree_path).await;
        }

        // Prune any stale worktree metadata
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.repo_root)
            .output()
            .await;

        Ok(())
    }

    /// Checks if the worktree directory is active and valid.
    pub fn is_valid(&self) -> bool {
        self.worktree_path.exists() && self.worktree_path.join(".git").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::identity::RepositoryIdentity;

    #[tokio::test]
    async fn test_agent_workspace_creation_and_cleanup() {
        let temp_dir = std::env::temp_dir().join(format!("agentmesh_ws_test_{}", Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.unwrap();

        let task_id = Uuid::new_v4();
        let short_id = "TASK-042";
        let task_branch = "agentmesh/task-042";

        let ws = AgentWorkspace::create(
            &id.repo_root,
            task_id,
            short_id,
            task_branch,
            "main",
            None,
        )
        .await
        .expect("Workspace creation must succeed");

        assert!(ws.is_valid());
        assert_eq!(ws.short_id, short_id);
        assert_eq!(ws.task_branch, task_branch);
        assert_eq!(ws.base_commit_sha, id.head_commit_sha);

        // Verify independent working directory: writing to worktree does NOT affect main repo
        let worktree_file = ws.worktree_path.join("agent_work.txt");
        tokio::fs::write(&worktree_file, "isolated agent modifications").await.unwrap();
        assert!(worktree_file.exists());
        assert!(!id.repo_root.join("agent_work.txt").exists(), "Main repo root must remain untouched");

        // Cleanup
        ws.cleanup().await.expect("Cleanup must succeed");
        assert!(!ws.worktree_path.exists());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
