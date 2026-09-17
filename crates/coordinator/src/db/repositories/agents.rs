use anyhow::{Context, Result};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AdapterType, Agent, AgentStatus, NewAgent};

pub struct AgentRepository;

impl AgentRepository {
    /// Registers a new agent. Status defaults to `offline`.
    pub async fn create(pool: &PgPool, new_agent: &NewAgent) -> Result<Agent> {
        let capabilities_json = json!(new_agent.capabilities);

        let agent = sqlx::query_as!(
            Agent,
            r#"
            INSERT INTO agents (human_owner, api_key_hash, adapter_type, capabilities, nats_subject, status)
            VALUES ($1, $2, $3, $4, $5, $6)
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
                created_at
            "#,
            new_agent.human_owner,
            new_agent.api_key_hash,
            new_agent.adapter_type as AdapterType,
            capabilities_json,
            new_agent.nats_subject,
            AgentStatus::Offline as AgentStatus,
        )
        .fetch_one(pool)
        .await
        .context("Failed to insert agent")?;

        Ok(agent)
    }

    /// Registers a new agent with a pre-specified ID. Status defaults to `offline`.
    pub async fn create_with_id(
        pool: &PgPool,
        id: Uuid,
        new_agent: &NewAgent,
    ) -> Result<Agent> {
        let capabilities_json = json!(new_agent.capabilities);

        let agent = sqlx::query_as!(
            Agent,
            r#"
            INSERT INTO agents (id, human_owner, api_key_hash, adapter_type, capabilities, nats_subject, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
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
                created_at
            "#,
            id,
            new_agent.human_owner,
            new_agent.api_key_hash,
            new_agent.adapter_type as AdapterType,
            capabilities_json,
            new_agent.nats_subject,
            AgentStatus::Offline as AgentStatus,
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
                created_at
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
                created_at
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
                created_at
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
                created_at
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
        let agent = sqlx::query_as!(
            Agent,
            r#"
            UPDATE agents
            SET current_task_id = $2, status = $3
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
                created_at
            "#,
            id,
            task_id,
            status as AgentStatus,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to set agent current task")?;

        Ok(agent)
    }

    /// Records the agent's heartbeat (updates `last_seen` to now).
    pub async fn record_heartbeat(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows = sqlx::query!(
            r#"
            UPDATE agents
            SET last_seen = NOW()
            WHERE id = $1
            "#,
            id
        )
        .execute(pool)
        .await
        .context("Failed to record agent heartbeat")?
        .rows_affected();

        Ok(rows > 0)
    }

    /// Deletes an agent by ID. Returns true if a row was deleted.
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool> {
        let rows = sqlx::query!(
            r#"DELETE FROM agents WHERE id = $1"#,
            id
        )
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
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
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
        };
        let agent = AgentRepository::create(&pool, &new_agent)
            .await
            .expect("Should create agent");

        assert_eq!(agent.human_owner, "Alice");
        assert_eq!(agent.status, AgentStatus::Offline);
        assert_eq!(agent.adapter_type, AdapterType::Mock);
        assert_eq!(agent.capabilities_list(), vec!["rust", "python"]);
        assert!(agent.current_task_id.is_none());
        assert!(!agent.is_available(), "Offline agent should not be available");

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
        assert!(idled.is_available(), "Idle agent with no task should be available");

        // 4. List idle agents
        let idle_agents = AgentRepository::list_by_status(&pool, AgentStatus::Idle)
            .await
            .expect("Should list idle agents");
        assert!(idle_agents.iter().any(|a| a.id == agent.id));

        // 5. Heartbeat
        let heartbeat_ok = AgentRepository::record_heartbeat(&pool, agent.id)
            .await
            .expect("Should record heartbeat");
        assert!(heartbeat_ok);

        // 6. Delete
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
