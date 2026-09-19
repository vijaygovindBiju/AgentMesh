//! Structured Logging and Correlation ID Context.

use tracing::{info_span, Span};
use uuid::Uuid;

/// Request / task correlation context for tracing distributed actions.
#[derive(Debug, Clone, Default)]
pub struct TraceContext {
    pub trace_id: String,
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
}

impl TraceContext {
    /// Creates a new correlation context with a random trace_id.
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4().simple().to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
        }
    }

    pub fn with_project(mut self, project_id: Uuid) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub fn with_task(mut self, task_id: Uuid) -> Self {
        self.task_id = Some(task_id);
        self
    }

    pub fn with_agent(mut self, agent_id: Uuid) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    /// Creates an instrumented tracing span with correlation fields.
    pub fn span(&self, name: &'static str) -> Span {
        info_span!(
            "agentmesh_op",
            op = name,
            trace_id = %self.trace_id,
            project_id = ?self.project_id.map(|id| id.to_string()),
            task_id = ?self.task_id.map(|id| id.to_string()),
            agent_id = ?self.agent_id.map(|id| id.to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_span_creation() {
        let task_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();

        let ctx = TraceContext::new()
            .with_task(task_id)
            .with_agent(agent_id);

        let span = ctx.span("execute_task");
        let _enter = span.enter();
        tracing::info!("Tracing correlated action inside span");
    }
}
