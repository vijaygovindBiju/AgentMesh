use std::time::Duration;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::AgentMessage;
use coordinator::coordinator::{CommandHandler, OverlapDetector};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, ProposalRepository,
    TaskRepository,
};
use coordinator::domain::{
    AdapterType, DependencyKind, NewAgent, NewProject, NewProposal, NewTask,
    NewTaskDependency, OverlapSeverity, TaskStatus,
};
use coordinator::messaging::{connect, ensure_streams, EventSubscriber};
use coordinator::tui::{
    render, AppState, CurrentScreen, ReviewTaskState, TuiAction, TuiUpdateEvent,
};


async fn setup_pool() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });
    let pool = create_pool(&url).await.ok()?;
    run_migrations(&pool).await.ok()?;
    Some(pool)
}

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Acceptance Criteria:
/// - When Task A and Task B share a resource, OverlapWarning is created in DB
/// - Concurrent tasks sharing a resource trigger Critical severity
/// - Sequential tasks sharing a resource trigger Info severity
#[tokio::test]
async fn test_overlap_detection_creates_warning_in_db() {
    let Some(pool) = setup_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 6 Overlap Proj".to_string(),
            description: "Testing overlap detection and persistence".to_string(),
        },
    )
    .await
    .expect("Project creation failed");

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "Decompose payment system".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .expect("Proposal creation failed");

    // 1. Create two concurrent tasks sharing "src/payment.rs"
    let task_a = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-01".to_string(),
            title: "Stripe payment integration".to_string(),
            description: "Add Stripe webhooks and checkout".to_string(),
            affected_resources: vec!["src/payment.rs".to_string(), "src/models.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task_b = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-02".to_string(),
            title: "PayPal gateway integration".to_string(),
            description: "Add PayPal capture and refunds".to_string(),
            affected_resources: vec!["src/payment.rs".to_string(), "src/routes.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // 2. Create two sequential tasks sharing "src/schema.sql"
    let task_c = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-03".to_string(),
            title: "DB migration init".to_string(),
            description: "Create initial table schemas".to_string(),
            affected_resources: vec!["src/schema.sql".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task_d = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-04".to_string(),
            title: "DB indexing and constraints".to_string(),
            description: "Add indexes to schema".to_string(),
            affected_resources: vec!["src/schema.sql".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Task D depends on Task C (Blocks) -> Sequential!
    TaskRepository::add_dependency(
        &pool,
        &NewTaskDependency {
            dependent_id: task_d.id,
            depends_on_id: task_c.id,
            kind: DependencyKind::Blocks,
        },
    )
    .await
    .unwrap();

    // 3. Run OverlapDetector
    let detected = OverlapDetector::detect_and_persist_for_project(&pool, project.id)
        .await
        .expect("Overlap detection failed");

    assert_eq!(detected.len(), 2, "Expected 2 overlap warnings");

    // 4. Verify in DB
    let warnings = OverlapWarningRepository::list_by_project(&pool, project.id)
        .await
        .unwrap();
    assert_eq!(warnings.len(), 2);

    // Critical warning should be first (concurrent on src/payment.rs)
    assert_eq!(warnings[0].resource, "src/payment.rs");
    assert_eq!(warnings[0].severity, OverlapSeverity::Critical);
    assert!(!warnings[0].acknowledged);
    let payment_tasks = warnings[0].task_id_list();
    assert!(payment_tasks.contains(&task_a.id));
    assert!(payment_tasks.contains(&task_b.id));

    // Info warning should be second (sequential on src/schema.sql)
    assert_eq!(warnings[1].resource, "src/schema.sql");
    assert_eq!(warnings[1].severity, OverlapSeverity::Info);
    let schema_tasks = warnings[1].task_id_list();
    assert!(schema_tasks.contains(&task_c.id));
    assert!(schema_tasks.contains(&task_d.id));

    // Cleanup
    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

/// Acceptance Criteria:
/// - Warning visible in plan review screen before human approves the task
/// - Critical overlap blocks task approval until acknowledged
/// - Acknowledgment unblocks approval and updates warning state
#[tokio::test]
async fn test_warning_visible_in_plan_review_and_blocks_approval_until_acknowledged() {
    let Some(pool) = setup_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 6 Plan Review Overlap".to_string(),
            description: "Testing plan review warning visibility".to_string(),
        },
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "Architecture review".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "PLAN-01".to_string(),
            title: "Task 1 touching shared file".to_string(),
            description: "Touches core config".to_string(),
            affected_resources: vec!["crates/config.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "PLAN-02".to_string(),
            title: "Task 2 touching shared file".to_string(),
            description: "Also touches core config".to_string(),
            affected_resources: vec!["crates/config.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Advance to HumanReview
    TaskRepository::update_status(&pool, task1.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task2.id, TaskStatus::HumanReview).await.unwrap();

    // Detect and persist overlaps
    let warnings = OverlapDetector::detect_and_persist_for_project(&pool, project.id)
        .await
        .unwrap();
    assert_eq!(warnings.len(), 1);
    let warning_id = warnings[0].id;
    assert_eq!(warnings[0].severity, OverlapSeverity::Critical);

    // Attempting to approve Task 1 MUST FAIL due to unacknowledged critical overlap
    let approve_res = CommandHandler::execute_approve_task(&pool, task1.id, "Alice").await;
    assert!(approve_res.is_err(), "Approval must be blocked by unacknowledged critical overlap");

    // Initialize TUI AppState
    let mut state = AppState::new();
    state.active_project = Some(project.clone());
    let mut review_tasks = vec![
        ReviewTaskState::new(task1.clone()),
        ReviewTaskState::new(task2.clone()),
    ];
    OverlapDetector::populate_task_overlaps(&mut review_tasks, &warnings);

    // Verify warnings populated on task states
    assert_eq!(review_tasks[0].overlap_warnings.len(), 1);
    assert!(!review_tasks[0].overlap_warnings[0].acknowledged);
    assert_eq!(review_tasks[0].overlap_warnings[0].resource, "crates/config.rs");

    state.review_tasks = review_tasks;
    state.active_overlaps = warnings.clone();
    state.current_screen = CurrentScreen::PlanReview;
    state.selected_task_index = 0;


    // Render headless Plan Review screen to ensure card & details pane render warnings without panic
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &state)).expect("Render PlanReview");

    // Operator presses 'a' to acknowledge the overlap warning on Task 1
    let action = state.handle_key(make_key(KeyCode::Char('a')));
    assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id }));
    assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);
    assert!(state.active_overlaps.is_empty());

    // Outer handler processes acknowledgment in PostgreSQL
    CommandHandler::execute_acknowledge_overlap(&pool, warning_id).await.unwrap();

    // Now approving Task 1 succeeds!
    let approved_task = CommandHandler::execute_approve_task(&pool, task1.id, "Alice")
        .await
        .expect("Approval should succeed after acknowledgment");
    assert_eq!(approved_task.status, TaskStatus::Approved);

    // Cleanup
    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

