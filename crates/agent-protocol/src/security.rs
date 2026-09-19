//! Security and permission boundary data structures for AgentMesh.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The authorization role assigned to an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    /// Standard worker capable of executing approved tasks within permission boundaries.
    #[default]
    Worker,
    /// Read-only agent capable of code analysis and review without modifying source files.
    Reviewer,
    /// Agent restricted to telemetry or monitoring tasks.
    ReadOnly,
    /// Administrative agent with unrestricted capabilities.
    Admin,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::ReadOnly => "readonly",
            Self::Admin => "admin",
        }
    }
}

impl fmt::Display for AgentRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for AgentRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "worker" => Ok(Self::Worker),
            "reviewer" => Ok(Self::Reviewer),
            "readonly" => Ok(Self::ReadOnly),
            "admin" => Ok(Self::Admin),
            other => Err(format!("Unknown agent role: {other}")),
        }
    }
}

/// Permission boundaries that limit an agent's access to filesystem paths and actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionBoundary {
    /// Allowed path patterns (e.g. `["crates/frontend/*"]`).
    /// If empty, all paths are allowed unless matched by `denied_paths`.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Explicitly forbidden path patterns (e.g. `[".env*", "*credentials*", "*id_rsa*"]`).
    #[serde(default = "default_denied_paths")]
    pub denied_paths: Vec<String>,
    /// Whether the agent is allowed to modify source files in its task branch.
    #[serde(default = "default_true")]
    pub can_modify_code: bool,
    /// Whether the agent is allowed to execute arbitrary shell/build commands.
    #[serde(default = "default_true")]
    pub can_run_commands: bool,
    /// Maximum allowed task execution time in seconds before coordinator triggers timeout.
    #[serde(default)]
    pub max_execution_seconds: Option<u64>,
}

fn default_true() -> bool {
    true
}

fn default_denied_paths() -> Vec<String> {
    vec![
        ".env*".to_string(),
        "*.env".to_string(),
        "*id_rsa*".to_string(),
        "*credentials*".to_string(),
        "*secrets*".to_string(),
        ".git/config".to_string(),
    ]
}

impl Default for PermissionBoundary {
    fn default() -> Self {
        Self {
            allowed_paths: Vec::new(),
            denied_paths: default_denied_paths(),
            can_modify_code: true,
            can_run_commands: true,
            max_execution_seconds: Some(3600), // 1 hour default
        }
    }
}

impl PermissionBoundary {
    /// Creates a permissive boundary with standard security denylists.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method to restrict to specific path prefixes or patterns.
    pub fn with_allowed_path(mut self, pattern: impl Into<String>) -> Self {
        self.allowed_paths.push(pattern.into());
        self
    }

    /// Builder method to add a forbidden path pattern.
    pub fn with_denied_path(mut self, pattern: impl Into<String>) -> Self {
        self.denied_paths.push(pattern.into());
        self
    }

    /// Builder method to set code modification permissions.
    pub fn with_code_modification(mut self, allowed: bool) -> Self {
        self.can_modify_code = allowed;
        self
    }

    /// Builder method to set command execution permissions.
    pub fn with_command_execution(mut self, allowed: bool) -> Self {
        self.can_run_commands = allowed;
        self
    }

    /// Checks whether a specific filesystem path is permitted under these boundaries.
    pub fn is_path_allowed(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");

        // 1. Check against explicitly denied paths first (deny takes precedence)
        for denied in &self.denied_paths {
            if glob_matches(denied, &normalized) {
                return false;
            }
        }

        // 2. If allowed_paths list is non-empty, path must match at least one allowed pattern
        if !self.allowed_paths.is_empty() {
            let matches_allowed = self.allowed_paths.iter().any(|allowed| glob_matches(allowed, &normalized));
            if !matches_allowed {
                return false;
            }
        }

        true
    }

    /// Validates a list of affected resources or file paths.
    /// Returns `Ok(())` if all are permitted, or `Err(denied_paths)` with offending items.
    pub fn validate_resources<'a>(&self, resources: &'a [String]) -> Result<(), Vec<&'a String>> {
        let mut denied = Vec::new();
        for res in resources {
            if !self.is_path_allowed(res) {
                denied.push(res);
            }
        }

        if denied.is_empty() {
            Ok(())
        } else {
            Err(denied)
        }
    }
}

/// Simple glob-like matcher supporting '*' wildcards and prefix matching.
pub fn glob_matches(pattern: &str, candidate: &str) -> bool {
    let p = pattern.trim_start_matches("./");
    let c = candidate.trim_start_matches("./");

    if p == "*" || p == "**" {
        return true;
    }

    if !p.contains('*') {
        // Direct equality or directory prefix
        if c == p {
            return true;
        }
        if p.ends_with('/') && c.starts_with(p) {
            return true;
        }
        return false;
    }

    let parts: Vec<&str> = p.split('*').collect();

    // Pattern starts with wildcard: e.g. "*credentials*"
    if parts.len() == 2 && parts[0].is_empty() && !parts[1].is_empty() {
        // Ends with: e.g. "*.env"
        return c.ends_with(parts[1]);
    } else if parts.len() == 2 && !parts[0].is_empty() && parts[1].is_empty() {
        // Starts with: e.g. ".env*"
        return c.starts_with(parts[0]);
    } else if parts.len() == 3 && parts[0].is_empty() && parts[2].is_empty() {
        // Contains: e.g. "*secret*"
        return c.contains(parts[1]);
    }

    // General wildcard matching
    let mut curr = c;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !curr.starts_with(part) {
                return false;
            }
            curr = &curr[part.len()..];
        } else if i == parts.len() - 1 {
            return curr.ends_with(part);
        } else {
            match curr.find(part) {
                Some(idx) => curr = &curr[idx + part.len()..],
                None => return false,
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matching() {
        assert!(glob_matches(".env*", ".env.production"));
        assert!(glob_matches("*.env", "local.env"));
        assert!(glob_matches("*secret*", "my_secret_keys.json"));
        assert!(glob_matches("crates/frontend/*", "crates/frontend/src/App.tsx"));
        assert!(!glob_matches("crates/frontend/*", "crates/backend/src/main.rs"));
    }

    #[test]
    fn test_permission_boundary_path_filtering() {
        let boundary = PermissionBoundary::new()
            .with_allowed_path("crates/backend/*")
            .with_denied_path("crates/backend/secrets/*");

        // Allowed path
        assert!(boundary.is_path_allowed("crates/backend/src/main.rs"));

        // Explicitly denied secret path inside allowed area
        assert!(!boundary.is_path_allowed("crates/backend/secrets/db_password.txt"));

        // Path outside allowed boundary
        assert!(!boundary.is_path_allowed("crates/frontend/src/App.tsx"));

        // Sensitive root file blocked by default denylist
        assert!(!boundary.is_path_allowed(".env"));
    }

    #[test]
    fn test_resource_validation() {
        let boundary = PermissionBoundary::new();
        let safe = vec!["src/main.rs".to_string(), "Cargo.toml".to_string()];
        assert!(boundary.validate_resources(&safe).is_ok());

        let unsafe_res = vec!["src/main.rs".to_string(), ".env.local".to_string()];
        let err = boundary.validate_resources(&unsafe_res).unwrap_err();
        assert_eq!(err, vec![&".env.local".to_string()]);
    }
}
