use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{AckKind, DeliveryStatus, NewTaskDelivery, TaskDelivery};

pub struct TaskDeliveryRepository;

impl TaskDeliveryRepository {
    /// Creates a `TaskDelivery` record with status `Pending` before publishing to NATS.
    /// Must be created first so a coordinator crash between record creation
    /// and NATS publish can be recovered on restart by scanning `Pending` records.
    pub async fn create(pool: &PgPool, new: &NewTaskDelivery) -> Result<TaskDelivery> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            INSERT INTO task_deliveries (
                task_id, agent_id, attempt, nats_stream, nats_subject,
                idempotency_key, expires_at, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING
                id,
                task_id,
                agent_id,
                attempt,
                nats_stream,
                nats_subject,
                nats_sequence,
                idempotency_key,
                delivered_at,
                acknowledged_at,
                ack_kind AS "ack_kind: AckKind",
                expires_at,
                status AS "status: DeliveryStatus",
                failure_reason,
                reassigned_to
            "#,
            new.task_id,
            new.agent_id,
            new.attempt,
            new.nats_stream,
            new.nats_subject,
            new.idempotency_key,
            new.expires_at,
            DeliveryStatus::Pending as DeliveryStatus,
        )
        .fetch_one(pool)
        .await
        .context("Failed to create task delivery")?;

