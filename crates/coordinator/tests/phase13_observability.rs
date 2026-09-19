use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentEventRepository, AgentRepository, GitConflictRepository, ProjectRepository,
    ProposalRepository, TaskDeliveryRepository, TaskRepository, UnexpectedResourceRepository,
};
use coordinator::domain::{
    AckKind, AdapterType, AgentEventType, DeliveryStatus, NewAgent, NewAgentEvent, NewProject,
    NewProposal, NewTask, NewTaskDelivery, TaskStatus,
};
use coordinator::observability::{
    CoordinatorEventRepository, DeliveryDiagnostics, FailureDiagnostics, MetricsCollector,
    TimelineService, TraceContext,
};

async fn setup_test_env() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    Some(pool)
}

async fn create_test_project_and_task(pool: &PgPool) -> (Uuid, Uuid) {
    let proj = ProjectRepository::create(
        pool,
        &NewProject {
            name: format!("Test Proj {}", &Uuid::new_v4().to_string()[..8]),
            description: "Test description".to_string(),
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
            raw_prompt: "prompt".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("T-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Test Task".to_string(),
            description: "Desc".to_string(),
            affected_resources: vec!["crates/test".to_string()],
            estimated_size: Some("S".to_string()),
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
            ..Default::default()
        },
    )
    .await
    .unwrap();

    agent.id
}

#[tokio::test]
async fn test_phase13_1_coordinator_events_recording_and_querying() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let (project_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "EventsAgent").await;

    // 1. Record events
    let ev1 = CoordinatorEventRepository::record(
        &pool,
        "task.planned",
        Some(project_id),
        Some(task_id),
        None,
        "Task was generated and proposed",
        serde_json::json!({ "size": "M" }),
    )
    .await
    .expect("Record event 1");

    let ev2 = CoordinatorEventRepository::record(
        &pool,
        "task.assigned",
        Some(project_id),
        Some(task_id),
        Some(agent_id),
        "Task assigned to agent",
        serde_json::json!({ "attempt": 1 }),
    )
    .await
    .expect("Record event 2");

    assert_eq!(ev1.event_type, "task.planned");
    assert_eq!(ev2.event_type, "task.assigned");

    // 2. Query by task
    let task_events = CoordinatorEventRepository::find_by_task(&pool, task_id)
        .await
        .expect("Find by task");
    assert!(task_events.len() >= 2);
    assert!(task_events.iter().any(|e| e.event_type == "task.planned"));
    assert!(task_events.iter().any(|e| e.event_type == "task.assigned"));

    // 3. Query by agent
    let agent_events = CoordinatorEventRepository::find_by_agent(&pool, agent_id)
        .await
        .expect("Find by agent");
    assert!(agent_events.iter().any(|e| e.event_type == "task.assigned"));

    // 4. Query by event type
    let planned_events = CoordinatorEventRepository::find_by_type(&pool, "task.planned", 10)
        .await
        .expect("Find by type");
    assert!(planned_events.iter().any(|e| e.id == ev1.id));
}

