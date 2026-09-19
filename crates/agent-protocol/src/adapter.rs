use uuid::Uuid;

/// Interface that all agent adapters (Mock, Agy, and future runtimes) must implement.
/// Keeps the agent protocol transport/runtime independent.
pub trait AgentAdapter: Send + Sync {
    /// Unique identifier for this agent instance.
    fn agent_id(&self) -> Uuid;

    /// Display name of the human who owns this agent.
    fn human_owner(&self) -> &str;

    /// Supported capability tags (e.g., ["rust", "postgres"]).
    fn capabilities(&self) -> &[String];

    /// Adapter type string (e.g., "Mock", "Agy").
    fn adapter_type(&self) -> &str;

    /// Raw API key for registration authentication.
    fn api_key(&self) -> &str;

    /// Optional structured capabilities profile.
    fn capability_profile(&self) -> Option<crate::capabilities::AgentCapabilities> {
        None
    }
}
