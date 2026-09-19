//! Audit Logging for Security-Sensitive Actions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::security::secrets::SecretRedactor;

/// Structured record of a security-sensitive event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub status: String,
    pub details: serde_json::Value,
    pub ip_address: Option<String>,
}

impl AuditEvent {
    pub fn new(
        actor_type: impl Into<String>,
        actor_id: Option<String>,
        action: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: Option<String>,
        status: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            actor_type: actor_type.into(),
            actor_id,
            action: action.into(),
            resource_type: resource_type.into(),
            resource_id,
            status: status.into(),
            details: SecretRedactor::redact_json(&details),
            ip_address: None,
        }
    }
}

/// Records security-relevant audit logs into PostgreSQL and tracing logs.
pub struct AuditLogger;

impl AuditLogger {
    /// Inserts an audit event into the database and emits an appropriately-leveled trace.
    pub async fn record(pool: &PgPool, event: &AuditEvent) -> Result<(), sqlx::Error> {
        let is_alert = event.status == "denied" || event.status == "failure";
        if is_alert {
            warn!(
                actor = ?event.actor_id,
                action = %event.action,
                resource = ?event.resource_id,
                status = %event.status,
                details = %event.details,
                "Security Audit Alert"
            );
        } else {
            info!(
                actor = ?event.actor_id,
                action = %event.action,
                resource = ?event.resource_id,
                status = %event.status,
                "Security Audit Event"
            );
        }

        sqlx::query!(
            r#"
            INSERT INTO audit_logs (
                id, timestamp, actor_type, actor_id, action, resource_type,
                resource_id, status, details, ip_address
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            event.id,
            event.timestamp,
            event.actor_type,
            event.actor_id,
            event.action,
            event.resource_type,
            event.resource_id,
            event.status,
            event.details,
            event.ip_address,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Helper to record authentication failure.
    pub async fn log_auth_failure(
        pool: &PgPool,
        agent_id: Uuid,
        reason: &str,
    ) -> Result<(), sqlx::Error> {
        let event = AuditEvent::new(
            "agent",
            Some(agent_id.to_string()),
            "auth_failure",
            "agent",
            Some(agent_id.to_string()),
            "denied",
            serde_json::json!({ "reason": reason }),
        );
        Self::record(pool, &event).await
    }

    /// Helper to record task impersonation attempt.
    pub async fn log_task_impersonation(
        pool: &PgPool,
        actor_agent_id: Uuid,
        task_id: Uuid,
        assigned_to: Option<Uuid>,
        action: &str,
    ) -> Result<(), sqlx::Error> {
        let event = AuditEvent::new(
            "agent",
            Some(actor_agent_id.to_string()),
            "task_impersonation_blocked",
            "task",
            Some(task_id.to_string()),
            "denied",
            serde_json::json!({
                "assigned_agent_id": assigned_to.map(|id| id.to_string()),
                "attempted_action": action,
            }),
        );
        Self::record(pool, &event).await
    }

    /// Helper to record permission boundary violation.
    pub async fn log_permission_denied(
        pool: &PgPool,
        agent_id: Uuid,
        action: &str,
        denied_paths: &[String],
    ) -> Result<(), sqlx::Error> {
        let event = AuditEvent::new(
            "agent",
            Some(agent_id.to_string()),
            "permission_denied",
            "agent",
            Some(agent_id.to_string()),
            "denied",
            serde_json::json!({
                "attempted_action": action,
                "denied_paths": denied_paths,
            }),
        );
        Self::record(pool, &event).await
    }
}
