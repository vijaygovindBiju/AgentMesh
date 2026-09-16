# ADR 006 — LLM Provider Abstraction

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

The coordinator needs to call a large language model to decompose project descriptions into dependency-aware task plans. Multiple LLM providers exist (Anthropic, OpenAI, Gemini), and the system must not be coupled to any single one.

Requirements:
- The coordinator core must not import any provider-specific SDK or HTTP client code.
- The provider must be selectable at runtime via environment configuration.
- The interface must be testable without real API keys (mock provider for tests).
- The input/output contract must be independent of the provider's API format.

---

## Decision

**Define a `LlmProvider` trait in the coordinator. Select the concrete implementation at startup from environment variables. The coordinator core depends only on the trait.**

---

## Trait Design

```rust
// coordinator/src/ai/provider.rs

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Decompose a project description into a proposed task plan.
    async fn propose_plan(
        &self,
        project: &ProjectContext,
        agents: &[AgentContext],
    ) -> Result<ProposedPlan>;

    /// Provider name for logging and audit records.
    fn provider_name(&self) -> &str;

    /// Model identifier for logging and audit records.
    fn model_name(&self) -> &str;
}

pub struct ProjectContext {
    pub name: String,
    pub description: String,
}

pub struct AgentContext {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub capabilities: Vec<String>,
}

pub struct ProposedPlan {
    pub tasks: Vec<ProposedTask>,
    pub dependencies: Vec<ProposedDependency>,
    pub reasoning: String,   // LLM's explanation of the plan
}

pub struct ProposedTask {
    pub title: String,
    pub description: String,
    pub suggested_agent_id: Option<Uuid>,
    pub affected_resources: Vec<String>,
    pub estimated_size: Option<String>,
}

pub struct ProposedDependency {
    pub dependent_title: String,    // matched by title to ProposedTask
    pub depends_on_title: String,
    pub reason: String,
}
```

---

## Runtime Configuration

```bash
AI_PROVIDER=anthropic     # anthropic | openai | gemini
AI_MODEL=claude-3-5-sonnet-20241022
ANTHROPIC_API_KEY=sk-...
```

At startup, the coordinator reads `AI_PROVIDER` and constructs the appropriate implementation:

```rust
let provider: Box<dyn LlmProvider> = match config.ai_provider.as_str() {
    "anthropic" => Box::new(AnthropicProvider::new(&config)?),
    "openai"    => Box::new(OpenAiProvider::new(&config)?),
    "gemini"    => Box::new(GeminiProvider::new(&config)?),
    other => bail!("Unknown AI_PROVIDER: {other}"),
};
```

The `provider` value is passed into the coordinator core as `Arc<dyn LlmProvider>`. No provider-specific code exists in the coordinator logic.

---

## Structured Output

All providers must return a structured `ProposedPlan`. The prompt instructs the LLM to respond in JSON matching the schema. If the LLM response is malformed:

1. Log the raw response for debugging.
2. Return an error to the coordinator state machine.
3. The coordinator surfaces the error in the TUI: "AI planning failed — see logs."
4. Human can retry or provide an updated description.

The coordinator never crashes on a malformed LLM response.

---

## Test / Mock Provider

A `MockLlmProvider` is implemented in the coordinator crate (behind `#[cfg(test)]` or a feature flag):

```rust
pub struct MockLlmProvider {
    pub plan: ProposedPlan,
}

impl LlmProvider for MockLlmProvider {
    async fn propose_plan(&self, _project: &ProjectContext, _agents: &[AgentContext]) -> Result<ProposedPlan> {
        Ok(self.plan.clone())
    }
    fn provider_name(&self) -> &str { "mock" }
    fn model_name(&self) -> &str { "mock-v1" }
}
```

This allows integration tests to run without API keys and with deterministic plans.

---

## Consequences

- Each provider implementation handles its own prompt construction, HTTP call, and JSON parsing. The coordinator core sees only `ProposedPlan`.
- Adding a new provider requires implementing `LlmProvider` in a new file — no changes to coordinator core.
- The `ProposedPlan` schema is the stable interface. If the LLM prompt changes, it must still produce a `ProposedPlan`-compatible JSON structure.
- The `reasoning` field is stored in the `Proposal` table for auditability (humans can understand why the AI proposed what it did).
