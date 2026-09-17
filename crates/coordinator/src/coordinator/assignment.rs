use anyhow::{Context, Result};
use async_nats::jetstream::Context as JetStreamContext;
use chrono::Utc;
use sqlx::PgPool;
use tracing::{error, info, warn};
use uuid::Uuid;

use agent_protocol::TaskSpec;
use crate::db::repositories::{
    AgentRepository, TaskDeliveryRepository, TaskRepository,
};
use crate::domain::{
    AdapterType, Agent, AgentStatus, DeliveryStatus, Task, TaskStatus,
};
use crate::messaging::publisher::TaskPublisher;

#[derive(Debug, Clone)]
pub struct AssignmentResult {
    pub task_id: Uuid,
    pub agent_id: Uuid,
    pub delivery_id: Uuid,
    pub idempotency_key: String,
    pub nats_sequence: Option<u64>,
}

pub struct AssignmentService;

impl AssignmentService {
    /// Evaluates all approved tasks in PostgreSQL and assigns ready ones to idle agents.
    ///
    /// Concurrency & Invariants:
    /// 1. Uses `FOR UPDATE SKIP LOCKED` on tasks and agents in a transaction so concurrent
    ///    coordinator workers cannot double-assign a task or double-book an agent.
    /// 2. Gated by `task_approvals` record: only tasks with status `approved` and human approval row can proceed.
    /// 3. Gated by `task_dependencies`: tasks with uncompleted blocking prerequisites are skipped.
    /// 4. Atomic outbox pattern: `TaskDelivery(Pending)` is committed before transport publish.
    /// 5. In-band compensation: if NATS publish fails synchronously, delivery is marked Terminal
    ///    and the task/agent states are rolled back.
    pub async fn assign_ready_tasks(
        pool: &PgPool,
        project_id: Option<Uuid>,
        jetstream: Option<&JetStreamContext>,
    ) -> Result<Vec<AssignmentResult>> {
        let mut tx = pool.begin().await.context("Failed to begin assignment transaction")?;

        // 1. Claim candidate approved tasks with satisfied blockers and recorded human approval
        let claimable_tasks = sqlx::query_as!(
            Task,
            r#"
            SELECT t.id, t.project_id, t.short_id, t.title, t.description,
                   t.status AS "status: TaskStatus", t.assigned_agent_id,
                   t.affected_resources, t.estimated_size, t.proposal_id,
                   t.created_at, t.updated_at
            FROM tasks t
            JOIN task_approvals ta ON t.id = ta.task_id AND t.proposal_id = ta.proposal_id
            WHERE ($1::uuid IS NULL OR t.project_id = $1)
              AND t.status = 'approved'
              AND ta.status IN ('approved', 'edited_and_approved')
              AND NOT EXISTS (
                  SELECT 1
                  FROM task_dependencies td
                  JOIN tasks b ON td.depends_on_id = b.id
                  WHERE td.dependent_id = t.id
                    AND td.kind = 'blocks'
                    AND b.status <> 'completed'
              )
            ORDER BY t.created_at ASC
            FOR UPDATE OF t SKIP LOCKED
            "#,
            project_id
        )
        .fetch_all(&mut *tx)
        .await
        .context("Failed to query claimable approved tasks")?;

        if claimable_tasks.is_empty() {
            tx.commit().await?;
            return Ok(vec![]);
        }

        let mut staged_deliveries = Vec::new();

        for task in claimable_tasks {
            // 2. Select an idle agent (prefer pre-assigned/suggested agent if set and idle)
            let matched_agent = if let Some(target_agent_id) = task.assigned_agent_id {
                sqlx::query_as!(
                    Agent,
                    r#"
                    SELECT id, human_owner, api_key_hash, adapter_type AS "adapter_type: AdapterType",
                           capabilities, nats_subject, status AS "status: AgentStatus",
                           current_task_id, last_seen, created_at
                    FROM agents
                    WHERE id = $1 AND status = 'idle'
                    FOR UPDATE SKIP LOCKED
                    "#,
                    target_agent_id
                )
                .fetch_optional(&mut *tx)
                .await?
            } else {
                None
            };

            let agent = match matched_agent {
                Some(a) => Some(a),
                None => {
                    // Find any idle registered agent that is not targeted by another active task
                    sqlx::query_as!(
                        Agent,
                        r#"
                        SELECT a.id, a.human_owner, a.api_key_hash, a.adapter_type AS "adapter_type: AdapterType",
                               a.capabilities, a.nats_subject, a.status AS "status: AgentStatus",
                               a.current_task_id, a.last_seen, a.created_at
                        FROM agents a
                        WHERE a.status = 'idle'
                          AND NOT EXISTS (
                              SELECT 1 FROM tasks t
                              WHERE t.assigned_agent_id = a.id
                                AND t.status IN ('approved', 'assigned', 'executing')
                                AND t.id <> $1
                          )
                        ORDER BY a.last_seen DESC NULLS LAST
                        LIMIT 1
                        FOR UPDATE SKIP LOCKED
                        "#,
                        task.id
                    )
                    .fetch_optional(&mut *tx)
                    .await?
                }
            };

            let Some(agent) = agent else {
                // No available idle agent for this task in this cycle
                continue;
            };

            // 3. Compute attempt number (increments on reassignment)
            let attempt_row = sqlx::query!(
                "SELECT COALESCE(MAX(attempt), 0) + 1 AS next_attempt FROM task_deliveries WHERE task_id = $1",
                task.id
            )
            .fetch_one(&mut *tx)
            .await?;
            let attempt = attempt_row.next_attempt.unwrap_or(1);
            let idempotency_key = format!("{}:{}", task.id, attempt);
            let nats_subject = format!("coordinator.tasks.assign.{}", agent.id);

            // 4. Create TaskDelivery with status Pending
            let expires_at = Utc::now() + chrono::Duration::seconds(60);
            let delivery = sqlx::query!(
                r#"
                INSERT INTO task_deliveries (
                    task_id, agent_id, attempt, nats_stream, nats_subject,
                    idempotency_key, expires_at, status
                )
                VALUES ($1, $2, $3, 'TASK_ASSIGNMENTS', $4, $5, $6, 'pending')
                RETURNING id
                "#,
                task.id,
                agent.id,
                attempt,
                nats_subject,
                idempotency_key,
                expires_at
            )
            .fetch_one(&mut *tx)
            .await?;

            // 5. Update Task -> Assigned
            sqlx::query!(
                "UPDATE tasks SET status = 'assigned', assigned_agent_id = $2, updated_at = NOW() WHERE id = $1",
                task.id,
                agent.id
            )
            .execute(&mut *tx)
            .await?;

            // 6. Update Agent -> Busy
            sqlx::query!(
                "UPDATE agents SET status = 'busy', current_task_id = $1 WHERE id = $2",
                task.id,
                agent.id
            )
            .execute(&mut *tx)
            .await?;

            staged_deliveries.push((task, agent, delivery.id, idempotency_key));
        }

        // Commit DB transaction (outbox intent is now durable)
        tx.commit().await.context("Failed to commit assignment transaction")?;

        let mut results = Vec::new();

        // 7. Post-commit transport publishing
        for (task, agent, delivery_id, idempotency_key) in staged_deliveries {
            let mut nats_seq = None;

            if let Some(js) = jetstream {
                // Fetch blocking dependency IDs
                let deps = TaskRepository::list_dependencies(pool, task.id).await.unwrap_or_default();
                let depends_on = deps
                    .into_iter()
                    .filter(|d| d.kind == crate::domain::DependencyKind::Blocks)
                    .map(|d| d.depends_on_id)
                    .collect();

                let spec = TaskSpec {
                    task_id: task.id,
                    short_id: task.short_id.clone(),
                    title: task.title.clone(),
                    description: task.description.clone(),
                    affected_resources: task.resources(),
                    depends_on,
                    idempotency_key: idempotency_key.clone(),
                    assigned_at: Utc::now(),
                };

                match TaskPublisher::publish_assignment(js, agent.id, &spec).await {
                    Ok(seq) => {
                        nats_seq = Some(seq);
                        if let Err(e) = TaskDeliveryRepository::mark_delivered(pool, delivery_id, seq as i64).await {
                            error!(delivery_id = %delivery_id, error = %e, "Failed to mark delivery delivered");
                        }
                    }
                    Err(e) => {
                        error!(
                            task_id = %task.id,
                            agent_id = %agent.id,
                            error = %e,
                            "NATS publish failed immediately; executing in-band rollback compensation"
                        );
                        // In-band compensation: rollback state so task can be retried cleanly
                        let _ = TaskDeliveryRepository::mark_terminal(
                            pool,
                            delivery_id,
                            &format!("NATS publish error: {e}"),
                        )
                        .await;
                        let _ = TaskRepository::update_status(pool, task.id, TaskStatus::Approved).await;
                        let _ = TaskRepository::assign_agent(pool, task.id, None).await;
                        let _ = AgentRepository::set_current_task(pool, agent.id, None, AgentStatus::Idle).await;
                        continue;
                    }
                }
            } else {
                // In headless/test mode without NATS, mark delivered directly
                let _ = TaskDeliveryRepository::mark_delivered(pool, delivery_id, 1).await;
            }

            results.push(AssignmentResult {
                task_id: task.id,
                agent_id: agent.id,
                delivery_id,
                idempotency_key,
                nats_sequence: nats_seq,
            });
        }

        Ok(results)
    }

