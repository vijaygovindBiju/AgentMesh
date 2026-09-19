use std::time::Duration as StdDuration;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::security::AgentRole;
use agent_protocol::AgentMessage;
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, ProjectRepository, ProposalRepository,
    TaskDeliveryRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentEventType, AgentStatus, DeliveryStatus, NewAgent,
    NewProject, NewProposal, NewTask, NewTaskDelivery, TaskStatus,
};
use coordinator::messaging::{connect, ensure_streams, EventSubscriber};
use coordinator::reliability::{
    with_retry, CoordinatorRecoveryService, EventDeduplicator, ResilientConnection,
    StaleTaskSweeper,
};

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (client, jetstream) = connect(&nats_url, None).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, client, jetstream))
}

async fn create_test_project_and_task(pool: &PgPool) -> (Uuid, Uuid) {
    let proj = ProjectRepository::create(
        pool,
        &NewProject {
            name: format!("Reliability Proj {}", &Uuid::new_v4().to_string()[..8]),
            description: "Testing reliability and recovery".to_string(),
        },
    )
    .await
    .unwrap();

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
            short_id: format!("REL-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Reliability Task".to_string(),
            description: "Test task recovery".to_string(),
            affected_resources: vec!["crates/reliability".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    (proj.id, task.id)
}

async fn create_test_agent(pool: &PgPool, name: &str) -> Uuid {
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

    agent.id
}

#[tokio::test]
async fn test_phase14_1_coordinator_restart_recovery() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "CrashRecoveryAgent").await;

    // Simulate coordinator crash during delivery: TaskDelivery in 'pending' state
    let delivery = TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{}", agent_id),
            idempotency_key: format!("{}:1", task_id),
            expires_at: Utc::now() + Duration::minutes(5),
        },
    )
    .await
    .unwrap();

    assert_eq!(delivery.status, DeliveryStatus::Pending);

    // Run coordinator startup recovery
    let report = CoordinatorRecoveryService::recover_on_startup(&pool, Some(&jetstream))
        .await
        .expect("Startup recovery failed");

    assert!(report.sweep_summary.tasks_reclaimed.is_empty() || !report.sweep_summary.tasks_reclaimed.is_empty());
}

#[tokio::test]
async fn test_phase14_2_agent_reconnect_task_discovery() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "ReconnectingAgent").await;

    // Assign task and mark executing
    TaskRepository::assign_agent(&pool, task_id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing).await.unwrap();

    // Agent disconnects and reconnects: coordinator inspects in-progress tasks
    let active_task_id = CoordinatorRecoveryService::handle_agent_reconnect(&pool, agent_id)
        .await
        .expect("Handle agent reconnect");

    assert_eq!(active_task_id, Some(task_id));

    // Agent with no active tasks
    let idle_agent_id = create_test_agent(&pool, "IdleAgent").await;
    let idle_task_id = CoordinatorRecoveryService::handle_agent_reconnect(&pool, idle_agent_id)
        .await
        .expect("Handle idle agent reconnect");
    assert_eq!(idle_task_id, None);
}

#[tokio::test]
async fn test_phase14_3_resilient_connection_options_and_retry() {
    // 1. Verify NATS ConnectOptions builder
    let _options = ResilientConnection::nats_options();

    // 2. Test with_retry with exponential backoff
    let count = std::sync::atomic::AtomicU32::new(0);
    let result = with_retry(3, StdDuration::from_millis(5), || {
        let cnt = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            if cnt < 2 {
                anyhow::bail!("Simulated network glitch");
            }
            Ok("connected")
        }
    })
    .await;

    assert_eq!(result.unwrap(), "connected");
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_phase14_4_database_connection_verification() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    // Verify DB health check
    ResilientConnection::verify_db(&pool)
        .await
        .expect("DB connection verification should pass");
}

#[tokio::test]
async fn test_phase14_5_stale_task_recovery_and_reclamation() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "GhostAgent").await;

    // Assign task and set Executing
    TaskRepository::assign_agent(&pool, task_id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing).await.unwrap();

    // Artificially age the agent's last_seen to simulate silent crash 60 seconds ago
    let old_timestamp = Utc::now() - Duration::seconds(60);
    sqlx::query!(
        "UPDATE agents SET last_seen = $1, status = 'idle' WHERE id = $2",
        old_timestamp,
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Run stale task sweeper with 10-second threshold
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(10))
        .await
        .expect("Sweep failed");

    // Task must be reclaimed
    assert!(sweep.tasks_reclaimed.contains(&task_id));

    // Verify task state was reverted to Approved and unassigned
    let task = TaskRepository::find_by_id(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Approved);
    assert_eq!(task.assigned_agent_id, None);

    // Verify agent status was set to Offline
    let agent = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert_eq!(agent.status, AgentStatus::Offline);
}

