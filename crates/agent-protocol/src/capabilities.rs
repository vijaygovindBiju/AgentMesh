use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::status::AgentStatus;

// ─── Runtime Capability ──────────────────────────────────────────────────────

/// Host operating system, CPU architecture, and execution runtime environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapability {
    /// Operating system name (e.g. "linux", "macos", "windows").
    pub os: String,
    /// Host CPU architecture (e.g. "x86_64", "aarch64").
    pub arch: String,
    /// Adapter runtime type (e.g. "Mock", "Agy").
    pub adapter_type: String,
    /// Installed Antigravity CLI version, if detected.
    pub agy_version: Option<String>,
    /// Number of available CPU cores.
    pub cpu_count: usize,
    /// Total system memory in megabytes, if detected.
    pub memory_mb: Option<u64>,
}

impl Default for RuntimeCapability {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            adapter_type: "Mock".to_string(),
            agy_version: None,
            cpu_count: 1,
            memory_mb: None,
        }
    }
}

// ─── Language & Framework Capability ──────────────────────────────────────────

/// A programming language runtime/compiler and supported frameworks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageCapability {
    /// Canonical language name (e.g. "rust", "python", "typescript", "go", "dart").
    pub name: String,
    /// Compiler or runtime version string (e.g. "1.79.0", "3.11.4").
    pub version: Option<String>,
    /// Known frameworks or libraries available (e.g. ["axum", "tokio", "sqlx"]).
    #[serde(default)]
    pub frameworks: Vec<String>,
}

impl LanguageCapability {
    pub fn new(name: impl Into<String>, version: Option<String>, frameworks: Vec<String>) -> Self {
        Self {
            name: name.into().to_lowercase(),
            version,
            frameworks: frameworks.into_iter().map(|f| f.to_lowercase()).collect(),
        }
    }
}

// ─── Tool Capability ─────────────────────────────────────────────────────────

/// A developer tool or CLI utility detected on the agent host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCapability {
    /// Tool binary name (e.g. "git", "docker", "cargo", "npm", "sqlx", "agy").
    pub name: String,
    /// Tool version string if reported.
    pub version: Option<String>,
    /// Executable path if known.
    pub path: Option<String>,
}

impl ToolCapability {
    pub fn new(name: impl Into<String>, version: Option<String>, path: Option<String>) -> Self {
        Self {
            name: name.into().to_lowercase(),
            version,
            path,
        }
    }
}

// ─── Agent Capabilities Profile ──────────────────────────────────────────────

/// Structured profile describing all capabilities of an agent instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentCapabilities {
    /// Host OS, CPU arch, and adapter runtime.
    pub runtime: RuntimeCapability,
    /// Installed programming languages and frameworks.
    #[serde(default)]
    pub languages: Vec<LanguageCapability>,
    /// Detected developer CLI utilities.
    #[serde(default)]
    pub tools: Vec<ToolCapability>,
    /// Free-form domain and skill tags (e.g. ["backend", "frontend", "database", "security"]).
    #[serde(default)]
    pub tags: Vec<String>,
}

impl AgentCapabilities {
    pub fn new(runtime: RuntimeCapability) -> Self {
        Self {
            runtime,
            languages: Vec::new(),
            tools: Vec::new(),
            tags: Vec::new(),
        }
    }

    pub fn with_language(mut self, language: LanguageCapability) -> Self {
        self.languages.push(language);
        self
    }

    pub fn with_tool(mut self, tool: ToolCapability) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into().to_lowercase());
        self
    }

    pub fn with_tags(mut self, tags: impl IntoIterator<Item = String>) -> Self {
        self.tags.extend(tags.into_iter().map(|t| t.to_lowercase()));
        self
    }

    /// Checks if the agent supports the given language name.
    pub fn has_language(&self, lang: &str) -> bool {
        let l = lang.to_lowercase();
        self.languages.iter().any(|c| c.name == l)
    }

    /// Checks if the agent has the given developer tool installed.
    pub fn has_tool(&self, tool: &str) -> bool {
        let t = tool.to_lowercase();
        self.tools.iter().any(|c| c.name == t)
    }

    /// Checks if the agent supports a specific framework.
    pub fn has_framework(&self, framework: &str) -> bool {
        let fw = framework.to_lowercase();
        self.languages.iter().any(|l| l.frameworks.contains(&fw))
    }

    /// Checks if the agent has a specific skill or domain tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        let t = tag.to_lowercase();
        self.tags.iter().any(|tag_str| tag_str == &t)
    }

    /// Returns a flat, deduplicated list of all normalized tags (runtime OS/arch, languages,
    /// frameworks, tools, and domain tags).
    pub fn all_tags(&self) -> Vec<String> {
        let mut list = Vec::new();
        list.push(self.runtime.os.to_lowercase());
        list.push(self.runtime.arch.to_lowercase());
        list.push(self.runtime.adapter_type.to_lowercase());

        for lang in &self.languages {
            list.push(lang.name.to_lowercase());
            for fw in &lang.frameworks {
                list.push(fw.to_lowercase());
            }
        }

        for tool in &self.tools {
            list.push(tool.name.to_lowercase());
        }

        for tag in &self.tags {
            list.push(tag.to_lowercase());
        }

        list.sort();
        list.dedup();
        list
    }
}

// ─── Agent Health ─────────────────────────────────────────────────────────────

