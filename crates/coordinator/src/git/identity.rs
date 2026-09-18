use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Authoritative identity of a Git repository coordinated by AgentMesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryIdentity {
    /// Absolute canonical path to the root directory of the repository (.git parent).
    pub repo_root: PathBuf,
    /// Default or base branch (e.g. "main", "master").
    pub base_branch: String,
    /// Latest HEAD commit hash (40-character hex).
    pub head_commit_sha: String,
    /// Optional remote origin URL.
    pub origin_url: Option<String>,
}

impl RepositoryIdentity {
    /// Discovers and validates repository identity from a path inside the repo.
    pub async fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // 1. Verify path is inside a git working tree and obtain repo root
        let root_output = Command::new("git")
            .arg("rev-parse")
            .arg("--show-toplevel")
            .current_dir(path)
            .output()
            .await
            .with_context(|| format!("Failed to run git in {}", path.display()))?;

        if !root_output.status.success() {
            let stderr = String::from_utf8_lossy(&root_output.stderr);
            anyhow::bail!("Path '{}' is not a git repository: {stderr}", path.display());
        }

        let repo_root = PathBuf::from(String::from_utf8_lossy(&root_output.stdout).trim());

        // 2. Obtain current base branch
        let branch_output = Command::new("git")
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .current_dir(&repo_root)
            .output()
            .await?;

        let current_branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();
        let base_branch = if current_branch == "HEAD" || current_branch.is_empty() {
            "main".to_string()
        } else {
            current_branch
        };

        // 3. Obtain HEAD commit SHA
        let sha_output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(&repo_root)
            .output()
            .await?;

        let head_commit_sha = if sha_output.status.success() {
            String::from_utf8_lossy(&sha_output.stdout).trim().to_string()
        } else {
            "0000000000000000000000000000000000000000".to_string()
        };

        // 4. Obtain remote origin URL if configured
        let origin_output = Command::new("git")
            .arg("remote")
            .arg("get-url")
            .arg("origin")
            .current_dir(&repo_root)
            .output()
            .await;

        let origin_url = match origin_output {
            Ok(out) if out.status.success() => {
                let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if url.is_empty() { None } else { Some(url) }
            }
            _ => None,
        };

        Ok(Self {
            repo_root,
            base_branch,
            head_commit_sha,
            origin_url,
        })
    }

    /// Initializes a new git repository in the target path with an initial commit on base_branch.
    pub async fn init(path: impl AsRef<Path>, base_branch: &str) -> Result<Self> {
        let path = path.as_ref();
        tokio::fs::create_dir_all(path).await?;

        let init_out = Command::new("git")
            .arg("init")
            .arg("-b")
            .arg(base_branch)
            .current_dir(path)
            .output()
            .await?;

        if !init_out.status.success() {
            // Fallback for older git versions without -b flag
            Command::new("git").arg("init").current_dir(path).output().await?;
            Command::new("git")
                .arg("checkout")
                .arg("-B")
                .arg(base_branch)
                .current_dir(path)
                .output()
                .await?;
        }

        // Configure dummy user for local test commits
        let _ = Command::new("git")
            .args(["config", "user.name", "AgentMesh Coordinator"])
            .current_dir(path)
            .output()
            .await;
        let _ = Command::new("git")
            .args(["config", "user.email", "coordinator@agentmesh.dev"])
            .current_dir(path)
            .output()
            .await;

        // Create an initial README and commit so HEAD is valid
        let readme_path = path.join("README.md");
        tokio::fs::write(&readme_path, "# AgentMesh Managed Project\n").await?;
        let _ = Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .await;
        let _ = Command::new("git")
            .args(["commit", "-m", "chore: initial commit"])
            .current_dir(path)
            .output()
            .await;

        Self::discover(path).await
    }

    /// Checks whether the working tree has no uncommitted changes or untracked files.
    pub async fn is_clean(&self) -> Result<bool> {
        let status = Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .current_dir(&self.repo_root)
            .output()
            .await?;

        let output = String::from_utf8_lossy(&status.stdout).trim().to_string();
        Ok(output.is_empty())
    }

    /// Resolves a Git reference (e.g. "HEAD", "main", or branch name) to its 40-character SHA.
    pub async fn resolve_ref(&self, git_ref: &str) -> Result<String> {
        let out = Command::new("git")
            .arg("rev-parse")
            .arg(git_ref)
            .current_dir(&self.repo_root)
            .output()
            .await
            .with_context(|| format!("Failed to resolve git ref '{git_ref}'"))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("Failed to resolve git ref '{git_ref}': {stderr}");
        }

        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Refreshes the HEAD commit SHA from disk.
    pub async fn refresh_head(&mut self) -> Result<String> {
        let sha = self.resolve_ref("HEAD").await?;
        self.head_commit_sha = sha.clone();
        Ok(sha)
    }

    /// Stages all changes in the repo and creates a commit with the specified message.
    pub async fn commit_all(&self, message: &str) -> Result<String> {
        let add_out = Command::new("git")
            .arg("add")
            .arg("-A")
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !add_out.status.success() {
            let stderr = String::from_utf8_lossy(&add_out.stderr);
            anyhow::bail!("git add failed: {stderr}");
        }

        let commit_out = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(message)
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !commit_out.status.success() {
            let stderr = String::from_utf8_lossy(&commit_out.stderr);
            anyhow::bail!("git commit failed: {stderr}");
        }

        self.resolve_ref("HEAD").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_current_repo() {
        let current_dir = std::env::current_dir().unwrap();
        let identity = RepositoryIdentity::discover(&current_dir).await;
        assert!(identity.is_ok(), "Must discover AgentMesh repository identity");
        let id = identity.unwrap();
        assert!(id.repo_root.exists());
        assert!(!id.base_branch.is_empty());
        assert_eq!(id.head_commit_sha.len(), 40);
    }

    #[tokio::test]
    async fn test_init_and_clean_check() {
        let temp_dir = std::env::temp_dir().join(format!("agentmesh_repo_test_{}", uuid::Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.expect("Failed to init repo");
        assert_eq!(id.base_branch, "main");
        assert!(id.is_clean().await.unwrap());

        // Modify a file
        tokio::fs::write(temp_dir.join("test.txt"), "hello").await.unwrap();
        assert!(!id.is_clean().await.unwrap(), "Repo must not be clean after writing untracked file");

        // Commit change
        let sha = id.commit_all("feat: add test file").await.expect("Commit should succeed");
        assert_eq!(sha.len(), 40);
        assert!(id.is_clean().await.unwrap());

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