/// Acceptance Criteria:
/// - Warning visible in dashboard for unacknowledged overlaps
/// - Unacknowledged warnings are highlighted and can be acknowledged from dashboard
#[tokio::test]
async fn test_warning_visible_in_dashboard_for_unacknowledged_overlaps() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    // with_mock_data includes 1 active unacknowledged critical overlap
    assert_eq!(state.active_overlaps.len(), 1);
    let warn = &state.active_overlaps[0];
    assert_eq!(warn.severity, OverlapSeverity::Critical);
    assert_eq!(warn.resource, "crates/db");
    let warn_id = warn.id;

    // Render Dashboard in headless test terminal
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &state)).expect("Render Dashboard with active overlap");

    // Operator acknowledges overlap directly from Dashboard via 'a'
    let action = state.handle_key(make_key(KeyCode::Char('a')));
    assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id: warn_id }));
    assert!(state.active_overlaps.is_empty());

    // Render again — verify clean display with 0 active overlaps
    terminal.draw(|f| render(f, &state)).expect("Render Dashboard with zero overlaps");
}

/// Acceptance Criteria:
/// - Dashboard updates live as agent events arrive (no full restart needed)
/// - Real-time updates from NATS events (async channel -> TUI state)
#[tokio::test]
async fn test_dashboard_updates_live_from_agent_events_channel() {
    let mut state = AppState::new();
    let agent_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let proj_id = Uuid::new_v4();

    state.active_project = Some(coordinator::domain::Project {
        id: proj_id,
        name: "Live Mesh Project".to_string(),
        description: "Testing live updates".to_string(),
        status: coordinator::domain::ProjectStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    state.agents = vec![coordinator::domain::Agent::mock(
        agent_id,
        "agent-backend (Alice)",
        AdapterType::Agy,
        vec!["rust".to_string()],
        coordinator::domain::AgentStatus::Idle,
    )];

    let task = coordinator::domain::Task {
        id: task_id,
        project_id: proj_id,
        short_id: "LIVE-1".to_string(),
        title: "Build microservice adapter".to_string(),
        description: "Implement gRPC interface".to_string(),
        status: TaskStatus::Approved,
        assigned_agent_id: None,
        affected_resources: json!(["src/grpc.rs"]),
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    state.review_tasks = vec![ReviewTaskState::new(task)];
    state.current_screen = CurrentScreen::Dashboard;

    // Verify initial dashboard state: Agent is IDLE, Task is Approved
    assert_eq!(state.agents[0].status, coordinator::domain::AgentStatus::Idle);
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(state.agents[0].current_task_id, None);

    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &state)).expect("Render initial dashboard");

    // Setup async update channel into TUI state
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();

    // 1. Incoming event: TaskStarted
    tx.send(TuiUpdateEvent::AgentMessage(AgentMessage::TaskStarted {
        agent_id,
        task_id,
        idempotency_key: "idem-live-1".to_string(),
        timestamp: Utc::now(),
    }))
    .unwrap();

    // Drain channel updates into state (same mechanism as TerminalApp::run_with_channel)
    while let Ok(update) = rx.try_recv() {
        state.apply_update(update);
    }

    // Verify state projected live
    assert_eq!(state.agents[0].status, coordinator::domain::AgentStatus::Busy);
    assert_eq!(state.agents[0].current_task_id, Some(task_id));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Executing);
    assert_eq!(state.review_tasks[0].task.assigned_agent_id, Some(agent_id));
    terminal.draw(|f| render(f, &state)).expect("Render Executing dashboard");

    // 2. Incoming event: ProgressUpdate
    tx.send(TuiUpdateEvent::AgentMessage(AgentMessage::ProgressUpdate {
        agent_id,
        task_id,
        percent: 75,
        message: "Compiling gRPC protos".to_string(),
        timestamp: Utc::now(),
    }))
    .unwrap();

    while let Ok(update) = rx.try_recv() {
        state.apply_update(update);
    }

    assert!(state.status_message.as_ref().unwrap().contains("75%"));
    terminal.draw(|f| render(f, &state)).expect("Render Progress dashboard");

    // 3. Incoming event: Blocked
    tx.send(TuiUpdateEvent::AgentMessage(AgentMessage::Blocked {
        agent_id,
        task_id,
        reason: "Waiting on upstream proto definition".to_string(),
        blocking_task_id: None,
        timestamp: Utc::now(),
    }))
    .unwrap();

    while let Ok(update) = rx.try_recv() {
        state.apply_update(update);
    }

    assert_eq!(state.agents[0].status, coordinator::domain::AgentStatus::Blocked);
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Blocked);
    terminal.draw(|f| render(f, &state)).expect("Render Blocked dashboard");

    // 4. Incoming event: Completed
    tx.send(TuiUpdateEvent::AgentMessage(AgentMessage::Completed {
        agent_id,
        task_id,
        summary: "gRPC adapter created and tested".to_string(),
        timestamp: Utc::now(),
    }))
    .unwrap();

    while let Ok(update) = rx.try_recv() {
        state.apply_update(update);
    }

    assert_eq!(state.agents[0].status, coordinator::domain::AgentStatus::Idle);
    assert_eq!(state.agents[0].current_task_id, None);
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Completed);
    terminal.draw(|f| render(f, &state)).expect("Render Completed dashboard");
}

