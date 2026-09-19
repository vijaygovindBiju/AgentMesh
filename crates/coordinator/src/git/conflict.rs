use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use tokio::process::Command;

/// Report of cross-agent Git merge conflicts detected between two branches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitConflictReport {
    pub branch_a: String,
    pub branch_b: String,
    pub conflicting_files: Vec<String>,
    pub description: String,
}

/// Evaluation of whether two tasks can safely run concurrently based on resource overlap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencySafety {
    /// Safe to execute simultaneously; resource footprints do not intersect.
    Safe,
    /// Simultaneous execution carries a race or merge conflict risk due to shared files.
    OverlapRisk { shared_resources: Vec<String> },
}

/// Detects cross-agent Git merge conflicts and prevents unsafe simultaneous modifications.
pub struct ConflictDetector;

impl ConflictDetector {
    /// Evaluates whether two task branches would encounter Git 3-way merge conflicts
    /// if merged into a common baseline.
    ///
    /// Uses `git merge-tree` to perform an in-memory merge simulation without modifying
    /// the repository's index or any working tree.
    pub async fn detect_cross_agent_conflicts(
        repo_root: &Path,
        branch_a: &str,
        branch_b: &str,
    ) -> Result<Option<GitConflictReport>> {
        // 1. Resolve merge base
        let base_out = Command::new("git")
            .args(["merge-base", branch_a, branch_b])
            .current_dir(repo_root)
            .output()
            .await
            .with_context(|| format!("Failed to find merge-base for {branch_a} and {branch_b}"))?;

        if !base_out.status.success() {
            // No common ancestor; cannot perform 3-way merge
            return Ok(None);
        }

        let merge_base = String::from_utf8_lossy(&base_out.stdout).trim().to_string();

        // 2. Perform 3-way merge simulation using git merge-tree
        // Try modern git syntax first: git merge-tree --write-tree branch_a branch_b
        let modern_out = Command::new("git")
            .args(["merge-tree", "--write-tree", branch_a, branch_b])
            .current_dir(repo_root)
            .output()
            .await;

        if let Ok(out) = modern_out {
            if !out.status.success() {
                // Exit code non-zero indicates merge conflicts!
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let conflicting_files = Self::parse_conflicts_from_output(&stdout, &stderr);

                return Ok(Some(GitConflictReport {
                    branch_a: branch_a.to_string(),
                    branch_b: branch_b.to_string(),
                    conflicting_files: conflicting_files.clone(),
                    description: format!(
                        "Merge conflicts detected in {} file(s) between '{}' and '{}'",
                        conflicting_files.len(),
                        branch_a,
                        branch_b
                    ),
                }));
            } else {
                return Ok(None);
            }
        }

        // Fallback: Classic git merge-tree <merge_base> <branch_a> <branch_b>
        let classic_out = Command::new("git")
            .args(["merge-tree", &merge_base, branch_a, branch_b])
            .current_dir(repo_root)
            .output()
            .await
            .context("Failed to run git merge-tree")?;

        let output_text = String::from_utf8_lossy(&classic_out.stdout);
        if output_text.contains("+<<<<<<<") || output_text.contains("changed in both") {
            let conflicting_files = Self::parse_classic_merge_tree(&output_text);
            return Ok(Some(GitConflictReport {
                branch_a: branch_a.to_string(),
                branch_b: branch_b.to_string(),
                conflicting_files: conflicting_files.clone(),
                description: format!(
                    "Merge conflicts detected in {} file(s) between '{}' and '{}'",
                    conflicting_files.len(),
                    branch_a,
                    branch_b
                ),
            }));
        }

        Ok(None)
    }

    /// Evaluates concurrency safety for two tasks based on their affected resources.
    pub fn check_concurrency_safety(
        resources_a: &[String],
        resources_b: &[String],
    ) -> ConcurrencySafety {
        let mut shared = BTreeSet::new();

        for res_a in resources_a {
            let a = res_a.trim().replace('\\', "/");
            for res_b in resources_b {
                let b = res_b.trim().replace('\\', "/");

                // 1. Exact path match
                if a == b {
                    shared.insert(a.clone());
                    continue;
                }

                // 2. Directory prefix / containment
                let a_dir = if a.ends_with('/') {
                    a.clone()
                } else {
                    format!("{a}/")
                };
                let b_dir = if b.ends_with('/') {
                    b.clone()
                } else {
                    format!("{b}/")
                };

                if b.starts_with(&a_dir) {
                    shared.insert(b.clone());
                } else if a.starts_with(&b_dir) {
                    shared.insert(a.clone());
                }
            }
        }

        if shared.is_empty() {
            ConcurrencySafety::Safe
        } else {
            ConcurrencySafety::OverlapRisk {
                shared_resources: shared.into_iter().collect(),
            }
        }
    }

