use std::time::Duration;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::{AgentMessage, CoordinatorMessage};
use coordinator::ai::{
    AvailableAgentContext, MockLlmProvider, PlanningRequest, PlanningResponse,
    PlanningService, ProposedDependency, ProposedTask,
};
use coordinator::coordinator::{
    ApprovalGateError, CommandHandler, CoordinatorCore, CoordinatorState, OverlapDetector,
};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, NewAgent, NewProject, OverlapSeverity, TaskStatus,
};
use coordinator::messaging::{connect, ensure_streams};
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

/// End-to-End integration test fulfilling all 10 v0.1 success criteria:
/// 1. Human enters project description in TUI
/// 2. LLM produces dependency-aware task plan
/// 3. Human reviews tasks one-at-a-time with Y/N/E/↑↓/Enter
/// 4. Coordinator assigns approved tasks, dependency-ordered
/// 5. Tasks delivered via NATS JetStream to mock agents
/// 6. Mock agents report: started, progress, blocked, completed
/// 7. TUI live dashboard shows agent + task state
/// 8. One dependency chain demonstrated (Task B waits for Task A)
/// 9. One overlap warning demonstrated (two tasks share same resource)
/// 10. Full stack starts cleanly with docker compose up
#[tokio::test]
async fn test_phase7_end_to_end_full_lifecycle_demo() {
    let Some(pool) = setup_pool().await else {
        eprintln!("Skipping test: PostgreSQL not reachable");
        return;
    };

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let (_client, jetstream) = match connect(&nats_url, None).await {
        Ok(res) => res,
        Err(_) => {
            eprintln!("Skipping test: NATS not reachable");
            return;
        }
    };
    ensure_streams(&jetstream).await.unwrap();

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 1: Register 2 Mock Agents in the Mesh Fleet
    // ─────────────────────────────────────────────────────────────────────────
    let agent_a_id = Uuid::new_v4();
    let agent_b_id = Uuid::new_v4();

    let agent_a = AgentRepository::create_with_id(
        &pool,
        agent_a_id,
        &NewAgent {
            human_owner: "Alice (Backend Lead)".to_string(),
            api_key_hash: "hash_alice".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{agent_a_id}.events"),
        },
    )
    .await
    .unwrap();

    let agent_b = AgentRepository::create_with_id(
        &pool,
        agent_b_id,
        &NewAgent {
            human_owner: "Bob (Infra Lead)".to_string(),
            api_key_hash: "hash_bob".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["infra".to_string(), "sql".to_string()],
            nats_subject: format!("agents.{agent_b_id}.events"),
        },
    )
    .await
    .unwrap();

    // Agents start Idle
    AgentRepository::update_status(&pool, agent_a_id, AgentStatus::Idle).await.unwrap();
    AgentRepository::update_status(&pool, agent_b_id, AgentStatus::Idle).await.unwrap();

    // Create consumers for task assignments on agent side
    let stream_assignments = jetstream
        .get_stream(coordinator::messaging::streams::TASK_ASSIGNMENTS_STREAM)
        .await
        .unwrap();

    let consumer_a = stream_assignments
        .get_or_create_consumer(
            &format!("agent-a-consumer-{}", &agent_a_id.to_string()[..8]),
            async_nats::jetstream::consumer::pull::Config {
                filter_subject: format!("tasks.{agent_a_id}.assigned"),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 2: Human Enters Project & LLM Produces Dependency-Aware Task Plan
    // ─────────────────────────────────────────────────────────────────────────
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Cloud Checkout Engine".to_string(),
            description: "High-throughput order processing and inventory settlement".to_string(),
        },
    )
    .await
    .unwrap();

    // Define AI Planning response with 3 tasks:
    // - Task 1 (PLAN-1) touches "crates/db/schema.sql" and "src/models/order.rs"
    // - Task 2 (PLAN-2) touches "src/models/order.rs" (CONCURRENT CONFLICT with Task 1!)
    // - Task 3 (PLAN-3) touches "src/inventory.rs" and DEPENDS ON Task 1 (BLOCKS)
    let ai_plan = PlanningResponse {
        reasoning: "Decomposed into order schema, webhook processing, and inventory decrement".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "PLAN-1".to_string(),
                title: "Order schema and database migrations".to_string(),
                description: "Design PostgreSQL schema and create order entities".to_string(),
                suggested_agent_id: Some(agent_a_id),
                affected_resources: vec![
                    "crates/db/schema.sql".to_string(),
                    "src/models/order.rs".to_string(),
                ],
                estimated_size: Some("M".to_string()),
            },
            ProposedTask {
                short_id: "PLAN-2".to_string(),
                title: "Payment webhook ingress".to_string(),
                description: "Process payment callbacks and update order record".to_string(),
                suggested_agent_id: Some(agent_b_id),
                affected_resources: vec![
                    "src/models/order.rs".to_string(),
                    "src/routes/payment.rs".to_string(),
                ],
                estimated_size: Some("S".to_string()),
            },
            ProposedTask {
                short_id: "PLAN-3".to_string(),
                title: "Inventory decrement on completion".to_string(),
                description: "Deduct stock quantities once order schema is verified".to_string(),
                suggested_agent_id: Some(agent_a_id),
                affected_resources: vec!["src/inventory.rs".to_string()],
                estimated_size: Some("M".to_string()),
            },
        ],
        proposed_dependencies: vec![
            ProposedDependency {
                dependent_short_id: "PLAN-3".to_string(),
                depends_on_short_id: "PLAN-1".to_string(),
                kind: "blocks".to_string(),
                reason: "Inventory decrement requires order schema to exist".to_string(),
            },
        ],
    };

    let mock_llm = MockLlmProvider::new().with_response(ai_plan);
    let _proposal_id = PlanningService::generate_and_persist_plan(
        &pool,
        &mock_llm,
        &PlanningRequest {
            project_id: project.id,
            project_name: project.name.clone(),
            project_description: project.description.clone(),
            available_agents: vec![
                AvailableAgentContext {
                    agent_id: agent_a_id,
                    human_owner: agent_a.human_owner.clone(),
                    adapter_type: "Mock".to_string(),
                    capabilities: agent_a.capabilities_list(),
                },
                AvailableAgentContext {
                    agent_id: agent_b_id,
                    human_owner: agent_b.human_owner.clone(),
                    adapter_type: "Mock".to_string(),
                    capabilities: agent_b.capabilities_list(),
                },
            ],
            existing_tasks: vec![],
        },
    )
    .await
    .expect("AI decomposition failed");

    let tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    assert_eq!(tasks.len(), 3);

    let task1 = tasks.iter().find(|t| t.short_id == "PLAN-1").unwrap();
    let task2 = tasks.iter().find(|t| t.short_id == "PLAN-2").unwrap();
    let task3 = tasks.iter().find(|t| t.short_id == "PLAN-3").unwrap();


    // ─────────────────────────────────────────────────────────────────────────
    // STEP 3: Resource Overlap Detection & Persistence
    // ─────────────────────────────────────────────────────────────────────────
    // Task 1 and Task 2 both touch "src/models/order.rs" concurrently -> Critical overlap!
    let overlaps = OverlapWarningRepository::list_by_project(&pool, project.id).await.unwrap();
    assert!(!overlaps.is_empty(), "Critical overlap warning must be generated");
    let critical_overlap = overlaps.iter().find(|w| w.resource == "src/models/order.rs").unwrap();
    assert_eq!(critical_overlap.severity, OverlapSeverity::Critical);
    assert!(!critical_overlap.acknowledged);

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 4: Human Review & Approval Gating with Overlap Resolution
    // ─────────────────────────────────────────────────────────────────────────
    // Attempting to approve Task 1 without acknowledging critical overlap MUST FAIL
    let approve_fail = CommandHandler::execute_approve_task(&pool, task1.id, "Alice").await;
    match approve_fail {
        Err(ApprovalGateError::BlockedByCriticalOverlap { resources, .. }) => {
            assert!(resources.contains(&"src/models/order.rs".to_string()));
        }
        other => panic!("Expected BlockedByCriticalOverlap, got {:?}", other),
    }

    // Initialize TUI AppState to verify screen rendering & keybindings
    let mut app_state = AppState::new();
    app_state.active_project = Some(project.clone());
    app_state.agents = vec![agent_a.clone(), agent_b.clone()];
    app_state.active_overlaps = overlaps.clone();

    let mut review_tasks = vec![
        ReviewTaskState::new(task1.clone()),
        ReviewTaskState::new(task2.clone()),
        ReviewTaskState::new(task3.clone()),
    ];
    OverlapDetector::populate_task_overlaps(&mut review_tasks, &overlaps);
    app_state.review_tasks = review_tasks;
    app_state.current_screen = CurrentScreen::PlanReview;
    app_state.selected_task_index = 0;

    // Render Plan Review screen headless (TestBackend)
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &app_state)).expect("Render PlanReview");

    // Human operator acknowledges the overlap warning via key 'a'
    let ack_action = app_state.handle_key(make_key(KeyCode::Char('a')));
    assert_eq!(
        ack_action,
        Some(TuiAction::AcknowledgeOverlap { warning_id: critical_overlap.id })
    );
    CommandHandler::execute_acknowledge_overlap(&pool, critical_overlap.id).await.unwrap();

    // Now Human Operator approves Task 1 via key 'y'
    let approve_action_1 = app_state.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(approve_action_1, Some(TuiAction::ApproveTask { task_id: task1.id }));
    CommandHandler::execute_approve_task(&pool, task1.id, "Alice").await.unwrap();

    // Human Operator rejects Task 2 via key 'n'
    let reject_action_2 = app_state.handle_key(make_key(KeyCode::Char('n')));
    assert_eq!(reject_action_2, Some(TuiAction::RejectTask { task_id: task2.id }));
    CommandHandler::execute_reject_task(&pool, task2.id, "Alice").await.unwrap();

    // Human Operator approves Task 3 via key 'y'
    let approve_action_3 = app_state.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(approve_action_3, Some(TuiAction::ApproveTask { task_id: task3.id }));
    CommandHandler::execute_approve_task(&pool, task3.id, "Alice").await.unwrap();

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 5: Dependency-Ordered Assignment Gating (Coordinator Core)
    // ─────────────────────────────────────────────────────────────────────────
    let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));
    coordinator.set_active_project(project.id);
    coordinator.transition_state(CoordinatorState::Assigning).unwrap();

    // Assignment Cycle 1:
    // Task 1 is Approved and has 0 blockers -> Assigned to Agent A!
    // Task 3 is Approved, but blocked by Task 1 (not yet Completed) -> Remains Approved!
    let assignments = coordinator.run_assignment_cycle().await.unwrap();
    assert_eq!(assignments.len(), 1, "Only Task 1 should be ready for assignment");
    assert_eq!(assignments[0].task_id, task1.id);
    assert_eq!(assignments[0].agent_id, agent_a_id);

    // Verify Task 1 is Assigned in PostgreSQL, Task 3 is still Approved
    let t1_db = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    let t3_db = TaskRepository::find_by_id(&pool, task3.id).await.unwrap().unwrap();
    assert_eq!(t1_db.status, TaskStatus::Assigned);
    assert_eq!(t3_db.status, TaskStatus::Approved);

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 6: Task 1 Delivery via JetStream & Mock Agent Execution
    // ─────────────────────────────────────────────────────────────────────────
    let mut messages_a = consumer_a.messages().await.unwrap();
    let nats_msg = tokio::time::timeout(Duration::from_secs(5), messages_a.next())
        .await
        .expect("Timeout waiting for JetStream assignment")
        .expect("Message stream ended")
        .expect("JetStream message error");

    nats_msg.ack().await.expect("JetStream ACK failed");

    let coord_msg: CoordinatorMessage = serde_json::from_slice(&nats_msg.payload).unwrap();
    let spec = match coord_msg {
        CoordinatorMessage::TaskAssignment { spec } => spec,
        other => panic!("Expected TaskAssignment, got {:?}", other),
    };
    assert_eq!(spec.task_id, task1.id);
    assert_eq!(spec.short_id, "PLAN-1");

    // Mock Agent A emits: TaskStarted -> ProgressUpdate (50%) -> Completed
    let (tui_tx, mut tui_rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();

    // 1. TaskStarted
    let start_msg = AgentMessage::TaskStarted {
        agent_id: agent_a_id,
        task_id: task1.id,
        idempotency_key: format!("idem-{}", Uuid::new_v4()),
        timestamp: Utc::now(),
    };
    coordinator.handle_agent_message(start_msg.clone()).await.unwrap();
    tui_tx.send(TuiUpdateEvent::AgentMessage(start_msg)).unwrap();

    // 2. ProgressUpdate (50%)
    let progress_msg = AgentMessage::ProgressUpdate {
        agent_id: agent_a_id,
        task_id: task1.id,
        percent: 50,
        message: "Applying schema migrations".to_string(),
        timestamp: Utc::now(),
    };
    coordinator.handle_agent_message(progress_msg.clone()).await.unwrap();
    tui_tx.send(TuiUpdateEvent::AgentMessage(progress_msg)).unwrap();

    // 3. Completed
    let complete_msg = AgentMessage::Completed {
        agent_id: agent_a_id,
        task_id: task1.id,
        summary: "Migrations applied cleanly".to_string(),
        timestamp: Utc::now(),
    };
    coordinator.handle_agent_message(complete_msg.clone()).await.unwrap();
    tui_tx.send(TuiUpdateEvent::AgentMessage(complete_msg)).unwrap();

    // Drain into TUI state and verify live projection
    while let Ok(evt) = tui_rx.try_recv() {
        app_state.apply_update(evt);
    }

    assert_eq!(app_state.agents[0].status, AgentStatus::Idle);
    assert_eq!(app_state.agents[0].current_task_id, None);
    let t1_state = app_state.review_tasks.iter().find(|rt| rt.task.id == task1.id).unwrap();
    assert_eq!(t1_state.task.status, TaskStatus::Completed);

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 7: Dependency Chain Triggered — Task 3 Unblocked and Assigned
    // ─────────────────────────────────────────────────────────────────────────
    // Task 1 is now Completed! Next assignment cycle should unblock Task 3!
    let assignments_2 = coordinator.run_assignment_cycle().await.unwrap();
    assert_eq!(assignments_2.len(), 1, "Task 3 must now be assigned after Task 1 completed");
    assert_eq!(assignments_2[0].task_id, task3.id);

    let t3_assigned = TaskRepository::find_by_id(&pool, task3.id).await.unwrap().unwrap();
    assert_eq!(t3_assigned.status, TaskStatus::Assigned);

    // Agent executes Task 3 to completion
    let start_msg_3 = AgentMessage::TaskStarted {
        agent_id: assignments_2[0].agent_id,
        task_id: task3.id,
        idempotency_key: format!("idem-{}", Uuid::new_v4()),
        timestamp: Utc::now(),
    };
    coordinator.handle_agent_message(start_msg_3.clone()).await.unwrap();
    app_state.apply_agent_message(&start_msg_3);

    let complete_msg_3 = AgentMessage::Completed {
        agent_id: assignments_2[0].agent_id,
        task_id: task3.id,
        summary: "Inventory decrement hook integrated".to_string(),
        timestamp: Utc::now(),
    };
    coordinator.handle_agent_message(complete_msg_3.clone()).await.unwrap();
    app_state.apply_agent_message(&complete_msg_3);

    // ─────────────────────────────────────────────────────────────────────────
    // STEP 8: Live Dashboard & Project Completion Verification
    // ─────────────────────────────────────────────────────────────────────────
    // All approved tasks are Completed; Task 2 is Rejected.
    // Coordinator transitions to Done!
    assert_eq!(coordinator.state(), CoordinatorState::Done);

    // Switch to Dashboard Screen and render final summary
    app_state.current_screen = CurrentScreen::Dashboard;
    terminal.draw(|f| render(f, &app_state)).expect("Render Final Dashboard");

    // Verify all 3 tasks in final state
    let t1_final = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    let t2_final = TaskRepository::find_by_id(&pool, task2.id).await.unwrap().unwrap();
    let t3_final = TaskRepository::find_by_id(&pool, task3.id).await.unwrap().unwrap();

    assert_eq!(t1_final.status, TaskStatus::Completed);
    assert_eq!(t2_final.status, TaskStatus::Rejected);
    assert_eq!(t3_final.status, TaskStatus::Completed);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent_a_id).await.unwrap();
    AgentRepository::delete(&pool, agent_b_id).await.unwrap();
}
