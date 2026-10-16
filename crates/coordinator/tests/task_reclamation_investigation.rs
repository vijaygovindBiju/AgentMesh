//! Deterministic Regression Test Suite for Incident Investigation & Fix:
//! Task-Reclamation Liveness & Test Isolation Hardening.
//!
//! Verifies:
//! 1. Genuinely offline agent's stale task can be reclaimed safely.
//! 2. Healthy active agent's task is NOT reclaimed.
//! 3. Transient heartbeat delay does not cause premature reclamation or unsafe duplicate execution.
//! 4. Reclamation transfers task ownership and state consistently (delivery terminated, project_id recorded).
//! 5. Reclaimed tasks are not repeatedly reclaimed without a new valid assignment.
//! 6. Stale/unheartbeated test fixtures are detected, marked offline, and barred from task assignment.
//! 7. Coordinator startup recovery and sweeper do not incorrectly reclaim healthy active work.

use chrono::{Duration, Utc};
use sqlx::PgPool;
use std::time::Duration as StdDuration;
use uuid::Uuid;

use agent_protocol::security::AgentRole;
use coordinator::coordinator::AssignmentService;
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, ProjectRepository, ProposalRepository, TaskDeliveryRepository, TaskRepository,
};
use coordinator::domain::{
    AckKind, AdapterType, AgentStatus, ApprovalStatus, DeliveryStatus, NewAgent, NewProject,
    NewProposal, NewTask, NewTaskApproval, NewTaskDelivery, TaskStatus,
};
use coordinator::messaging::HeartbeatMonitor;
use coordinator::observability::CoordinatorEventRepository;
use coordinator::reliability::{CoordinatorRecoveryService, StaleTaskSweeper};

async fn setup_test_env() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;
    Some(pool)
}

struct TestCleanupGuard<'a> {
    pool: &'a PgPool,
    project_id: Option<Uuid>,
    agent_ids: Vec<Uuid>,
}

impl<'a> TestCleanupGuard<'a> {
    fn new(pool: &'a PgPool) -> Self {
        Self {
            pool,
            project_id: None,
            agent_ids: Vec::new(),
        }
    }

    fn track_project(&mut self, id: Uuid) {
        self.project_id = Some(id);
    }

    fn track_agent(&mut self, id: Uuid) {
        self.agent_ids.push(id);
    }

    async fn cleanup(self) {
        if let Some(pid) = self.project_id {
            let _ = ProjectRepository::delete(self.pool, pid).await;
        }
        for aid in self.agent_ids {
            let _ = sqlx::query!("DELETE FROM agents WHERE id = $1", aid)
                .execute(self.pool)
                .await;
        }
    }
}