#[tokio::test]
async fn test_phase13_2_task_execution_timeline_generation() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Observability Timeline Test".to_string(),
            description: "Test task execution timeline".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "prompt".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "TimelineAgent".to_string(),
            api_key_hash: "hash_timeline".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("TL-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task with milestones".to_string(),
            description: "Trace all lifecycle milestones".to_string(),
            affected_resources: vec!["crates/obs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    // Assign agent to task
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id))
        .await
        .unwrap();

    // Record delivery attempt
    let _delivery = TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id: task.id,
            agent_id: agent.id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{}", agent.id),
            idempotency_key: format!("{}:1", task.id),
            expires_at: Utc::now() + Duration::minutes(5),
        },
    )
    .await
    .unwrap();

    // Record agent lifecycle events
    AgentEventRepository::create(
        &pool,
        &NewAgentEvent {
            agent_id: agent.id,
            task_id: task.id,
            event_type: AgentEventType::TaskStarted,
            message: Some("Agent started task".to_string()),
            payload: serde_json::json!({ "pid": 1234 }),
        },
    )
    .await
    .unwrap();

    AgentEventRepository::create(
        &pool,
        &NewAgentEvent {
            agent_id: agent.id,
            task_id: task.id,
            event_type: AgentEventType::ProgressUpdate,
            message: Some("50% complete".to_string()),
            payload: serde_json::json!({ "percentage": 50 }),
        },
    )
    .await
    .unwrap();

    AgentEventRepository::create(
        &pool,
        &NewAgentEvent {
            agent_id: agent.id,
            task_id: task.id,
            event_type: AgentEventType::Completed,
            message: Some("Task successfully finished".to_string()),
            payload: serde_json::json!({ "result": "ok" }),
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::Completed)
        .await
        .unwrap();

    // Build timeline
    let timeline = TimelineService::build_task_timeline(&pool, task.id)
        .await
        .expect("Build task timeline");

    assert_eq!(timeline.task_id, task.id);
    assert_eq!(timeline.status, TaskStatus::Completed);
    assert_eq!(
        timeline.assigned_agent_name,
        Some("TimelineAgent".to_string())
    );
    assert!(timeline.events.len() >= 4);

    // Verify stages
    let stages: Vec<&str> = timeline.events.iter().map(|e| e.stage.as_str()).collect();
    assert!(stages.contains(&"proposed"));
    assert!(stages.contains(&"delivery_attempt"));
    assert!(stages.contains(&"started"));
    assert!(stages.contains(&"completed"));

    // Verify elapsed time on completed milestone
    let completed_event = timeline
        .events
        .iter()
        .find(|e| e.stage == "completed")
        .expect("Completed event present");
    assert!(completed_event.elapsed_since_start_ms.is_some());
}

#[tokio::test]
async fn test_phase13_3_agent_activity_timeline() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "ObservabilityWorker".to_string(),
            api_key_hash: "hash_worker".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["python".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create real task for the agent event
    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;

    AgentEventRepository::create(
        &pool,
        &NewAgentEvent {
            agent_id: agent.id,
            task_id,
            event_type: AgentEventType::ProgressUpdate,
            message: Some("Running unit tests".to_string()),
            payload: serde_json::json!({}),
        },
    )
    .await
    .unwrap();

    let timeline = TimelineService::build_agent_timeline(&pool, agent.id)
        .await
        .expect("Build agent timeline");

    assert_eq!(timeline.agent_id, agent.id);
    assert_eq!(timeline.human_owner, "ObservabilityWorker");
    assert!(timeline
        .activities
        .iter()
        .any(|a| a.activity_type == "registered"));
    assert!(timeline
        .activities
        .iter()
        .any(|a| a.message == "Running unit tests"));
}

#[tokio::test]
async fn test_phase13_4_delivery_visibility_and_expiration_detection() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let (_proj_id, task_id) = create_test_project_and_task(&pool).await;
    let agent_id = create_test_agent(&pool, "DeliveryAgent").await;

    // Create an expired pending delivery (expires in past)
    let past_expiration = Utc::now() - Duration::minutes(10);
    let expired_delivery = TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{}", agent_id),
            idempotency_key: format!("{}:1", task_id),
            expires_at: past_expiration,
        },
    )
    .await
    .unwrap();

    let diagnostics = DeliveryDiagnostics::inspect(&pool, task_id)
        .await
        .expect("Inspect deliveries");

    assert_eq!(diagnostics.total_attempts, 1);
    let cur = diagnostics
        .current_attempt
        .expect("Current attempt present");
    assert_eq!(cur.attempt, 1);
    assert_eq!(cur.status, DeliveryStatus::Pending);
    assert!(
        cur.is_expired,
        "Pending delivery with past expires_at should be flagged expired"
    );

    // Clean up created record
    let _ = TaskDeliveryRepository::record_ack(
        &pool,
        expired_delivery.id,
        AckKind::Term,
        DeliveryStatus::Terminal,
    )
    .await;
}

