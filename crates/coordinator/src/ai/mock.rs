use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::ai::provider::LlmProvider;
use crate::ai::schema::{PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask};

/// Deterministic mock LLM provider for testing without external API calls.
#[derive(Debug, Clone)]
pub struct MockLlmProvider {
    pub provider_name: String,
    pub model_name: String,
    pub custom_response: Option<PlanningResponse>,
    pub custom_error: Option<String>,
}

impl Default for MockLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockLlmProvider {
    pub fn new() -> Self {
        Self {
            provider_name: "mock".to_string(),
            model_name: "mock-planner-v1".to_string(),
            custom_response: None,
            custom_error: None,
        }
    }

    /// Configures the mock provider to return a specific PlanningResponse.
    pub fn with_response(mut self, response: PlanningResponse) -> Self {
        self.custom_response = Some(response);
        self
    }

    /// Configures the mock provider to return a simulated failure.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.custom_error = Some(error.into());
        self
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn propose_plan(&self, request: &PlanningRequest) -> Result<PlanningResponse> {
        if let Some(err) = &self.custom_error {
            bail!("{err}");
        }

        if let Some(resp) = &self.custom_response {
            return Ok(resp.clone());
        }

        // Default deterministic 2-task plan with 1 dependency
        let task1_agent = request.available_agents.first().map(|a| a.agent_id);
        let task2_agent = request
            .available_agents
            .get(1)
            .map(|a| a.agent_id)
            .or(task1_agent);

        Ok(PlanningResponse {
            reasoning: format!(
                "Decomposed project '{}' into foundational database schema and core API routes.",
                request.project_name
            ),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-1".to_string(),
                    title: "Setup database schema".to_string(),
                    description: format!("Initialize schema for {}", request.project_name),
                    suggested_agent_id: task1_agent,
                    affected_resources: vec!["migrations/001_initial.sql".to_string()],
                    estimated_size: Some("M".to_string()),
                },
                ProposedTask {
                    short_id: "TASK-2".to_string(),
                    title: "Implement API endpoints".to_string(),
                    description: "Implement CRUD handlers connecting to database".to_string(),
                    suggested_agent_id: task2_agent,
                    affected_resources: vec!["src/api/routes.rs".to_string()],
                    estimated_size: Some("L".to_string()),
                },
            ],
            proposed_dependencies: vec![ProposedDependency {
                dependent_short_id: "TASK-2".to_string(),
                depends_on_short_id: "TASK-1".to_string(),
                kind: "blocks".to_string(),
                reason: "API endpoints require database tables to exist".to_string(),
            }],
        })
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}