async fn create_test_project_and_task(pool: &PgPool, guard: &mut TestCleanupGuard<'_>) -> (Uuid, Uuid) {
    let proj = ProjectRepository::create(
        pool,
        &NewProject {
            name: format!("IncidentTest Proj {}", &Uuid::new_v4().to_string()[..8]),
            description: "Testing task reclamation incident".to_string(),
        },
    )
    .await
    .unwrap();
    guard.track_project(proj.id);

    let prop = ProposalRepository::create(
        pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("INC-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Incident Test Task".to_string(),
            description: "Test task reclamation".to_string(),
            affected_resources: vec!["crates/reliability".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    (proj.id, task.id)
}

async fn create_test_agent(pool: &PgPool, name: &str, guard: &mut TestCleanupGuard<'_>) -> Uuid {
    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create(
        pool,
        &NewAgent {
            human_owner: name.to_string(),
            api_key_hash: format!("hash_{}", &Uuid::new_v4().to_string()[..8]),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            role: Some(AgentRole::Worker.to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    guard.track_agent(agent.id);
    agent.id
}

// ----------------------------------------------------------------------------
// Test 1: A genuinely offline agent's stale task can be reclaimed safely.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_genuinely_offline_agent_task_reclaimed_safely() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let agent_id = create_test_agent(&pool, "GenuinelyOfflineWorker", &mut guard).await;

    // Set agent to offline and simulate expired heartbeat
    let old_timestamp = Utc::now() - Duration::seconds(60);
    sqlx::query!(
        "UPDATE agents SET status = 'offline', health_status = 'offline', last_seen = $1 WHERE id = $2",
        old_timestamp,
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Assign task and age it
    TaskRepository::assign_agent(&pool, task_id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Assigned)
        .await
        .unwrap();
    sqlx::query!(
        "UPDATE tasks SET updated_at = $1 WHERE id = $2",
        old_timestamp,
        task_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Create an active delivery
    let delivery = TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{agent_id}"),
            idempotency_key: format!("{task_id}:1"),
            expires_at: Utc::now() + Duration::seconds(60),
        },
    )
    .await
    .unwrap();
    TaskDeliveryRepository::mark_delivered(&pool, delivery.id, 1)
        .await
        .unwrap();

    // Sweep with 10s threshold
    let res = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .expect("Sweep failed");

    assert!(res.tasks_reclaimed.contains(&task_id));

    // Verify task state: unassigned, reverted to Approved
    let task = TaskRepository::find_by_id(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Approved);
    assert_eq!(task.assigned_agent_id, None);

    // Verify delivery state: marked terminal
    let del = TaskDeliveryRepository::find_by_id(&pool, delivery.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(del.status, DeliveryStatus::Terminal);
    assert_eq!(del.ack_kind, Some(AckKind::Term));

    // Verify event has correct project_id
    let events = CoordinatorEventRepository::find_by_task(&pool, task_id)
        .await
        .unwrap();
    let reclaim_ev = events
        .iter()
        .find(|e| e.event_type == "task.reclaimed")
        .expect("task.reclaimed event must exist");
    assert_eq!(reclaim_ev.project_id, Some(proj_id));

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 2: A healthy active agent's task is NOT reclaimed.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_healthy_active_agent_task_not_reclaimed() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (_proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let agent_id = create_test_agent(&pool, "HealthyActiveWorker", &mut guard).await;

    // Agent has fresh heartbeat
    let now = Utc::now();
    sqlx::query!(
        "UPDATE agents SET status = 'busy', health_status = 'healthy', last_seen = $1 WHERE id = $2",
        now,
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Task is executing
    TaskRepository::assign_agent(&pool, task_id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing)
        .await
        .unwrap();

    // Sweep with 30s threshold
    let res = StaleTaskSweeper::sweep(&pool, Duration::seconds(30))
        .await
        .expect("Sweep failed");

    assert!(!res.tasks_reclaimed.contains(&task_id));

    let task = TaskRepository::find_by_id(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Executing);
    assert_eq!(task.assigned_agent_id, Some(agent_id));

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 3: Transient heartbeat delay does not cause premature reclamation.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_transient_heartbeat_delay_does_not_cause_unsafe_duplicate_execution() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (_proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let agent_id = create_test_agent(&pool, "TransientDelayWorker", &mut guard).await;

    // Agent has a transient heartbeat delay (20 seconds ago, beyond 10s heartbeat rate but within 30s threshold)
    let delay_timestamp = Utc::now() - Duration::seconds(20);
    sqlx::query!(
        "UPDATE agents SET status = 'idle', health_status = 'healthy', last_seen = $1 WHERE id = $2",
        delay_timestamp,
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // BUT task was assigned 2 seconds ago (within assignment lease grace period)
    TaskRepository::assign_agent(&pool, task_id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Assigned)
        .await
        .unwrap();
    let just_now = Utc::now();
    sqlx::query!(
        "UPDATE tasks SET updated_at = $1 WHERE id = $2",
        just_now,
        task_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Sweep with 30s threshold. Since task was just assigned, grace period protects it!
    let res = StaleTaskSweeper::sweep(&pool, Duration::seconds(30))
        .await
        .expect("Sweep failed");

    assert!(
        !res.tasks_reclaimed.contains(&task_id),
        "Task within assignment grace period must not be reclaimed on transient heartbeat delay"
    );

    let task = TaskRepository::find_by_id(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Assigned);
    assert_eq!(task.assigned_agent_id, Some(agent_id));

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 4: Reclamation transfers task ownership and state consistently.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_reclamation_transfers_task_ownership_and_state_consistently() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let agent_id = create_test_agent(&pool, "OwnershipTransferWorker", &mut guard).await;

    // Set offline
    sqlx::query!(
        "UPDATE agents SET status = 'offline', health_status = 'offline' WHERE id = $1",
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    TaskRepository::assign_agent(&pool, task_id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing)
        .await
        .unwrap();

    // Create 3 deliveries to simulate retry limit reached
    for attempt in 1..=3 {
        let delivery = TaskDeliveryRepository::create(
            &pool,
            &NewTaskDelivery {
                task_id,
                agent_id,
                attempt,
                nats_stream: "TASK_ASSIGNMENTS".to_string(),
                nats_subject: format!("coordinator.tasks.assign.{agent_id}"),
                idempotency_key: format!("{task_id}:{attempt}"),
                expires_at: Utc::now() + Duration::seconds(60),
            },
        )
        .await
        .unwrap();

        if attempt < 3 {
            TaskDeliveryRepository::record_ack(&pool, delivery.id, AckKind::Term, DeliveryStatus::Terminal)
                .await
                .unwrap();
        } else {
            TaskDeliveryRepository::mark_delivered(&pool, delivery.id, attempt as i64)
                .await
                .unwrap();
        }
    }

    // Sweep
    let res = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .expect("Sweep failed");

    assert!(res.tasks_reclaimed.contains(&task_id));

    let task = TaskRepository::find_by_id(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    // After 3 attempts, must revert to HumanReview
    assert_eq!(task.status, TaskStatus::HumanReview);
    assert_eq!(task.assigned_agent_id, None);

    // Active delivery must be marked terminal
    let deliveries = TaskDeliveryRepository::list_by_task(&pool, task_id)
        .await
        .unwrap();
    for del in deliveries {
        assert_eq!(del.status, DeliveryStatus::Terminal);
    }

    // Event must have project_id and accurate metadata
    let events = CoordinatorEventRepository::find_by_task(&pool, task_id)
        .await
        .unwrap();
    let ev = events
        .iter()
        .find(|e| e.event_type == "task.reclaimed")
        .expect("task.reclaimed recorded");
    assert_eq!(ev.project_id, Some(proj_id));
    assert_eq!(ev.payload["attempts"], 3);
    assert_eq!(ev.payload["new_status"], "HumanReview");

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 5: Reclaimed tasks are not repeatedly reclaimed without a new valid assignment.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_reclaimed_tasks_not_repeatedly_reclaimed_without_new_valid_assignment() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (_proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let agent_id = create_test_agent(&pool, "RepeatedSweepWorker", &mut guard).await;

    sqlx::query!(
        "UPDATE agents SET status = 'offline', health_status = 'offline' WHERE id = $1",
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    TaskRepository::assign_agent(&pool, task_id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Assigned)
        .await
        .unwrap();

    // First sweep reclaims the task
    let res1 = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .unwrap();
    assert!(res1.tasks_reclaimed.contains(&task_id));

    // Subsequent second sweep: task is now unassigned in Approved/HumanReview, so 0 reclaimed
    let res2 = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .unwrap();
    assert!(
        !res2.tasks_reclaimed.contains(&task_id),
        "Task must not be reclaimed repeatedly without a new assignment"
    );

    // Third sweep: still 0 reclaimed
    let res3 = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .unwrap();
    assert!(!res3.tasks_reclaimed.contains(&task_id));

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 6: Stale/unheartbeated test fixtures are detected, marked offline, and barred from assignment.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_stale_unheartbeated_fixtures_marked_offline_and_not_assigned() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let stale_agent_id = create_test_agent(&pool, "UnheartbeatedFixtureAgent", &mut guard).await;

    // Simulate an agent fixture created 2 minutes ago that never sent a single heartbeat
    let past = Utc::now() - Duration::minutes(2);
    sqlx::query!(
        "UPDATE agents SET status = 'idle', health_status = 'healthy', last_seen = NULL, created_at = $1 WHERE id = $2",
        past,
        stale_agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Run timeout check
    let timed_out = HeartbeatMonitor::check_timeouts(&pool, StdDuration::from_secs(10))
        .await
        .expect("check_timeouts");

    assert!(
        timed_out.contains(&stale_agent_id),
        "Unheartbeated agent older than threshold must be marked offline"
    );

    let agent = AgentRepository::find_by_id(&pool, stale_agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agent.status, AgentStatus::Offline);

    // Approve task and attempt assignment
    TaskRepository::update_status(&pool, task_id, TaskStatus::Approved)
        .await
        .unwrap();
    // Record mock approval
    let proposal_id = sqlx::query_scalar!(
        "SELECT proposal_id FROM tasks WHERE id = $1",
        task_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let _ = ProposalRepository::record_approval(
        &pool,
        &NewTaskApproval {
            task_id,
            proposal_id,
            status: ApprovalStatus::Approved,
            edited_desc: None,
            approved_by: "TestReviewer".to_string(),
        },
    )
    .await;

    // Try assigning tasks for this project
    let assignments = AssignmentService::assign_ready_tasks(&pool, Some(proj_id), None)
        .await
        .unwrap();

    // Assignment MUST NOT select the dead/offline fixture agent!
    assert!(
        assignments.iter().all(|a| a.agent_id != stale_agent_id),
        "Offline fixture agent must never be assigned tasks"
    );

    guard.cleanup().await;
}

// ----------------------------------------------------------------------------
// Test 7: Coordinator startup recovery and sweeper do not incorrectly reclaim healthy active work.
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_launching_coordinator_does_not_reclaim_healthy_work() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };
    let mut guard = TestCleanupGuard::new(&pool);

    let (_proj_id, task_id) = create_test_project_and_task(&pool, &mut guard).await;
    let live_agent_id = create_test_agent(&pool, "LiveTuiOperatorAgent", &mut guard).await;

    // Agent is active and healthy
    let now = Utc::now();
    sqlx::query!(
        "UPDATE agents SET status = 'busy', health_status = 'healthy', last_seen = $1 WHERE id = $2",
        now,
        live_agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    TaskRepository::assign_agent(&pool, task_id, Some(live_agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing)
        .await
        .unwrap();

    // Execute startup recovery
    let report = CoordinatorRecoveryService::recover_on_startup(&pool, None)
        .await
        .expect("Startup recovery");

    assert!(
        !report.sweep_summary.tasks_reclaimed.contains(&task_id),
        "Startup recovery must not reclaim healthy in-progress work"
    );

    // Execute standard sweep
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(30))
        .await
        .expect("Sweep");

    assert!(
        !sweep.tasks_reclaimed.contains(&task_id),
        "Periodic sweeper must not reclaim healthy in-progress work"
    );

    let task = TaskRepository::find_by_id(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Executing);
    assert_eq!(task.assigned_agent_id, Some(live_agent_id));

    guard.cleanup().await;
}
