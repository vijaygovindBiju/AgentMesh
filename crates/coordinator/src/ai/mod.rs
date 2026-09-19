//! AI Planning layer: LLM provider abstraction, structured planning schemas,
//! deterministic plan validation, and PostgreSQL proposal persistence.

pub mod anthropic;
pub mod complexity;
pub mod matcher;
pub mod mock;
pub mod prompts;
pub mod provider;
pub mod replan;
pub mod repo_scanner;
pub mod schema;
pub mod service;
pub mod validator;

use anyhow::{bail, Result};
use std::sync::Arc;

pub use anthropic::AnthropicProvider;
pub use complexity::{ComplexityEstimate, ComplexityEstimator};
pub use matcher::AgentCapabilityMatcher;
pub use mock::MockLlmProvider;
pub use prompts::{PlanningPrompt, ReplanPrompt};
pub use provider::LlmProvider;
pub use replan::ReplanEngine;
pub use repo_scanner::{RepositoryContext, RepositoryScanner};
pub use schema::{
    AvailableAgentContext, CompletedTaskContext, ExistingTaskContext, FailedTaskContext,
    PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask, ReplanRequest,
};
pub use service::PlanningService;
pub use validator::{DetectedOverlap, PlanValidator, ValidatedPlan, ValidationError};

/// Constructs an LLM provider by name.
pub fn create_provider_by_name(name: &str) -> Result<Arc<dyn LlmProvider>> {
    match name.to_lowercase().as_str() {
        "mock" => {
            let model = std::env::var("AI_MODEL").unwrap_or_else(|_| "mock-planner-v1".to_string());
            let mut mock = MockLlmProvider::new();
            mock.model_name = model;
            Ok(Arc::new(mock))
        }
        "anthropic" => {
            let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
                anyhow::anyhow!(
                    "ANTHROPIC_API_KEY environment variable must be set when AI_PROVIDER=anthropic"
                )
            })?;
            let model = std::env::var("AI_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-20241022".to_string());
            Ok(Arc::new(AnthropicProvider::new(api_key, model)))
        }
        other => {
            bail!("Unsupported AI_PROVIDER '{other}'. Supported providers: 'mock', 'anthropic'")
        }
    }
}

/// Factory function to construct the active LLM provider from runtime environment variables.
///
/// Supported providers:
/// - `AI_PROVIDER=mock` (default for tests / local development)
/// - `AI_PROVIDER=anthropic` (requires `ANTHROPIC_API_KEY`)
pub fn create_provider_from_env() -> Result<Arc<dyn LlmProvider>> {
    let provider_type = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "mock".to_string());
    create_provider_by_name(&provider_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_create_provider_by_name() {
        let provider = create_provider_by_name("mock").unwrap();
        assert_eq!(provider.provider_name(), "mock");

        let res = create_provider_by_name("unknown_xyz");
        assert!(res.is_err());
    }

    #[test]
    fn test_create_provider_from_env() {
        let _lock = ENV_MUTEX.lock().unwrap();

        std::env::remove_var("AI_PROVIDER");
        let provider = create_provider_from_env().unwrap();
        assert_eq!(provider.provider_name(), "mock");

        std::env::set_var("AI_PROVIDER", "unknown_xyz");
        let res = create_provider_from_env();
        assert!(res.is_err());
        std::env::remove_var("AI_PROVIDER");
    }
}
