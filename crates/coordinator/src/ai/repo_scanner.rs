use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// High-level architectural and structural summary of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryContext {
    pub repo_root: PathBuf,
    pub detected_ecosystems: Vec<String>,
    pub primary_languages: Vec<String>,
    pub key_modules: Vec<String>,
    pub file_tree_sample: Vec<String>,
    pub readme_summary: Option<String>,
}

/// Scans a local Git repository to discover architecture, ecosystems, and file structure.
pub struct RepositoryScanner;

impl RepositoryScanner {
    /// Inspects the target repository and builds a structured `RepositoryContext`.
    pub async fn scan(repo_root: impl AsRef<Path>) -> Result<RepositoryContext> {
        let repo_root = repo_root.as_ref().canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize repo root {}",
                repo_root.as_ref().display()
            )
        })?;

        let mut ecosystems = Vec::new();
        let mut languages = Vec::new();
        let mut key_modules = Vec::new();

        // 1. Detect Rust ecosystem
        if repo_root.join("Cargo.toml").exists() || repo_root.join("backend/Cargo.toml").exists() {
            ecosystems.push("Rust / Cargo".to_string());
            languages.push("Rust".to_string());
            if let Ok(content) = tokio::fs::read_to_string(repo_root.join("Cargo.toml")).await {
                Self::extract_cargo_modules(&content, &mut key_modules);
            }
        }

        // 2. Detect Node.js / TypeScript ecosystem
        let has_node = repo_root.join("package.json").exists()
            || repo_root.join("frontend/package.json").exists()
            || repo_root.join("client/package.json").exists()
            || repo_root.join("ui/package.json").exists()
            || repo_root.join("web/package.json").exists();
        if has_node {
            ecosystems.push("Node.js / npm".to_string());
            let has_ts = repo_root.join("tsconfig.json").exists()
                || repo_root.join("frontend/tsconfig.json").exists()
                || repo_root.join("client/tsconfig.json").exists()
                || repo_root.join("ui/tsconfig.json").exists();
            if has_ts {
                languages.push("TypeScript".to_string());
            } else {
                languages.push("JavaScript".to_string());
            }
            key_modules.push("package.json".to_string());
        }

        // 3. Detect Python ecosystem
        if repo_root.join("pyproject.toml").exists()
            || repo_root.join("requirements.txt").exists()
            || repo_root.join("setup.py").exists()
            || repo_root.join("backend/requirements.txt").exists()
            || repo_root.join("backend/pyproject.toml").exists()
        {
            ecosystems.push("Python".to_string());
            languages.push("Python".to_string());
        }

        // 4. Detect Go ecosystem
        if repo_root.join("go.mod").exists() || repo_root.join("backend/go.mod").exists() {
            ecosystems.push("Go".to_string());
            languages.push("Go".to_string());
        }

        // 5. Scan key top-level source directories
        let mut dir_entries = tokio::fs::read_dir(&repo_root).await?;
        let mut candidate_dirs = Vec::new();
        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.')
                    && name != "target"
                    && name != "node_modules"
                    && name != "vendor"
                {
                    candidate_dirs.push(name);
                }
            }
        }
        candidate_dirs.sort();
        for dir in candidate_dirs {
            if !key_modules.contains(&dir) {
                key_modules.push(dir);
            }
        }

        // 6. Sample file tree (relative paths up to 2 levels deep, ignoring noise)
        let file_tree_sample = Self::collect_file_tree_sample(&repo_root).await?;

        // 7. Extract README summary if available
        let readme_summary = Self::read_readme_summary(&repo_root).await;

        Ok(RepositoryContext {
            repo_root,
            detected_ecosystems: ecosystems,
            primary_languages: languages,
            key_modules,
            file_tree_sample,
            readme_summary,
        })
    }

    fn extract_cargo_modules(cargo_toml: &str, modules: &mut Vec<String>) {
        let mut in_members = false;
        for line in cargo_toml.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("name = ") || trimmed.starts_with("name=") {
                if let Some(name) = trimmed.split('=').nth(1) {
                    let clean = name.trim().trim_matches('"').trim();
                    if !clean.is_empty() && !modules.contains(&clean.to_string()) {
                        modules.push(clean.to_string());
                    }
                }
            }
            if trimmed.starts_with("members") && trimmed.contains('[') {
                in_members = true;
            }
            if in_members {
                for part in trimmed.split('"') {
                    let part = part.trim();
                    if !part.is_empty()
                        && part != "members"
                        && part != "["
                        && part != "]"
                        && part != ","
                        && !part.contains('=')
                        && !modules.contains(&part.to_string())
                    {
                        modules.push(part.to_string());
                    }
                }
                if trimmed.contains(']') {
                    in_members = false;
                }
            }
        }
    }

    async fn collect_file_tree_sample(repo_root: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();
        Self::walk_dir(repo_root, repo_root, 0, 3, &mut files).await?;
        files.sort();
        // Limit sample size to 60 representative files
        if files.len() > 60 {
            files.truncate(60);
            files.push("... (additional files truncated)".to_string());
        }
        Ok(files)
    }

    async fn walk_dir(
        base: &Path,
        current: &Path,
        depth: usize,
        max_depth: usize,
        out: &mut Vec<String>,
    ) -> Result<()> {
        if depth >= max_depth {
            return Ok(());
        }

        let mut reader = match tokio::fs::read_dir(current).await {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip common noise directories
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name == "vendor"
                || name == "dist"
            {
                continue;
            }

            if let Ok(rel) = path.strip_prefix(base) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if path.is_file() {
                    out.push(rel_str);
                } else if path.is_dir() {
                    Box::pin(Self::walk_dir(base, &path, depth + 1, max_depth, out)).await?;
                }
            }
        }

        Ok(())
    }

    async fn read_readme_summary(repo_root: &Path) -> Option<String> {
        let candidates = [
            Some(repo_root.to_path_buf()),
            repo_root.parent().map(|p| p.to_path_buf()),
            repo_root
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf()),
        ];

        for candidate in candidates.into_iter().flatten() {
            for name in &["README.md", "readme.md", "README.txt", "README"] {
                let path = candidate.join(name);
                if path.exists() {
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        let preview: String = content.chars().take(800).collect();
                        return Some(preview.trim().to_string());
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scan_current_repo() {
        let current_dir = std::env::current_dir().unwrap();
        let ctx = RepositoryScanner::scan(&current_dir)
            .await
            .expect("Must scan AgentMesh repo");

        assert!(ctx
            .detected_ecosystems
            .contains(&"Rust / Cargo".to_string()));
        assert!(ctx.primary_languages.contains(&"Rust".to_string()));
        assert!(ctx
            .key_modules
            .iter()
            .any(|m| m.contains("coordinator") || m == "crates"));
        assert!(!ctx.file_tree_sample.is_empty());
        assert!(ctx.readme_summary.is_some());
    }

    #[tokio::test]
    async fn test_scan_temp_mixed_repo() {
        let temp = std::env::temp_dir().join(format!("scan_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(temp.join("src")).await.unwrap();
        tokio::fs::write(temp.join("package.json"), "{\"name\": \"frontend\"}")
            .await
            .unwrap();
        tokio::fs::write(temp.join("tsconfig.json"), "{}")
            .await
            .unwrap();
        tokio::fs::write(temp.join("README.md"), "# Frontend App\nA test application")
            .await
            .unwrap();

        let ctx = RepositoryScanner::scan(&temp).await.unwrap();
        assert!(ctx
            .detected_ecosystems
            .contains(&"Node.js / npm".to_string()));
        assert!(ctx.primary_languages.contains(&"TypeScript".to_string()));
        assert_eq!(
            ctx.readme_summary.as_deref(),
            Some("# Frontend App\nA test application")
        );

        let _ = tokio::fs::remove_dir_all(&temp).await;
    }
}