        Ok(delivery)
    }

    /// Finds a delivery record by primary key.
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<TaskDelivery>> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            SELECT
                id,
                task_id,
                agent_id,
                attempt,
                nats_stream,
                nats_subject,
                nats_sequence,
                idempotency_key,
                delivered_at,
                acknowledged_at,
                ack_kind AS "ack_kind: AckKind",
                expires_at,
                status AS "status: DeliveryStatus",
                failure_reason,
                reassigned_to
            FROM task_deliveries
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query task delivery by id")?;

        Ok(delivery)
    }

    /// Finds a delivery by idempotency key.
    /// Agents use this to detect and skip already-processed messages.
    pub async fn find_by_idempotency_key(
        pool: &PgPool,
        key: &str,
    ) -> Result<Option<TaskDelivery>> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            SELECT
                id,
                task_id,
                agent_id,
                attempt,
                nats_stream,
                nats_subject,
                nats_sequence,
                idempotency_key,
                delivered_at,
                acknowledged_at,
                ack_kind AS "ack_kind: AckKind",
                expires_at,
                status AS "status: DeliveryStatus",
                failure_reason,
                reassigned_to
            FROM task_deliveries
            WHERE idempotency_key = $1
            "#,
            key
        )
        .fetch_optional(pool)
        .await
        .context("Failed to query task delivery by idempotency key")?;

        Ok(delivery)
    }

    /// Lists all deliveries for a given task ordered by attempt ascending.
    pub async fn list_by_task(pool: &PgPool, task_id: Uuid) -> Result<Vec<TaskDelivery>> {
        let deliveries = sqlx::query_as!(
            TaskDelivery,
            r#"
            SELECT
                id,
                task_id,
                agent_id,
                attempt,
                nats_stream,
                nats_subject,
                nats_sequence,
                idempotency_key,
                delivered_at,
                acknowledged_at,
                ack_kind AS "ack_kind: AckKind",
                expires_at,
                status AS "status: DeliveryStatus",
                failure_reason,
                reassigned_to
            FROM task_deliveries
            WHERE task_id = $1
            ORDER BY attempt ASC
            "#,
            task_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to list task deliveries")?;

        Ok(deliveries)
    }

    /// Records that the NATS message was published; advances status to `Delivered`.
    pub async fn mark_delivered(
        pool: &PgPool,
        id: Uuid,
        nats_sequence: i64,
    ) -> Result<Option<TaskDelivery>> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            UPDATE task_deliveries
            SET status = $2, nats_sequence = $3
            WHERE id = $1
            RETURNING
                id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                idempotency_key, delivered_at,
                acknowledged_at, ack_kind AS "ack_kind: AckKind",
                expires_at, status AS "status: DeliveryStatus",
                failure_reason, reassigned_to
            "#,
            id,
            DeliveryStatus::Delivered as DeliveryStatus,
            nats_sequence,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to mark delivery as delivered")?;

        Ok(delivery)
    }

    /// Records the agent's acknowledgement (ACK / NAK / Term).
    pub async fn record_ack(
        pool: &PgPool,
        id: Uuid,
        ack_kind: AckKind,
        new_status: DeliveryStatus,
    ) -> Result<Option<TaskDelivery>> {
        let now: DateTime<Utc> = Utc::now();
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            UPDATE task_deliveries
            SET ack_kind = $2, status = $3, acknowledged_at = $4
            WHERE id = $1
            RETURNING
                id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                idempotency_key, delivered_at,
                acknowledged_at, ack_kind AS "ack_kind: AckKind",
                expires_at, status AS "status: DeliveryStatus",
                failure_reason, reassigned_to
            "#,
            id,
            ack_kind as AckKind,
            new_status as DeliveryStatus,
            now,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to record delivery ack")?;

        Ok(delivery)
    }

    /// Marks a delivery as Terminal with a failure reason.
    /// The coordinator then sets the task to Failed and notifies the human.
    pub async fn mark_terminal(
        pool: &PgPool,
        id: Uuid,
        reason: &str,
    ) -> Result<Option<TaskDelivery>> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            UPDATE task_deliveries
            SET status = $2, failure_reason = $3
            WHERE id = $1
            RETURNING
                id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                idempotency_key, delivered_at,
                acknowledged_at, ack_kind AS "ack_kind: AckKind",
                expires_at, status AS "status: DeliveryStatus",
                failure_reason, reassigned_to
            "#,
            id,
            DeliveryStatus::Terminal as DeliveryStatus,
            reason,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to mark delivery as terminal")?;

        Ok(delivery)
    }

    /// Marks a delivery as Reassigned to a new agent.
    /// The old record is closed (immutable audit trail).
    pub async fn mark_reassigned(
        pool: &PgPool,
        id: Uuid,
        new_agent_id: Uuid,
    ) -> Result<Option<TaskDelivery>> {
        let delivery = sqlx::query_as!(
            TaskDelivery,
            r#"
            UPDATE task_deliveries
            SET status = $2, reassigned_to = $3
            WHERE id = $1
            RETURNING
                id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                idempotency_key, delivered_at,
                acknowledged_at, ack_kind AS "ack_kind: AckKind",
                expires_at, status AS "status: DeliveryStatus",
                failure_reason, reassigned_to
            "#,
            id,
            DeliveryStatus::Reassigned as DeliveryStatus,
            new_agent_id,
        )
        .fetch_optional(pool)
        .await
        .context("Failed to mark delivery as reassigned")?;

        Ok(delivery)
    }

    /// Returns all Pending deliveries (for crash-recovery on coordinator restart).
    pub async fn list_pending(pool: &PgPool) -> Result<Vec<TaskDelivery>> {
        let deliveries = sqlx::query_as!(
            TaskDelivery,
            r#"
            SELECT
                id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                idempotency_key, delivered_at,
                acknowledged_at, ack_kind AS "ack_kind: AckKind",
                expires_at, status AS "status: DeliveryStatus",
                failure_reason, reassigned_to
            FROM task_deliveries
            WHERE status = 'pending'
            ORDER BY delivered_at ASC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to list pending deliveries")?;

        Ok(deliveries)
    }

    /// Returns all deliveries that have expired (`expires_at < NOW()`) where the task
    /// is still in `Assigned` state (i.e. agent was assigned/delivered/acked the task
    /// but crashed before publishing `TaskStarted` or beginning execution).
    pub async fn list_expired_unstarted(pool: &PgPool) -> Result<Vec<TaskDelivery>> {
        let deliveries = sqlx::query_as!(
            TaskDelivery,
            r#"
            SELECT
                td.id, td.task_id, td.agent_id, td.attempt, td.nats_stream, td.nats_subject, td.nats_sequence,
                td.idempotency_key, td.delivered_at,
                td.acknowledged_at, td.ack_kind AS "ack_kind: AckKind",
                td.expires_at, td.status AS "status: DeliveryStatus",
                td.failure_reason, td.reassigned_to
            FROM task_deliveries td
            JOIN tasks t ON td.task_id = t.id
            WHERE td.status IN ('delivered', 'acknowledged')
              AND td.expires_at < NOW()
              AND t.status = 'assigned'
            ORDER BY td.expires_at ASC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to list expired unstarted deliveries")?;

        Ok(deliveries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::repositories::agents::AgentRepository;
    use crate::db::repositories::projects::ProjectRepository;
    use crate::db::repositories::proposals::ProposalRepository;
    use crate::db::repositories::tasks::TaskRepository;
    use crate::domain::{AdapterType, AgentStatus, NewAgent, NewProject, NewProposal, NewTask, TaskStatus};

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_delivery_lifecycle_and_idempotency() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // 1. Setup Project, Proposal, Agent, Task
        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Delivery Test Proj".to_string(),
                description: "Testing deliveries".to_string(),
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

        let agent1 = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "Bob".to_string(),
                api_key_hash: "hash1".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.bob.events".to_string(),
            },
        )
        .await
        .expect("Agent 1 creation failed");

        let agent2 = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "Charlie".to_string(),
                api_key_hash: "hash2".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.charlie.events".to_string(),
            },
        )
        .await
        .expect("Agent 2 creation failed");

        let task = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "DELIV-001".to_string(),
                title: "Delivery target task".to_string(),
                description: "Testing delivery semantics".to_string(),
                affected_resources: vec![],
                estimated_size: None,
                proposal_id: proposal.id,
            },
        )
        .await
        .expect("Task creation failed");

        // 2. Create TaskDelivery attempt 1 (Pending)
        let key1 = TaskDelivery::make_idempotency_key(task.id, 1);
        let new_delivery = NewTaskDelivery {
            task_id: task.id,
            agent_id: agent1.id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{}", agent1.id),
            idempotency_key: key1.clone(),
            expires_at: Utc::now() + Duration::seconds(60),
        };

        let delivery = TaskDeliveryRepository::create(&pool, &new_delivery)
            .await
            .expect("TaskDelivery create failed");
        assert_eq!(delivery.status, DeliveryStatus::Pending);
        assert_eq!(delivery.attempt, 1);

        // 3. Crash recovery check: list_pending includes it
        let pending = TaskDeliveryRepository::list_pending(&pool)
            .await
            .expect("List pending failed");
        assert!(pending.iter().any(|d| d.id == delivery.id));

        // 4. Mark delivered
        let delivered = TaskDeliveryRepository::mark_delivered(&pool, delivery.id, 42)
            .await
            .expect("Mark delivered failed")
            .expect("Delivery returned");
        assert_eq!(delivered.status, DeliveryStatus::Delivered);
        assert_eq!(delivered.nats_sequence, Some(42));

        // 5. Record ACK
        let acked = TaskDeliveryRepository::record_ack(&pool, delivery.id, AckKind::Ack, DeliveryStatus::Acknowledged)
            .await
            .expect("Record ack failed")
            .expect("Delivery returned");
        assert_eq!(acked.status, DeliveryStatus::Acknowledged);
        assert_eq!(acked.ack_kind, Some(AckKind::Ack));
        assert!(acked.acknowledged_at.is_some());

        // 6. Idempotency lookup
        let found = TaskDeliveryRepository::find_by_idempotency_key(&pool, &key1)
            .await
            .expect("Idempotency lookup failed")
            .expect("Delivery found");
        assert_eq!(found.id, delivery.id);

        // 7. Mark reassigned to agent2
        let reassigned = TaskDeliveryRepository::mark_reassigned(&pool, delivery.id, agent2.id)
            .await
            .expect("Mark reassigned failed")
            .expect("Delivery returned");
        assert_eq!(reassigned.status, DeliveryStatus::Reassigned);
        assert_eq!(reassigned.reassigned_to, Some(agent2.id));

        // 8. List by task
        let list = TaskDeliveryRepository::list_by_task(&pool, task.id)
            .await
            .expect("List by task failed");
        assert_eq!(list.len(), 1);

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
        AgentRepository::delete(&pool, agent1.id).await.unwrap();
        AgentRepository::delete(&pool, agent2.id).await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_crash_after_ack_before_task_started_recovery() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        // 1. Setup Project, Proposal, Agent A and Agent B
        let project = ProjectRepository::create(
            &pool,
            &NewProject {
                name: "Crash Recovery Proj".to_string(),
                description: "Testing crash window".to_string(),
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

        let agent_a = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "AgentA-Owner".to_string(),
                api_key_hash: "hash_a".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.a.events".to_string(),
            },
        )
        .await
        .expect("Agent A creation failed");

        let agent_b = AgentRepository::create(
            &pool,
            &NewAgent {
                human_owner: "AgentB-Owner".to_string(),
                api_key_hash: "hash_b".to_string(),
                adapter_type: AdapterType::Mock,
                capabilities: vec!["rust".to_string()],
                nats_subject: "agents.b.events".to_string(),
            },
        )
        .await
        .expect("Agent B creation failed");

        let task = TaskRepository::create(
            &pool,
            &NewTask {
                project_id: project.id,
                short_id: "CRASH-001".to_string(),
                title: "Vulnerable task".to_string(),
                description: "Agent crashes after ACK".to_string(),
                affected_resources: vec!["src/lib.rs".to_string()],
                estimated_size: Some("S".to_string()),
                proposal_id: proposal.id,
            },
        )
        .await
        .expect("Task creation failed");

        // 2. Human approves task -> Coordinator assigns task to Agent A
        TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
        TaskRepository::assign_agent(&pool, task.id, Some(agent_a.id)).await.unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

        // 3. Coordinator creates TaskDelivery (attempt 1) with an expiration in the past
        // (simulating that the timeout has expired without the agent reporting TaskStarted)
        let key1 = TaskDelivery::make_idempotency_key(task.id, 1);
        let delivery1 = TaskDeliveryRepository::create(
            &pool,
            &NewTaskDelivery {
                task_id: task.id,
                agent_id: agent_a.id,
                attempt: 1,
                nats_stream: "TASK_ASSIGNMENTS".to_string(),
                nats_subject: format!("coordinator.tasks.assign.{}", agent_a.id),
                idempotency_key: key1.clone(),
                expires_at: Utc::now() - Duration::seconds(5), // Already expired!
            },
        )
        .await
        .expect("Delivery 1 creation failed");

        // 4. Agent A receives message and sends JetStream ACK, updating delivery to Acknowledged...
        TaskDeliveryRepository::record_ack(&pool, delivery1.id, AckKind::Ack, DeliveryStatus::Acknowledged)
            .await
            .unwrap();

        // ...BUT AGENT A CRASHES HERE!
        // Agent A NEVER publishes TaskStarted to AGENT_EVENTS.
        // Task.status is STILL Assigned (not Executing!).

        let current_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
        assert_eq!(current_task.status, TaskStatus::Assigned, "Task must still be Assigned because TaskStarted was never received");

        // 5. Coordinator's background sweep detects expired unstarted deliveries
        let expired_unstarted = TaskDeliveryRepository::list_expired_unstarted(&pool)
            .await
            .expect("Detection sweep failed");

        assert!(
            expired_unstarted.iter().any(|d| d.id == delivery1.id),
            "Expired unstarted delivery for Agent A must be caught by sweep"
        );

        // 6. Coordinator executes Recovery Path:
        // a) Mark Delivery 1 as Terminal
        let terminal_delivery = TaskDeliveryRepository::mark_terminal(
            &pool,
            delivery1.id,
            "Agent crashed after ACK before TaskStarted (delivery expired)",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(terminal_delivery.status, DeliveryStatus::Terminal);

        // b) Mark Task as Failed (allowing human review / reassignment)
        let failed_task = TaskRepository::update_status(&pool, task.id, TaskStatus::Failed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed_task.status, TaskStatus::Failed);

        // c) Mark crashed Agent A as Offline
        AgentRepository::set_current_task(&pool, agent_a.id, None, AgentStatus::Offline)
            .await
            .unwrap();

        // 7. Reassignment Path (Human approves reassignment to Agent B):
        // a) Task returns to Approved, then Assigned to Agent B
        TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
        TaskRepository::assign_agent(&pool, task.id, Some(agent_b.id)).await.unwrap();
        TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

        // b) Mark old delivery as Reassigned to Agent B
        TaskDeliveryRepository::mark_reassigned(&pool, delivery1.id, agent_b.id)
            .await
            .unwrap();

        // c) Create new TaskDelivery (attempt 2) for Agent B
        let key2 = TaskDelivery::make_idempotency_key(task.id, 2);
        assert_ne!(key1, key2, "Attempt 2 must have a distinct idempotency key");

        let delivery2 = TaskDeliveryRepository::create(
            &pool,
            &NewTaskDelivery {
                task_id: task.id,
                agent_id: agent_b.id,
                attempt: 2,
                nats_stream: "TASK_ASSIGNMENTS".to_string(),
                nats_subject: format!("coordinator.tasks.assign.{}", agent_b.id),
                idempotency_key: key2.clone(),
                expires_at: Utc::now() + Duration::seconds(60),
            },
        )
        .await
        .expect("Delivery 2 creation failed");

        assert_eq!(delivery2.attempt, 2);

        // 8. Agent B succeeds:
        // a) Agent B ACKs
        TaskDeliveryRepository::record_ack(&pool, delivery2.id, AckKind::Ack, DeliveryStatus::Acknowledged)
            .await
            .unwrap();

        // b) Agent B sends TaskStarted
        TaskRepository::update_status(&pool, task.id, TaskStatus::Executing).await.unwrap();

        // c) Agent B finishes work and sends Completed
        let completed_task = TaskRepository::update_status(&pool, task.id, TaskStatus::Completed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed_task.status, TaskStatus::Completed);

        // 9. Verify final audit state in database:
        let all_deliveries = TaskDeliveryRepository::list_by_task(&pool, task.id).await.unwrap();
        assert_eq!(all_deliveries.len(), 2);
        assert_eq!(all_deliveries[0].attempt, 1);
        assert_eq!(all_deliveries[0].status, DeliveryStatus::Reassigned);
        assert_eq!(all_deliveries[0].reassigned_to, Some(agent_b.id));
        assert_eq!(all_deliveries[1].attempt, 2);
        assert_eq!(all_deliveries[1].status, DeliveryStatus::Acknowledged);

        // Clean up
        ProjectRepository::delete(&pool, project.id).await.unwrap();
        AgentRepository::delete(&pool, agent_a.id).await.unwrap();
        AgentRepository::delete(&pool, agent_b.id).await.unwrap();
    }
}