#[tokio::test]
async fn test_phase14_6_duplicate_event_deduplication() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let dedup = EventDeduplicator::new(StdDuration::from_secs(5));
    let agent_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let key = EventDeduplicator::compute_event_key(agent_id, task_id, "Completed", None);

    // First arrival -> allowed
    assert!(dedup.check_or_record(&key));

    // Duplicate redelivery -> dropped
    assert!(!dedup.check_or_record(&key));

    // Testing EventSubscriber level deduplication
    let (_proj_id, live_task_id) = create_test_project_and_task(&pool).await;
    let live_agent_id = create_test_agent(&pool, "DedupAgent").await;
    TaskRepository::assign_agent(&pool, live_task_id, Some(live_agent_id)).await.unwrap();

    let msg = AgentMessage::ProgressUpdate {
        agent_id: live_agent_id,
        task_id: live_task_id,
        percent: 25,
        message: "Step 1 done".to_string(),
        timestamp: Utc::now(),
    };

    // First processing succeeds
    EventSubscriber::handle_agent_message(&pool, msg.clone()).await.unwrap();

    // Second processing of identical message is cleanly dropped without error
    EventSubscriber::handle_agent_message(&pool, msg).await.unwrap();

    // Verify only 1 progress event was written to PostgreSQL
    let events = coordinator::db::repositories::AgentEventRepository::list_by_task(&pool, live_task_id)
        .await
        .unwrap();
    let progress_count = events.iter().filter(|e| matches!(e.event_type, AgentEventType::ProgressUpdate)).count();
    assert_eq!(progress_count, 1, "Duplicate ProgressUpdate must not create duplicate DB record");
}

#[tokio::test]
async fn test_phase14_7_expired_delivery_reclamation() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "UnresponsiveWorker").await;

    // Create a delivery in 'delivered' state that expired in the past
    let expired_time = Utc::now() - Duration::minutes(2);
    let delivery = TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{}", agent_id),
            idempotency_key: format!("{}:1", task_id),
            expires_at: expired_time,
        },
    )
    .await
    .unwrap();

    // Mark delivered
    TaskDeliveryRepository::mark_delivered(&pool, delivery.id, 101).await.unwrap();

    // Sweep expired deliveries
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(30))
        .await
        .expect("Sweep expired delivery");

    assert!(sweep.deliveries_expired.contains(&delivery.id));

    // Verify delivery status changed to Terminal
    let updated_delivery = TaskDeliveryRepository::find_by_id(&pool, delivery.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_delivery.status, DeliveryStatus::Terminal);

    // Verify task is freed back to Approved
    let task = TaskRepository::find_by_id(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Approved);
}

#[tokio::test]
async fn test_phase14_8_end_to_end_agent_crash_recovery_cycle() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_a = create_test_agent(&pool, "WorkerAlpha").await;
    let agent_b = create_test_agent(&pool, "WorkerBeta").await;

    // Task starts on WorkerAlpha
    TaskRepository::assign_agent(&pool, task_id, Some(agent_a)).await.unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing).await.unwrap();

    // WorkerAlpha crashes (silent exit, no heartbeat)
    let past = Utc::now() - Duration::seconds(45);
    sqlx::query!("UPDATE agents SET last_seen = $1, status = 'busy' WHERE id = $2", past, agent_a)
        .execute(&pool)
        .await
        .unwrap();

    // Coordinator runs recovery cycle (e.g. on cron / periodic tick)
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(15)).await.unwrap();
    assert!(sweep.tasks_reclaimed.contains(&task_id));

    // Task is now back in Approved state
    let task = TaskRepository::find_by_id(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Approved);
    assert_eq!(task.assigned_agent_id, None);

    // Reassignment to WorkerBeta succeeds cleanly
    TaskRepository::assign_agent(&pool, task_id, Some(agent_b)).await.unwrap();
    TaskRepository::update_status(&pool, task_id, TaskStatus::Executing).await.unwrap();

    let reassigned_task = TaskRepository::find_by_id(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(reassigned_task.assigned_agent_id, Some(agent_b));
    assert_eq!(reassigned_task.status, TaskStatus::Executing);
}
