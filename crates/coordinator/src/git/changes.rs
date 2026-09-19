use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use tokio::process::Command;

/// Represents an unexpected file or directory modification performed by an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnexpectedChange {
    pub resource_path: String,
    pub reason: String,
}

/// Tracks files and resources modified by agents and detects unexpected changes.
pub struct ResourceTracker;

impl ResourceTracker {
    /// Detects all files modified by an agent inside a workspace worktree
    /// compared to the starting `base_commit_sha`.
    ///
    /// Combines:
    /// 1. Untracked files (`git status --porcelain`)
    /// 2. Uncommitted staged and unstaged modifications (`git diff --name-only HEAD`)
    /// 3. Committed modifications on the task branch (`git diff --name-only <base_commit> HEAD`)
    pub async fn detect_modified_resources(
        worktree_path: &Path,
        base_commit_sha: &str,
    ) -> Result<Vec<String>> {
        let mut modified = BTreeSet::new();

        // 1. Untracked files
        let status_out = Command::new("git")
            .args(["status", "--porcelain", "-uall"])
            .current_dir(worktree_path)
            .output()
            .await
            .context("Failed to run git status")?;

        for line in String::from_utf8_lossy(&status_out.stdout).lines() {
            let line = line.trim();
            if line.starts_with("?? ") {
                let path = line.trim_start_matches("?? ").trim().trim_matches('"');
                if !path.is_empty() {
                    modified.insert(path.replace('\\', "/"));
                }
            }
        }

        // 2. Uncommitted changes against HEAD
        let diff_uncommitted = Command::new("git")
            .args(["diff", "--name-only", "HEAD"])
            .current_dir(worktree_path)
            .output()
            .await?;

        for line in String::from_utf8_lossy(&diff_uncommitted.stdout).lines() {
            let path = line.trim().trim_matches('"');
            if !path.is_empty() {
                modified.insert(path.replace('\\', "/"));
            }
        }

        // 3. Committed changes between base_commit_sha and HEAD
        if !base_commit_sha.is_empty() {
            let diff_committed = Command::new("git")
                .args(["diff", "--name-only", base_commit_sha, "HEAD"])
                .current_dir(worktree_path)
                .output()
                .await?;

            for line in String::from_utf8_lossy(&diff_committed.stdout).lines() {
                let path = line.trim().trim_matches('"');
                if !path.is_empty() {
                    modified.insert(path.replace('\\', "/"));
                }
            }
        }

        Ok(modified.into_iter().collect())
    }

    /// Compares the actual modified files against the planned/approved `affected_resources`.
    ///
    /// Flags any file modified by the agent that does not match any planned resource
    /// (via exact path match, directory prefix match, or glob pattern).
    pub fn detect_unexpected_changes(
        actual_modified: &[String],
        planned_affected: &[String],
    ) -> Vec<UnexpectedChange> {
        let mut unexpected = Vec::new();

        for actual in actual_modified {
            let matches = planned_affected.iter().any(|planned| {
                let planned = planned.trim().replace('\\', "/");
                let actual = actual.trim().replace('\\', "/");

                // 1. Exact match
                if actual == planned {
                    return true;
                }

                // 2. Directory prefix match (e.g. "src/" or "src" covers "src/main.rs")
                let dir_prefix = if planned.ends_with('/') {
                    planned.clone()
                } else {
                    format!("{planned}/")
                };

                if actual.starts_with(&dir_prefix) {
                    return true;
                }

                // 3. Simple wildcard matching (e.g. "*.rs" or "docs/*")
                if planned.contains('*') && simple_wildcard_match(&planned, &actual) {
                    return true;
                }

                false
            });

            if !matches {
                unexpected.push(UnexpectedChange {
                    resource_path: actual.clone(),
                    reason: format!(
                        "File '{actual}' was modified but was not declared in planned affected resources: {planned_affected:?}"
                    ),
                });
            }
        }

        unexpected
    }
}

fn simple_wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return text.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        return text.starts_with(prefix) && text.ends_with(suffix);
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::identity::RepositoryIdentity;
    use crate::git::workspace::AgentWorkspace;
    use uuid::Uuid;

    #[test]
    fn test_unexpected_change_detection() {
        let planned = vec![
            "src/auth/".to_string(),
            "Cargo.toml".to_string(),
            "docs/*.md".to_string(),
        ];

        let actual = vec![
            "src/auth/login.rs".to_string(),
            "Cargo.toml".to_string(),
            "docs/readme.md".to_string(),
            "src/unexpected.rs".to_string(),
            "secret.key".to_string(),
        ];

        let unexpected = ResourceTracker::detect_unexpected_changes(&actual, &planned);
        assert_eq!(unexpected.len(), 2);
        assert!(unexpected
            .iter()
            .any(|u| u.resource_path == "src/unexpected.rs"));
        assert!(unexpected.iter().any(|u| u.resource_path == "secret.key"));
    }

    #[tokio::test]
    async fn test_detect_modified_resources_in_workspace() {
        let temp_dir =
            std::env::temp_dir().join(format!("agentmesh_changes_test_{}", Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.unwrap();

        let ws = AgentWorkspace::create(
            &id.repo_root,
            Uuid::new_v4(),
            "TASK-100",
            "agentmesh/task-100",
            "main",
            None,
        )
        .await
        .unwrap();

        // 1. Initially no modified files
        let initial_mods =
            ResourceTracker::detect_modified_resources(&ws.worktree_path, &ws.base_commit_sha)
                .await
                .unwrap();
        assert!(initial_mods.is_empty());

        // 2. Add an untracked file
        tokio::fs::write(ws.worktree_path.join("untracked.txt"), "data")
            .await
            .unwrap();
        let mods_untracked =
            ResourceTracker::detect_modified_resources(&ws.worktree_path, &ws.base_commit_sha)
                .await
                .unwrap();
        assert_eq!(mods_untracked, vec!["untracked.txt"]);

        // 3. Commit the file and modify README
        let _ = Command::new("git")
            .args(["add", "untracked.txt"])
            .current_dir(&ws.worktree_path)
            .output()
            .await;
        let _ = Command::new("git")
            .args(["commit", "-m", "feat: add untracked.txt"])
            .current_dir(&ws.worktree_path)
            .output()
            .await;

        tokio::fs::write(ws.worktree_path.join("README.md"), "# Updated Readme\n")
            .await
            .unwrap();

        let all_mods =
            ResourceTracker::detect_modified_resources(&ws.worktree_path, &ws.base_commit_sha)
                .await
                .unwrap();

        assert!(all_mods.contains(&"untracked.txt".to_string()));
        assert!(all_mods.contains(&"README.md".to_string()));

        // Cleanup
        ws.cleanup().await.unwrap();
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
