//! Security subsystem for AgentMesh.
//!
//! Provides agent authentication, role-based authorization, permission boundaries,
//! task impersonation prevention, secret isolation, NATS subject gating, and audit logging.

pub mod audit;
pub mod auth;
pub mod nats_security;
pub mod permissions;
pub mod secrets;
pub mod task_auth;

use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Agent key is revoked: {0}")]
    AgentRevoked(Uuid),

    #[error("Agent key is expired: {0}")]
    AgentKeyExpired(Uuid),

    #[error("Unauthorized action '{action}' for agent {agent_id}: {reason}")]
    UnauthorizedAction {
        agent_id: Uuid,
        action: String,
        reason: String,
    },

    #[error("Permission boundary violation: agent {agent_id} attempted to access denied paths: {paths:?}")]
    PermissionBoundaryViolation {
        agent_id: Uuid,
        paths: Vec<String>,
    },

    #[error("Task impersonation detected: agent {actor_agent_id} attempted action '{action}' on task {task_id}, but task is assigned to {assigned_to:?}")]
    TaskImpersonation {
        actor_agent_id: Uuid,
        task_id: Uuid,
        assigned_to: Option<Uuid>,
        action: String,
    },

    #[error("Database error in security subsystem: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Internal error in security subsystem: {0}")]
    Anyhow(#[from] anyhow::Error),
}
