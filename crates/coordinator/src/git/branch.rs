use std::path::Path;
use anyhow::{Context, Result};
use tokio::process::Command;

/// Branch naming strategy and branch lifecycle operations for AgentMesh tasks.
pub struct BranchStrategy;

impl BranchStrategy {
    /// Generates the canonical branch name for a task.
    /// Example: "TASK-001" -> "agentmesh/task-001"
    pub fn task_branch_name(short_id: &str) -> String {
        let sanitized = short_id
            .trim()
            .to_lowercase()
            .replace([' ', '/', '\\', ':', '~', '^', '?', '*', '['], "-");
        format!("agentmesh/{sanitized}")
    }

    /// Checks if a branch name follows the AgentMesh naming convention.
    pub fn is_agentmesh_branch(branch_name: &str) -> bool {
        branch_name.starts_with("agentmesh/")
    }

    /// Checks if a local branch exists in the repository.
    pub async fn branch_exists(repo_root: &Path, branch_name: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{branch_name}")])
            .current_dir(repo_root)
            .output()
            .await
            .with_context(|| format!("Failed to check branch '{branch_name}'"))?;

        Ok(output.status.success())
    }

    /// Resolves the commit SHA pointed to by a specific branch.
    pub async fn get_branch_sha(repo_root: &Path, branch_name: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", &format!("refs/heads/{branch_name}")])
            .current_dir(repo_root)
            .output()
            .await
            .with_context(|| format!("Failed to resolve branch '{branch_name}'"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Branch '{branch_name}' not found: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Lists all AgentMesh task branches currently present in the repository.
    pub async fn list_task_branches(repo_root: &Path) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["branch", "--list", "agentmesh/*", "--format=%(refname:short)"])
            .current_dir(repo_root)
            .output()
            .await
            .context("Failed to list agentmesh branches")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to list branches: {stderr}");
        }

        let branches = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(branches)
    }

    /// Deletes a local branch from the repository.
    pub async fn delete_branch(repo_root: &Path, branch_name: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        let output = Command::new("git")
            .args(["branch", flag, branch_name])
            .current_dir(repo_root)
            .output()
            .await
            .with_context(|| format!("Failed to delete branch '{branch_name}'"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to delete branch '{branch_name}': {stderr}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::identity::RepositoryIdentity;

    #[test]
    fn test_task_branch_naming() {
        assert_eq!(BranchStrategy::task_branch_name("TASK-001"), "agentmesh/task-001");
        assert_eq!(BranchStrategy::task_branch_name("feat: auth"), "agentmesh/feat--auth");
        assert!(BranchStrategy::is_agentmesh_branch("agentmesh/task-001"));
        assert!(!BranchStrategy::is_agentmesh_branch("main"));
    }

    #[tokio::test]
    async fn test_branch_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("agentmesh_branch_test_{}", uuid::Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.unwrap();

        let branch = BranchStrategy::task_branch_name("TASK-999");
        assert!(!BranchStrategy::branch_exists(&id.repo_root, &branch).await.unwrap());

        // Create branch
        Command::new("git")
            .args(["branch", &branch, "main"])
            .current_dir(&id.repo_root)
            .output()
            .await
            .unwrap();

        assert!(BranchStrategy::branch_exists(&id.repo_root, &branch).await.unwrap());
        let sha = BranchStrategy::get_branch_sha(&id.repo_root, &branch).await.unwrap();
        assert_eq!(sha, id.head_commit_sha);

        let list = BranchStrategy::list_task_branches(&id.repo_root).await.unwrap();
        assert!(list.contains(&branch));

        // Delete branch
        BranchStrategy::delete_branch(&id.repo_root, &branch, true).await.unwrap();
        assert!(!BranchStrategy::branch_exists(&id.repo_root, &branch).await.unwrap());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