    /// Background reconciliation loop for stuck 'pending' deliveries (out-of-band crash recovery).
    pub async fn reconcile_pending_deliveries(
        pool: &PgPool,
        jetstream: Option<&JetStreamContext>,
    ) -> Result<usize> {
        let stuck = sqlx::query_as!(
            crate::domain::TaskDelivery,
            r#"
            SELECT id, task_id, agent_id, attempt, nats_stream, nats_subject, nats_sequence,
                   idempotency_key, delivered_at, acknowledged_at, ack_kind AS "ack_kind: crate::domain::AckKind",
                   expires_at, status AS "status: DeliveryStatus", failure_reason, reassigned_to
            FROM task_deliveries
            WHERE status = 'pending'
              AND delivered_at < NOW() - INTERVAL '5 seconds'
            ORDER BY delivered_at ASC
            "#
        )
        .fetch_all(pool)
        .await
        .context("Failed to query stuck pending deliveries")?;

        let count = stuck.len();
        if count == 0 {
            return Ok(0);
        }

        warn!(count, "Reconciling stuck pending deliveries");

        for delivery in stuck {
            let task = TaskRepository::find_by_id(pool, delivery.task_id).await?;
            let Some(task) = task else { continue };

            if let Some(js) = jetstream {
                let deps = TaskRepository::list_dependencies(pool, task.id).await.unwrap_or_default();
                let depends_on = deps
                    .into_iter()
                    .filter(|d| d.kind == crate::domain::DependencyKind::Blocks)
                    .map(|d| d.depends_on_id)
                    .collect();

                let spec = TaskSpec {
                    task_id: task.id,
                    short_id: task.short_id.clone(),
                    title: task.title.clone(),
                    description: task.description.clone(),
                    affected_resources: task.resources(),
                    depends_on,
                    idempotency_key: delivery.idempotency_key.clone(),
                    assigned_at: Utc::now(),
                };

                match TaskPublisher::publish_assignment(js, delivery.agent_id, &spec).await {
                    Ok(seq) => {
                        info!(delivery_id = %delivery.id, seq, "Reconciliation republished task assignment");
                        let _ = TaskDeliveryRepository::mark_delivered(pool, delivery.id, seq as i64).await;
                    }
                    Err(e) => {
                        error!(delivery_id = %delivery.id, error = %e, "Reconciliation publish failed");
                        if delivery.attempt >= 3 {
                            let _ = TaskDeliveryRepository::mark_terminal(
                                pool,
                                delivery.id,
                                "Reconciliation publish retry limit exceeded",
                            )
                            .await;
                            let _ = TaskRepository::update_status(pool, task.id, TaskStatus::Approved).await;
                            let _ = TaskRepository::assign_agent(pool, task.id, None).await;
                            let _ = AgentRepository::set_current_task(pool, delivery.agent_id, None, AgentStatus::Idle).await;
                        }
                    }
                }
            }
        }

        Ok(count)
    }
}
