use anyhow::Result;
use async_trait::async_trait;

use crate::ai::schema::{PlanningRequest, PlanningResponse};

/// Abstraction for LLM providers.
///
/// Keeps the coordinator core and domain completely independent of specific AI SDKs.
/// Implementations live behind this trait (Anthropic, Gemini, OpenAI, Mock).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Calls the model to decompose the project into a structured plan.
    async fn propose_plan(&self, request: &PlanningRequest) -> Result<PlanningResponse>;

    /// Provider identifier for audit trail and proposal records (e.g., "anthropic", "gemini", "mock").
    fn provider_name(&self) -> &str;

    /// Model identifier for audit trail and proposal records (e.g., "claude-3-5-sonnet", "gemini-1.5-pro", "mock-v1").
    fn model_name(&self) -> &str;
}