/// Overall operational health status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Agent is fully operational and heartbeating normally.
    #[default]
    Healthy,
    /// Agent experienced consecutive failures or elevated latency.
    Degraded,
    /// Agent has excessive failures or critical errors; should not receive tasks.
    Unhealthy,
    /// Agent is disconnected or missed heartbeats.
    Offline,
}

/// Dynamic health metrics and status reported by an agent or evaluated by coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentHealth {
    pub status: HealthStatus,
    pub last_heartbeat: DateTime<Utc>,
    pub heartbeat_latency_ms: Option<u64>,
    pub consecutive_failures: u32,
    pub tasks_completed: u32,
    pub tasks_failed: u32,
    pub last_error: Option<String>,
    pub disk_free_mb: Option<u64>,
}

impl Default for AgentHealth {
    fn default() -> Self {
        Self {
            status: HealthStatus::Healthy,
            last_heartbeat: Utc::now(),
            heartbeat_latency_ms: None,
            consecutive_failures: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            last_error: None,
            disk_free_mb: None,
        }
    }
}

impl AgentHealth {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self.status, HealthStatus::Healthy)
    }

    pub fn record_success(&mut self) {
        self.tasks_completed = self.tasks_completed.saturating_add(1);
        self.consecutive_failures = 0;
        self.status = HealthStatus::Healthy;
    }

    pub fn record_failure(&mut self, error: impl Into<String>) {
        self.tasks_failed = self.tasks_failed.saturating_add(1);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_error = Some(error.into());

        if self.consecutive_failures >= 5 {
            self.status = HealthStatus::Unhealthy;
        } else if self.consecutive_failures >= 2 {
            self.status = HealthStatus::Degraded;
        }
    }

    pub fn record_heartbeat(&mut self, latency_ms: Option<u64>) {
        self.last_heartbeat = Utc::now();
        self.heartbeat_latency_ms = latency_ms;
        if self.status == HealthStatus::Offline {
            self.status = HealthStatus::Healthy;
        }
    }
}

// ─── Agent Availability ───────────────────────────────────────────────────────

/// Availability state of an agent for accepting new task assignments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAvailability {
    pub status: AgentStatus,
    pub is_available: bool,
    pub max_concurrency: usize,
    pub active_tasks: usize,
    pub draining: bool,
}

impl Default for AgentAvailability {
    fn default() -> Self {
        Self {
            status: AgentStatus::Offline,
            is_available: false,
            max_concurrency: 1,
            active_tasks: 0,
            draining: false,
        }
    }
}

impl AgentAvailability {
    pub fn can_accept_task(&self) -> bool {
        !self.draining
            && matches!(self.status, AgentStatus::Idle)
            && self.active_tasks < self.max_concurrency
    }
}

// ─── Task Requirements ────────────────────────────────────────────────────────

/// Explicit requirements for a task used to match the best capable agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskRequirements {
    pub required_languages: Vec<String>,
    pub required_tools: Vec<String>,
    pub required_os: Option<String>,
    pub required_tags: Vec<String>,
    pub preferred_tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities_round_trip() {
        let profile = AgentCapabilities::new(RuntimeCapability {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            adapter_type: "Agy".to_string(),
            agy_version: Some("agy 0.4.0".to_string()),
            cpu_count: 8,
            memory_mb: Some(16384),
        })
        .with_language(LanguageCapability::new(
            "rust",
            Some("1.79.0".to_string()),
            vec!["tokio".to_string(), "axum".to_string()],
        ))
        .with_tool(ToolCapability::new(
            "cargo",
            Some("1.79.0".to_string()),
            Some("/usr/bin/cargo".to_string()),
        ))
        .with_tag("backend");

        assert!(profile.has_language("rust"));
        assert!(profile.has_tool("cargo"));
        assert!(profile.has_framework("tokio"));
        assert!(profile.has_tag("backend"));
        assert!(!profile.has_language("python"));

        let all = profile.all_tags();
        assert!(all.contains(&"rust".to_string()));
        assert!(all.contains(&"tokio".to_string()));
        assert!(all.contains(&"cargo".to_string()));
        assert!(all.contains(&"linux".to_string()));

        let json = serde_json::to_string(&profile).expect("serialize");
        let recovered: AgentCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile, recovered);
    }

    #[test]
    fn test_agent_health_state_transitions() {
        let mut health = AgentHealth::new();
        assert!(health.is_healthy());

        health.record_failure("compilation error");
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.status, HealthStatus::Healthy);

        health.record_failure("test failure");
        assert_eq!(health.consecutive_failures, 2);
        assert_eq!(health.status, HealthStatus::Degraded);

        for _ in 0..3 {
            health.record_failure("timeout");
        }
        assert_eq!(health.consecutive_failures, 5);
        assert_eq!(health.status, HealthStatus::Unhealthy);
        assert!(!health.is_healthy());

        health.record_success();
        assert_eq!(health.consecutive_failures, 0);
        assert_eq!(health.status, HealthStatus::Healthy);
        assert!(health.is_healthy());
    }

    #[test]
    fn test_agent_availability_gate() {
        let mut avail = AgentAvailability {
            status: AgentStatus::Idle,
            is_available: true,
            max_concurrency: 1,
            active_tasks: 0,
            draining: false,
        };
        assert!(avail.can_accept_task());

        avail.active_tasks = 1;
        assert!(!avail.can_accept_task());

        avail.active_tasks = 0;
        avail.draining = true;
        assert!(!avail.can_accept_task());

        avail.draining = false;
        avail.status = AgentStatus::Busy;
        assert!(!avail.can_accept_task());
    }
}
