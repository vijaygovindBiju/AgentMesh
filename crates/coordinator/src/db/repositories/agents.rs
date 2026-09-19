use anyhow::{Context, Result};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AdapterType, Agent, AgentStatus, HealthStatus, NewAgent};

pub struct AgentRepository;

impl AgentRepository {
    /// Registers a new agent. Status defaults to `offline`.
    pub async fn create(pool: &PgPool, new_agent: &NewAgent) -> Result<Agent> {
        let capabilities_json = json!(new_agent.capabilities);
        let profile_json = new_agent.profile.as_ref().map(|p| json!(p));
        let max_concurrency = new_agent.max_concurrency.unwrap_or(1);
        let role = new_agent
            .role
            .clone()
            .unwrap_or_else(|| "worker".to_string());
        let permissions_json = json!(new_agent.permissions.clone().unwrap_or_default());

        let agent = sqlx::query_as!(
            Agent,
            r#"
            INSERT INTO agents (
                human_owner, api_key_hash, adapter_type, capabilities, nats_subject, status,
                capability_profile, max_concurrency, role, permissions, api_key_expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            new_agent.human_owner,
            new_agent.api_key_hash,
            new_agent.adapter_type as AdapterType,
            capabilities_json,
            new_agent.nats_subject,
            AgentStatus::Offline as AgentStatus,
            profile_json,
            max_concurrency,
            role,
            permissions_json,
            new_agent.api_key_expires_at,
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert agent")?;

        Ok(agent)
    }

    /// Registers a new agent with a pre-specified ID. Status defaults to `offline`.
    pub async fn create_with_id(pool: &PgPool, id: Uuid, new_agent: &NewAgent) -> Result<Agent> {
        let capabilities_json = json!(new_agent.capabilities);
        let profile_json = new_agent.profile.as_ref().map(|p| json!(p));
        let max_concurrency = new_agent.max_concurrency.unwrap_or(1);
        let role = new_agent
            .role
            .clone()
            .unwrap_or_else(|| "worker".to_string());
        let permissions_json = json!(new_agent.permissions.clone().unwrap_or_default());

        let agent = sqlx::query_as!(
            Agent,
            r#"
            INSERT INTO agents (
                id, human_owner, api_key_hash, adapter_type, capabilities, nats_subject, status,
                capability_profile, max_concurrency, role, permissions, api_key_expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            new_agent.human_owner,
            new_agent.api_key_hash,
            new_agent.adapter_type as AdapterType,
            capabilities_json,
            new_agent.nats_subject,
            AgentStatus::Offline as AgentStatus,
            profile_json,
            max_concurrency,
            role,
            permissions_json,
            new_agent.api_key_expires_at,
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert agent with id")?;

        Ok(agent)
    }

    /// Finds an agent by primary key ID.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            SELECT
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            FROM agents
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query agent by id")?;

        Ok(agent)
    }

    /// Lists all agents ordered by creation time.
    pub async fn list(pool: &PgPool) -> Result<Vec<Agent>> {
        let agents = sqlx::query_as!(
            Agent,
            r#"
            SELECT
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            FROM agents
            ORDER BY created_at ASC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to list agents")?;

        Ok(agents)
    }

    /// Lists all agents with a specific status (e.g. Idle agents available for assignment).
    pub async fn list_by_status(pool: &PgPool, status: AgentStatus) -> Result<Vec<Agent>> {
        let agents = sqlx::query_as!(
            Agent,
            r#"
            SELECT
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            FROM agents
            WHERE status = $1
            ORDER BY created_at ASC
            "#,
            status as AgentStatus,
        )
        .fetch_all(pool)
        .await
        .context("Failed to list agents by status")?;

        Ok(agents)
    }

    /// Lists all agents that are available to accept new tasks.
    pub async fn list_available(pool: &PgPool) -> Result<Vec<Agent>> {
        let agents = sqlx::query_as!(
            Agent,
            r#"
            SELECT
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            FROM agents
            WHERE status = 'idle'
              AND current_task_id IS NULL
              AND is_draining = FALSE
              AND is_revoked = FALSE
              AND (api_key_expires_at IS NULL OR api_key_expires_at > NOW())
              AND active_tasks_count < max_concurrency
              AND health_status IN ('healthy', 'degraded')
            ORDER BY created_at ASC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to list available agents")?;

        Ok(agents)
    }

    /// Updates the agent's operational status.
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: AgentStatus,
    ) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET status = $2
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            status as AgentStatus,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update agent status")?;

        Ok(agent)
    }

    /// Sets the agent's current task and status simultaneously.
    /// Pass `None` for `task_id` when the agent finishes and becomes idle.
    pub async fn set_current_task(
        pool: &PgPool,
        id: Uuid,
        task_id: Option<Uuid>,
        status: AgentStatus,
    ) -> Result<Option<Agent>> {
        let active_count: i32 = if task_id.is_some() { 1 } else { 0 };

        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET current_task_id = $2,
                status = $3,
                active_tasks_count = $4
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            task_id,
            status as AgentStatus,
            active_count,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to set agent current task")?;

        Ok(agent)
    }

    /// Updates agent structured capability profile.
    pub async fn update_capability_profile(
        pool: &PgPool,
        id: Uuid,
        profile: &agent_protocol::AgentCapabilities,
    ) -> Result<Option<Agent>> {
        let profile_json = json!(profile);
        let tags_json = json!(profile.all_tags());

        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET capability_profile = $2,
                capabilities = $3
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            profile_json,
            tags_json,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update capability profile")?;

        Ok(agent)
    }

    /// Updates health status and diagnostic metrics for an agent.
    pub async fn update_health(
        pool: &PgPool,
        id: Uuid,
        health_status: HealthStatus,
        consecutive_failures: i32,
        last_error: Option<&str>,
        latency_ms: Option<i64>,
    ) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET health_status = $2,
                consecutive_failures = $3,
                last_error = $4,
                heartbeat_latency_ms = $5
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            health_status as HealthStatus,
            consecutive_failures,
            last_error,
            latency_ms,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update agent health")?;

        Ok(agent)
    }

    /// Records successful task completion by this agent.
    pub async fn record_task_completion(pool: &PgPool, id: Uuid) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET tasks_completed_count = tasks_completed_count + 1,
                consecutive_failures = 0,
                health_status = 'healthy'
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to record task completion for agent")?;

        Ok(agent)
    }

    /// Records task failure by this agent and recalculates health status.
    pub async fn record_task_failure(
        pool: &PgPool,
        id: Uuid,
        error_msg: &str,
    ) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET tasks_failed_count = tasks_failed_count + 1,
                consecutive_failures = consecutive_failures + 1,
                last_error = $2,
                health_status = CASE
                    WHEN consecutive_failures + 1 >= 5 THEN 'unhealthy'::agent_health_status
                    WHEN consecutive_failures + 1 >= 2 THEN 'degraded'::agent_health_status
                    ELSE health_status
                END
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            error_msg,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to record task failure for agent")?;

        Ok(agent)
    }

