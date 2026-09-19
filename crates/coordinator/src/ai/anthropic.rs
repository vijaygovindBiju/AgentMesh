use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ai::prompts::PlanningPrompt;
use crate::ai::provider::LlmProvider;
use crate::ai::schema::{PlanningRequest, PlanningResponse};

/// Real LLM provider for Anthropic Claude models via HTTP REST API.
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Strips Markdown code fences if the model wraps JSON in ```json ... ```.
    pub fn clean_json_response(raw: &str) -> &str {
        let trimmed = raw.trim();
        if let Some(stripped) = trimmed.strip_prefix("```json") {
            if let Some(inner) = stripped.strip_suffix("```") {
                return inner.trim();
            }
        } else if let Some(stripped) = trimmed.strip_prefix("```") {
            if let Some(inner) = stripped.strip_suffix("```") {
                return inner.trim();
            }
        }
        trimmed
    }
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn propose_plan(&self, request: &PlanningRequest) -> Result<PlanningResponse> {
        let system_prompt = PlanningPrompt::system_prompt();
        let user_prompt = PlanningPrompt::user_prompt(request);

        let req_body = AnthropicRequest {
            model: &self.model,
            max_tokens: 4096,
            system: system_prompt,
            messages: vec![AnthropicMessage {
                role: "user",
                content: &user_prompt,
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req_body)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Anthropic API returned error {status}: {body}");
        }

        let resp_json: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic response JSON")?;

        let first_text = resp_json
            .content
            .first()
            .map(|c| c.text.as_str())
            .context("Anthropic response content was empty")?;

        let cleaned = Self::clean_json_response(first_text);
        let plan: PlanningResponse = serde_json::from_str(cleaned).with_context(|| {
            format!("Failed to parse PlanningResponse from LLM text: {cleaned}")
        })?;

        Ok(plan)
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_json_response_with_markdown_fences() {
        let raw = "```json\n{\"reasoning\": \"test\", \"proposed_tasks\": [], \"proposed_dependencies\": []}\n```";
        let cleaned = AnthropicProvider::clean_json_response(raw);
        assert_eq!(
            cleaned,
            "{\"reasoning\": \"test\", \"proposed_tasks\": [], \"proposed_dependencies\": []}"
        );
    }

    #[test]
    fn test_clean_json_response_without_fences() {
        let raw = "{\"reasoning\": \"test\"}";
        let cleaned = AnthropicProvider::clean_json_response(raw);
        assert_eq!(cleaned, "{\"reasoning\": \"test\"}");
    }
}
