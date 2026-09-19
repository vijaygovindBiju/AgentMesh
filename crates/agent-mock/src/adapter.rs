use std::time::Duration;
use agent_protocol::AgentAdapter;
use uuid::Uuid;

/// Configuration and state for a Mock Agent instance.
#[derive(Debug, Clone)]
pub struct MockAgent {
    pub id: Uuid,
    pub human_owner: String,
    pub capabilities: Vec<String>,
    pub api_key: String,
    pub task_delay: Duration,
    pub simulate_blocker: Option<Uuid>,
    pub simulate_failure: bool,
    pub profile: Option<agent_protocol::AgentCapabilities>,
}

impl MockAgent {
    /// Creates a new MockAgent with default settings.
    pub fn new(human_owner: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            human_owner: human_owner.into(),
            capabilities: vec!["rust".to_string(), "mock".to_string()],
            api_key: api_key.into(),
            task_delay: Duration::from_millis(100),
            simulate_blocker: None,
            simulate_failure: false,
            profile: None,
        }
    }

    /// Sets the task simulation delay.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.task_delay = delay;
        self
    }

    /// Configures the mock agent to simulate a blocked state.
    pub fn with_blocker(mut self, blocking_task_id: Uuid) -> Self {
        self.simulate_blocker = Some(blocking_task_id);
        self
    }

    /// Configures the mock agent to simulate a task failure.
    pub fn with_failure(mut self, should_fail: bool) -> Self {
        self.simulate_failure = should_fail;
        self
    }

    /// Configures a structured capabilities profile for the mock agent.
    pub fn with_profile(mut self, profile: agent_protocol::AgentCapabilities) -> Self {
        self.profile = Some(profile);
        self
    }
}

impl AgentAdapter for MockAgent {
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
        "Mock"
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }

    fn capability_profile(&self) -> Option<agent_protocol::AgentCapabilities> {
        self.profile.clone()
    }
}