    /// Sets agent draining mode (when true, completes existing tasks without accepting new ones).
    pub async fn set_draining(pool: &PgPool, id: Uuid, draining: bool) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET is_draining = $2
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            draining,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to set agent draining status")?;

        Ok(agent)
    }

    /// Revokes an agent's access immediately.
    pub async fn revoke_agent(pool: &PgPool, id: Uuid) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET is_revoked = TRUE,
                status = 'offline'
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to revoke agent")?;

        Ok(agent)
    }

    /// Un-revokes an agent's access.
    pub async fn unrevoke_agent(pool: &PgPool, id: Uuid) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET is_revoked = FALSE
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to unrevoke agent")?;

        Ok(agent)
    }

    /// Updates agent role (e.g. worker, reviewer, readonly, admin).
    pub async fn update_role(pool: &PgPool, id: Uuid, role: &str) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET role = $2
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            role,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update agent role")?;

        Ok(agent)
    }

    /// Updates agent permission boundary.
    pub async fn update_permissions(
        pool: &PgPool,
        id: Uuid,
        permissions: &agent_protocol::security::PermissionBoundary,
    ) -> Result<Option<Agent>> {
        let perms_json = json!(permissions);
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET permissions = $2
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            perms_json,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to update agent permissions")?;

        Ok(agent)
    }

    /// Rotates an agent's API key with a new hash.
    pub async fn rotate_api_key(
        pool: &PgPool,
        id: Uuid,
        new_api_key_hash: &str,
    ) -> Result<Option<Agent>> {
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET api_key_hash = $2,
                api_key_created_at = NOW(),
                is_revoked = FALSE
            WHERE id = $1
            RETURNING
                id,
                human_owner,
                api_key_hash,
                adapter_type AS "adapter_type: AdapterType",
                capabilities,
                nats_subject,
                status AS "status: AgentStatus",
                current_task_id,
                last_seen,
                created_at,
                capability_profile,
                health_status AS "health_status: HealthStatus",
                consecutive_failures,
                tasks_completed_count,
                tasks_failed_count,
                last_error,
                heartbeat_latency_ms,
                max_concurrency,
                active_tasks_count,
                is_draining,
                role,
                permissions,
                is_revoked,
                api_key_created_at,
                api_key_expires_at
            "#,
            id,
            new_api_key_hash,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to rotate agent API key")?;

        Ok(agent)
    }

    /// Records the agent's heartbeat (updates `last_seen` to now and optionally latency).
    pub async fn record_heartbeat(pool: &PgPool, id: Uuid) -> Result<bool> {
        Self::record_heartbeat_with_latency(pool, id, None).await
    }

    /// Records heartbeat with measured latency.
    pub async fn record_heartbeat_with_latency(
        pool: &PgPool,
        id: Uuid,
        latency_ms: Option<i64>,
    ) -> Result<bool> {
        let rows = sqlx::query!(
            r#"
            UPDATE agents
            SET last_seen = NOW(),
                heartbeat_latency_ms = COALESCE($2, heartbeat_latency_ms),
                health_status = CASE
                    WHEN health_status = 'offline' THEN 'healthy'::agent_health_status
                    ELSE health_status
                END
            WHERE id = $1
            "#,
            id,
            latency_ms
        )
        .execute(pool)
        .await
        .context("Failed to record agent heartbeat")?
        .rows_affected();

        Ok(rows > 0)
    }

    /// Deletes an agent by ID. Returns true if a row was deleted.
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows = sqlx::query!(r#"DELETE FROM agents WHERE id = $1"#, id)
            .execute(pool)
            .await
            .context("Failed to delete agent")?
            .rows_affected();

        Ok(rows > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
        });
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_agent_crud_lifecycle() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // 1. Register agent
        let new_agent = NewAgent {
            human_owner: "Alice".to_string(),
            api_key_hash: "hashed_key".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string(), "python".to_string()],
            nats_subject: "agents.alice-mock.events".to_string(),
            profile: None,
            max_concurrency: Some(1),
            role: Some("worker".to_string()),
            permissions: None,
            api_key_expires_at: None,
        };
        let agent = AgentRepository::create(&pool, &new_agent)
            .await
            .expect("Should create agent");

        assert_eq!(agent.human_owner, "Alice");
        assert_eq!(agent.status, AgentStatus::Offline);
        assert_eq!(agent.adapter_type, AdapterType::Mock);
        assert_eq!(agent.capabilities_list(), vec!["rust", "python"]);
        assert!(agent.current_task_id.is_none());
        assert_eq!(agent.health_status, HealthStatus::Healthy);
        assert!(!agent.is_draining);
        assert!(
            !agent.is_available(),
            "Offline agent should not be available"
        );

        // 2. Find by id
        let found = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .expect("Should query agent")
            .expect("Agent should exist");
        assert_eq!(found.id, agent.id);

        // 3. Update to Idle
        let idled = AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle)
            .await
            .expect("Should update status")
            .expect("Agent returned");
        assert_eq!(idled.status, AgentStatus::Idle);
        assert!(
            idled.is_available(),
            "Idle agent with no task should be available"
        );

        // 4. List idle agents
        let idle_agents = AgentRepository::list_by_status(&pool, AgentStatus::Idle)
            .await
            .expect("Should list idle agents");
        assert!(idle_agents.iter().any(|a| a.id == agent.id));

        // 5. Test availability & draining
        let available = AgentRepository::list_available(&pool)
            .await
            .expect("Should list available agents");
        assert!(available.iter().any(|a| a.id == agent.id));

        AgentRepository::set_draining(&pool, agent.id, true)
            .await
            .unwrap();
        let avail_after_drain = AgentRepository::list_available(&pool).await.unwrap();
        assert!(!avail_after_drain.iter().any(|a| a.id == agent.id));
        AgentRepository::set_draining(&pool, agent.id, false)
            .await
            .unwrap();

        // 6. Test task completion and failure health tracking
        AgentRepository::record_task_completion(&pool, agent.id)
            .await
            .unwrap();
        let agent_after_comp = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent_after_comp.tasks_completed_count, 1);

        AgentRepository::record_task_failure(&pool, agent.id, "Syntax error")
            .await
            .unwrap();
        AgentRepository::record_task_failure(&pool, agent.id, "Type error")
            .await
            .unwrap();
        let agent_after_fail = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent_after_fail.consecutive_failures, 2);
        assert_eq!(agent_after_fail.health_status, HealthStatus::Degraded);

        // 7. Heartbeat with latency
        let heartbeat_ok =
            AgentRepository::record_heartbeat_with_latency(&pool, agent.id, Some(45))
                .await
                .expect("Should record heartbeat");
        assert!(heartbeat_ok);

        // 8. Delete
        let deleted = AgentRepository::delete(&pool, agent.id)
            .await
            .expect("Should delete agent");
        assert!(deleted);

        let not_found = AgentRepository::find_by_id(&pool, agent.id)
            .await
            .expect("Query should succeed");
        assert!(not_found.is_none());
    }
}
