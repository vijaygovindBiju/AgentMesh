//! Repository for managing security audit records in PostgreSQL.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::security::audit::AuditEvent;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditLogRow {
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

impl From<AuditLogRow> for AuditEvent {
    fn from(row: AuditLogRow) -> Self {
        Self {
            id: row.id,
            timestamp: row.timestamp,
            actor_type: row.actor_type,
            actor_id: row.actor_id,
            action: row.action,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            status: row.status,
            details: row.details,
            ip_address: row.ip_address,
        }
    }
}

pub struct AuditRepository;

impl AuditRepository {
    /// Inserts an audit event into the database.
    pub async fn insert(pool: &PgPool, event: &AuditEvent) -> Result<()> {
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
        .await
        .context("Failed to insert audit log entry")?;

        Ok(())
    }

    /// Fetches the most recent audit logs.
    pub async fn find_recent(pool: &PgPool, limit: i64) -> Result<Vec<AuditEvent>> {
        let rows = sqlx::query_as!(
            AuditLogRow,
            r#"
            SELECT
                id, timestamp, actor_type, actor_id, action, resource_type,
                resource_id, status, details, ip_address
            FROM audit_logs
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(pool)
        .await
        .context("Failed to fetch recent audit logs")?;

        Ok(rows.into_iter().map(AuditEvent::from).collect())
    }

    /// Fetches audit logs for a specific actor.
    pub async fn find_by_actor(
        pool: &PgPool,
        actor_type: &str,
        actor_id: &str,
    ) -> Result<Vec<AuditEvent>> {
        let rows = sqlx::query_as!(
            AuditLogRow,
            r#"
            SELECT
                id, timestamp, actor_type, actor_id, action, resource_type,
                resource_id, status, details, ip_address
            FROM audit_logs
            WHERE actor_type = $1 AND actor_id = $2
            ORDER BY timestamp DESC
            LIMIT 100
            "#,
            actor_type,
            actor_id,
        )
        .fetch_all(pool)
        .await
        .context("Failed to fetch audit logs for actor")?;

        Ok(rows.into_iter().map(AuditEvent::from).collect())
    }

    /// Fetches recent security alerts (denied or failure events).
    pub async fn find_alerts(pool: &PgPool, limit: i64) -> Result<Vec<AuditEvent>> {
        let rows = sqlx::query_as!(
            AuditLogRow,
            r#"
            SELECT
                id, timestamp, actor_type, actor_id, action, resource_type,
                resource_id, status, details, ip_address
            FROM audit_logs
            WHERE status IN ('denied', 'failure')
            ORDER BY timestamp DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(pool)
        .await
        .context("Failed to fetch audit alerts")?;

        Ok(rows.into_iter().map(AuditEvent::from).collect())
    }
}
