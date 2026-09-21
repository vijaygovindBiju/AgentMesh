use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

use crate::ai::anthropic::AnthropicProvider;
use crate::ai::prompts::PlanningPrompt;
use crate::ai::provider::LlmProvider;
use crate::ai::schema::{PlanningRequest, PlanningResponse};

/// AI planner using the Antigravity `agy` CLI binary.
#[derive(Debug, Clone)]
pub struct AgyProvider {
    pub agy_path: PathBuf,
    pub model: String,
}

impl AgyProvider {
    pub fn new(model: impl Into<String>) -> Self {
        let agy_path = std::env::var("AGY_BIN_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("agy"));
        Self {
            agy_path,
            model: model.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for AgyProvider {
    async fn propose_plan(&self, request: &PlanningRequest) -> Result<PlanningResponse> {
        let full_prompt = format!(
            "{}\n\n{}",
            PlanningPrompt::system_prompt(),
            PlanningPrompt::user_prompt(request)
        );

        let mut cmd = Command::new(&self.agy_path);
        cmd.arg("-p")
            .arg(&full_prompt)
            .arg("--model")
            .arg(&self.model);

        let output = cmd
            .output()
            .await
            .with_context(|| format!("Failed to execute agy at {}", self.agy_path.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("agy planning execution failed: {stderr}");
        }

        let raw_stdout = String::from_utf8_lossy(&output.stdout);
        let cleaned = AnthropicProvider::clean_json_response(&raw_stdout);

        let plan: PlanningResponse = serde_json::from_str(cleaned)
            .with_context(|| format!("Failed to parse agy JSON planning output: {cleaned}"))?;

        Ok(plan)
    }

    fn provider_name(&self) -> &str {
        "agy"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
