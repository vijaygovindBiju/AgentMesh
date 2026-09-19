use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AgentEvent, AgentEventType, NewAgentEvent};

pub struct AgentEventRepository;

impl AgentEventRepository {
    /// Records an agent lifecycle event. Append-only — never update or delete.
    pub async fn create(pool: &PgPool, new: &NewAgentEvent) -> Result<AgentEvent> {
        let event = sqlx::query_as!(
            AgentEvent,
            r#"
            INSERT INTO agent_events (agent_id, task_id, event_type, message, payload)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING
                id,
                agent_id,
                task_id,
                event_type AS "event_type: AgentEventType",
                message,
                payload,
                received_at
            "#,
            new.agent_id,
            new.task_id,
            new.event_type as AgentEventType,
            new.message,
            new.payload,
        )
        .fetch_one(pool)
        .await
        .context("Failed to record agent event")?;

        Ok(event)
    }

    /// Returns all events for a specific task, ordered chronologically.
    pub async fn list_by_task(pool: &PgPool, task_id: Uuid) -> Result<Vec<AgentEvent>> {
        let events = sqlx::query_as!(
            AgentEvent,
            r#"
            SELECT
                id,
                agent_id,
                task_id,
                event_type AS "event_type: AgentEventType",
                message,
                payload,
                received_at
            FROM agent_events
            WHERE task_id = $1
            ORDER BY received_at ASC
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list agent events by task")?;

        Ok(events)
    }

    /// Returns all events from a specific agent, ordered chronologically.
    pub async fn list_by_agent(pool: &PgPool, agent_id: Uuid) -> Result<Vec<AgentEvent>> {
        let events = sqlx::query_as!(
            AgentEvent,
            r#"
            SELECT
                id,
                agent_id,
                task_id,
                event_type AS "event_type: AgentEventType",
                message,
                payload,
                received_at
            FROM agent_events
            WHERE agent_id = $1
            ORDER BY received_at ASC
            "#,
            agent_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list agent events by agent")?;

        Ok(events)
    }

    /// Returns the most recent event of a given type for a task.
    pub async fn latest_for_task(
        pool: &PgPool,
        task_id: Uuid,
        event_type: AgentEventType,
    ) -> Result<Option<AgentEvent>> {
        let event = sqlx::query_as!(
            AgentEvent,
            r#"
            SELECT
                id,
                agent_id,
                task_id,
                event_type AS "event_type: AgentEventType",
                message,
                payload,
                received_at
            FROM agent_events
            WHERE task_id = $1 AND event_type = $2
            ORDER BY received_at DESC
            LIMIT 1
            "#,
            task_id,
            event_type as AgentEventType,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query latest agent event")?;

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::agents::AgentRepository;
    use crate::db::repositories::projects::ProjectRepository;
    use crate::db::repositories::proposals::ProposalRepository;
    use crate::db::repositories::tasks::TaskRepository;
    use crate::domain::{AdapterType, NewAgent, NewProject, NewProposal, NewTask};

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_agent_event_recording_and_queries() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Event Test Proj".to_string(),
                description: "Testing events".to_string(),
            },
        )
        .await
        .expect("Project creation failed");

        let proposal = ProposalRepository::create(
            &pool,
            &NewProposal {
                project_id: project.id,
                ai_provider: "anthropic".to_string(),
                ai_model: "claude-3-5-sonnet".to_string(),
                raw_prompt: "plan".to_string(),
                raw_response: "{}".to_string(),
            },
        )
        .await
        .expect("Proposal creation failed");

        let agent = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "Dave".to_string(),
                api_key_hash: "hash_dave".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.dave.events".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("Agent creation failed");

        let task = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "EVENT-001".to_string(),
                title: "Event test task".to_string(),
                description: "Testing events".to_string(),
                affected_resources: vec![],
                estimated_size: None,
                proposal_id: proposal.id,
            },
        )
        .await
        .expect("Task creation failed");

        // 1. Record TaskStarted event
        let ev1 = AgentEventRepository::create(
            &pool,
            &NewAgentEvent {
                agent_id: agent.id,
                task_id: task.id,
                event_type: AgentEventType::TaskStarted,
                message: Some("Agent started work".to_string()),
                payload: json!({"status": "starting"}),
            },
        )
        .await
        .expect("Event 1 create failed");
        assert_eq!(ev1.event_type, AgentEventType::TaskStarted);

        // 2. Record ProgressUpdate event
        let _ev2 = AgentEventRepository::create(
            &pool,
            &NewAgentEvent {
                agent_id: agent.id,
                task_id: task.id,
                event_type: AgentEventType::ProgressUpdate,
                message: Some("50% complete".to_string()),
                payload: json!({"percent": 50}),
            },
        )
        .await
        .expect("Event 2 create failed");

        // 3. List by task
        let task_events = AgentEventRepository::list_by_task(&pool, task.id)
            .await
            .expect("List by task failed");
        assert_eq!(task_events.len(), 2);

        // 4. List by agent
        let agent_events = AgentEventRepository::list_by_agent(&pool, agent.id)
            .await
            .expect("List by agent failed");
        assert_eq!(agent_events.len(), 2);

        // 5. Latest for task
        let latest_progress = AgentEventRepository::latest_for_task(&pool, task.id, AgentEventType::ProgressUpdate)
            .await
            .expect("Query latest failed")
            .expect("Should find latest progress event");
        assert_eq!(latest_progress.message, Some("50% complete".to_string()));

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
        AgentRepository::delete(&pool, agent.id).await.unwrap();
    }
}