    fn parse_conflicts_from_output(stdout: &str, stderr: &str) -> Vec<String> {
        let mut files = BTreeSet::new();
        for line in stdout.lines().chain(stderr.lines()) {
            let line = line.trim();
            if line.starts_with("CONFLICT") {
                if let Some(pos) = line.find(" in ") {
                    let file = line[pos + 4..].trim().trim_matches('"');
                    if !file.is_empty() {
                        files.insert(file.to_string());
                    }
                }
            }
        }
        if files.is_empty() {
            vec!["<conflict detected>".to_string()]
        } else {
            files.into_iter().collect()
        }
    }

    fn parse_classic_merge_tree(output: &str) -> Vec<String> {
        let mut files = BTreeSet::new();
        let mut current_file = None;

        for line in output.lines() {
            if line.starts_with("diff --git") {
                // e.g. diff --git a/foo.txt b/foo.txt
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    current_file = Some(parts[3].trim_start_matches("b/").to_string());
                }
            } else if (line.contains("+<<<<<<<") || line.contains("changed in both"))
                && current_file.is_some()
            {
                if let Some(ref f) = current_file {
                    files.insert(f.clone());
                }
            }
        }

        if files.is_empty() {
            vec!["<conflict detected>".to_string()]
        } else {
            files.into_iter().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::identity::RepositoryIdentity;

    #[test]
    fn test_concurrency_safety_check() {
        // Safe: non-overlapping files
        let res_a = vec!["src/backend.rs".to_string(), "Cargo.toml".to_string()];
        let res_b = vec!["src/frontend.rs".to_string(), "package.json".to_string()];
        assert_eq!(
            ConflictDetector::check_concurrency_safety(&res_a, &res_b),
            ConcurrencySafety::Safe
        );

        // Overlap: exact file match
        let res_c = vec!["src/backend.rs".to_string()];
        match ConflictDetector::check_concurrency_safety(&res_a, &res_c) {
            ConcurrencySafety::OverlapRisk { shared_resources } => {
                assert_eq!(shared_resources, vec!["src/backend.rs"]);
            }
            ConcurrencySafety::Safe => panic!("Must detect overlap risk"),
        }

        // Overlap: directory prefix containment
        let res_dir = vec!["src/".to_string()];
        match ConflictDetector::check_concurrency_safety(&res_dir, &res_a) {
            ConcurrencySafety::OverlapRisk { shared_resources } => {
                assert!(shared_resources.contains(&"src/backend.rs".to_string()));
            }
            ConcurrencySafety::Safe => panic!("Must detect directory overlap risk"),
        }
    }

    #[tokio::test]
    async fn test_detect_cross_agent_conflicts() {
        let temp_dir =
            std::env::temp_dir().join(format!("agentmesh_conflict_test_{}", uuid::Uuid::new_v4()));
        let id = RepositoryIdentity::init(&temp_dir, "main").await.unwrap();

        // 1. Create file in main
        let file_path = id.repo_root.join("conflict.txt");
        tokio::fs::write(&file_path, "base line\n").await.unwrap();
        id.commit_all("feat: add conflict.txt").await.unwrap();

        // 2. Create Branch A with change to line 1
        Command::new("git")
            .args(["checkout", "-b", "agentmesh/branch-a"])
            .current_dir(&id.repo_root)
            .output()
            .await
            .unwrap();
        tokio::fs::write(&file_path, "branch A conflicting edit\n")
            .await
            .unwrap();
        id.commit_all("feat: edit in branch A").await.unwrap();

        // 3. Create Branch B from main with conflicting change to line 1
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&id.repo_root)
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "agentmesh/branch-b"])
            .current_dir(&id.repo_root)
            .output()
            .await
            .unwrap();
        tokio::fs::write(&file_path, "branch B conflicting edit\n")
            .await
            .unwrap();
        id.commit_all("feat: edit in branch B").await.unwrap();

        // Switch back to main
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&id.repo_root)
            .output()
            .await
            .unwrap();

        // 4. Run conflict detection
        let report = ConflictDetector::detect_cross_agent_conflicts(
            &id.repo_root,
            "agentmesh/branch-a",
            "agentmesh/branch-b",
        )
        .await
        .expect("Conflict detection execution must succeed");

        assert!(report.is_some(), "Must detect cross-agent merge conflict");
        let rep = report.unwrap();
        assert_eq!(rep.branch_a, "agentmesh/branch-a");
        assert_eq!(rep.branch_b, "agentmesh/branch-b");
        assert!(rep
            .conflicting_files
            .iter()
            .any(|f| f.contains("conflict.txt") || f == "<conflict detected>"));

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
