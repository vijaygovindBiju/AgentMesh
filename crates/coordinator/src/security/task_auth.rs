//! Task Authorization and Impersonation Prevention.

use sqlx::PgPool;
use uuid::Uuid;

use crate::db::repositories::{AgentRepository, TaskRepository};
use crate::security::SecurityError;

/// Validates that an agent has explicit authority to act on a specific task.
pub struct TaskAuthorizer;

impl TaskAuthorizer {
    /// Verifies that `agent_id` is the legitimate assigned agent for `task_id`.
    /// Prevents malicious or misconfigured agents from reporting completion/failure for tasks they do not own.
    pub async fn authorize_agent_for_task(
        pool: &PgPool,
        agent_id: Uuid,
        task_id: Uuid,
        action: &str,
    ) -> Result<(), SecurityError> {
        // 1. Verify agent is not revoked
        if let Some(agent) = AgentRepository::find_by_id(pool, agent_id).await? {
            if agent.is_revoked {
                return Err(SecurityError::AgentRevoked(agent_id));
            }
        }

        // 2. Fetch task and check assigned agent
        let task = TaskRepository::find_by_id(pool, task_id)
            .await?
            .ok_or_else(|| SecurityError::UnauthorizedAction {
                agent_id,
                action: action.to_string(),
                reason: format!("Task {task_id} not found"),
            })?;

        if task.assigned_agent_id != Some(agent_id) {
            return Err(SecurityError::TaskImpersonation {
                actor_agent_id: agent_id,
                task_id,
                assigned_to: task.assigned_agent_id,
                action: action.to_string(),
            });
        }

        Ok(())
    }
}