#[tokio::test]
async fn test_phase13_5_failure_diagnostics_and_remediation_guidance() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Diagnostics Project".to_string(),
            description: "Testing FailureDiagnostics".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
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

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "FailingAgent".to_string(),
            api_key_hash: "hash_fail".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["c++".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Mark agent degraded with consecutive failures
    AgentRepository::record_task_failure(&pool, agent.id, "Test failure 1")
        .await
        .unwrap();
    AgentRepository::record_task_failure(&pool, agent.id, "Test failure 2")
        .await
        .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("FAIL-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task with merge collision".to_string(),
            description: "Will conflict and fail".to_string(),
            affected_resources: vec!["crates/core/src/lib.rs".to_string()],
            estimated_size: Some("L".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("FAIL-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Conflicting sibling task".to_string(),
            description: "Conflicting task B".to_string(),
            affected_resources: vec!["crates/core/src/lib.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    TaskRepository::assign_agent(&pool, task1.id, Some(agent.id))
        .await
        .unwrap();

    // Record agent failure event
    AgentEventRepository::create(
        &pool,
        &NewAgentEvent {
            agent_id: agent.id,
            task_id: task1.id,
            event_type: AgentEventType::Failed,
            message: Some("Compilation error: borrow checker violation".to_string()),
            payload: serde_json::json!({ "exit_code": 1 }),
        },
    )
    .await
    .unwrap();

    // Record git conflict for task1 and task2
    GitConflictRepository::record_conflict(
        &pool,
        proj.id,
        task1.id,
        task2.id,
        "crates/core/src/lib.rs",
        "Conflicting edits on lines 10-25",
    )
    .await
    .unwrap();

    // Record unexpected resource edit
    UnexpectedResourceRepository::record(
        &pool,
        task1.id,
        "crates/core/Cargo.toml",
        "Modified Cargo.toml not declared in affected_resources",
    )
    .await
    .unwrap();

    // Run failure diagnostics
    let diag = FailureDiagnostics::diagnose_task(&pool, task1.id)
        .await
        .expect("Diagnose task");

    assert_eq!(diag.task_id, task1.id);
    assert_eq!(
        diag.error_message,
        "Compilation error: borrow checker violation"
    );
    assert_eq!(diag.agent_owner, Some("FailingAgent".to_string()));
    assert!(diag.has_git_conflicts);
    assert!(diag.has_unexpected_resource_changes);

    // Verify remediation advice
    assert!(diag.remediation_advice.len() >= 2);
    let titles: Vec<&str> = diag
        .remediation_advice
        .iter()
        .map(|r| r.title.as_str())
        .collect();
    assert!(titles.contains(&"Resolve Cross-Agent Git Merge Conflicts"));
    assert!(titles.contains(&"Review Unexpected Resource Footprint"));
}

#[tokio::test]
async fn test_phase13_6_system_metrics_aggregation() {
    let Some(pool) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let metrics = MetricsCollector::collect(&pool)
        .await
        .expect("Collect system metrics");

    // Basic invariant checks
    assert!(
        metrics.total_tasks
            >= metrics.tasks_completed + metrics.tasks_failed + metrics.tasks_executing
    );
    assert!(
        metrics.total_agents >= metrics.agents_idle + metrics.agents_busy + metrics.agents_offline
    );
    assert!(
        metrics.delivery_success_rate_percent >= 0.0
            && metrics.delivery_success_rate_percent <= 100.0
    );
}

#[tokio::test]
async fn test_phase13_7_trace_context_structured_logging() {
    let task_id = Uuid::new_v4();
    let agent_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();

    let ctx = TraceContext::new()
        .with_project(project_id)
        .with_task(task_id)
        .with_agent(agent_id);

    assert_eq!(ctx.project_id, Some(project_id));
    assert_eq!(ctx.task_id, Some(task_id));
    assert_eq!(ctx.agent_id, Some(agent_id));
    assert!(!ctx.trace_id.is_empty());

    // Span creation
    let span = ctx.span("process_task_assignment");
    assert_eq!(span.metadata().map(|m| m.name()), Some("agentmesh_op"));
}
