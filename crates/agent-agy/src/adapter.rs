use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use agent_protocol::AgentAdapter;

/// Adapter configuration for running tasks with real Antigravity (`agy`) CLI instances.
#[derive(Debug, Clone)]
pub struct AgyAgent {
    /// Unique identifier for this agent instance.
    pub id: Uuid,
    /// Name of the human who owns/operates this agent.
    pub human_owner: String,
    /// Supported capability tags (e.g., ["rust", "linux", "backend"]).
    pub capabilities: Vec<String>,
    /// Authentication token/key for coordinator registration.
    pub api_key: String,
    /// Path to the `agy` executable binary.
    pub agy_path: PathBuf,
    /// Optional AI model override (e.g., "gemini-3.8-flash-high").
    pub model: Option<String>,
    /// Optional reasoning effort level ("low", "medium", "high").
    pub effort: Option<String>,
    /// Target workspace directory where agy will execute tasks.
    pub workspace_dir: Option<PathBuf>,
    /// Maximum execution duration before timing out and killing the subprocess.
    pub timeout: Duration,
    /// Auto-approve tool permissions without user prompt.
    pub dangerously_skip_permissions: bool,
    /// Optional blocker task UUID for simulating/testing blocked states.
    pub simulate_blocker: Option<Uuid>,
}

impl AgyAgent {
    /// Creates a new `AgyAgent` with sane defaults.
    pub fn new(human_owner: impl Into<String>, api_key: impl Into<String>) -> Self {
        let agy_path = std::env::var("AGY_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Check if ~/.local/bin/agy exists, otherwise fallback to "agy" in PATH
                let home_bin = dirs_home().map(|h| h.join(".local/bin/agy"));
                if let Some(ref path) = home_bin {
                    if path.exists() {
                        return path.clone();
                    }
                }
                PathBuf::from("agy")
            });

        Self {
            id: Uuid::new_v4(),
            human_owner: human_owner.into(),
            capabilities: vec!["agy".to_string(), "general-coding".to_string()],
            api_key: api_key.into(),
            agy_path,
            model: None,
            effort: Some("medium".to_string()),
            workspace_dir: None,
            timeout: Duration::from_secs(600), // 10 minutes default
            dangerously_skip_permissions: true,
            simulate_blocker: None,
        }
    }

    pub fn with_simulate_blocker(mut self, blocker_id: Uuid) -> Self {
        self.simulate_blocker = Some(blocker_id);
        self
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_agy_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.agy_path = path.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_effort(mut self, effort: impl Into<String>) -> Self {
        self.effort = Some(effort.into());
        self
    }

    pub fn with_workspace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workspace_dir = Some(dir.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_permissions_skip(mut self, skip: bool) -> Self {
        self.dangerously_skip_permissions = skip;
        self
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

impl AgentAdapter for AgyAgent {
    fn agent_id(&self) -> Uuid {
        self.id
    }

    fn human_owner(&self) -> &str {
        &self.human_owner
    }

    fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    fn adapter_type(&self) -> &str {
        "Agy"
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agy_agent_adapter_contract() {
        let agent = AgyAgent::new("Alice", "secret_key")
            .with_capabilities(vec!["rust".to_string(), "backend".to_string()])
            .with_effort("high");

        assert_eq!(agent.human_owner(), "Alice");
        assert_eq!(agent.api_key(), "secret_key");
        assert_eq!(agent.adapter_type(), "Agy");
        assert_eq!(agent.capabilities(), &["rust", "backend"]);
        assert_eq!(agent.effort.as_deref(), Some("high"));
        assert!(agent.dangerously_skip_permissions);
    }
}