/// Acceptance Criteria:
/// - Real NATS JetStream event publishing, ingestion by EventSubscriber, and forwarding into TUI
#[tokio::test]
async fn test_nats_subscriber_ingestion_and_channel_forwarding() {
    let Some(pool) = setup_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let (client, jetstream) = match connect(&nats_url, None).await {
        Ok(res) => res,
        Err(_) => {
            eprintln!("Skipping test: NATS not reachable");
            return;
        }
    };
    ensure_streams(&jetstream).await.unwrap();


    let consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();

    let agent_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    // Create Agent in DB
    let _agent = AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "NatsTestOwner".to_string(),
            api_key_hash: "hash".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Setup TUI channel
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentMessage>();

    // Publish TaskStarted message to JetStream
    let start_msg = AgentMessage::TaskStarted {
        agent_id,
        task_id,
        idempotency_key: format!("nats-test-{}", Uuid::new_v4()),
        timestamp: Utc::now(),
    };

    client
        .publish(
            format!("agents.{agent_id}.events"),
            serde_json::to_vec(&start_msg).unwrap().into(),
        )
        .await
        .unwrap();


    // Process event using subscriber with channel forwarder
    let processed = tokio::time::timeout(
        Duration::from_secs(5),
        EventSubscriber::process_one_event(&pool, &consumer, Some(&tx)),
    )
    .await
    .expect("Timeout processing NATS event")
    .expect("process_one_event failed");

    assert!(processed.is_some());
    let received_channel_msg = rx.try_recv().expect("Channel should receive AgentMessage");
    match received_channel_msg {
        AgentMessage::TaskStarted { agent_id: recv_agent, task_id: recv_task, .. } => {
            assert_eq!(recv_agent, agent_id);
            assert_eq!(recv_task, task_id);
        }
        other => panic!("Unexpected message received: {:?}", other),
    }

    // Cleanup agent
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}
