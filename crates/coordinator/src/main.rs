use std::time::Duration;
use anyhow::Result;
use tracing::{error, info, warn};
use uuid::Uuid;

use coordinator::ai::{create_provider_from_env, AvailableAgentContext, PlanningRequest, PlanningService};
use coordinator::coordinator::{
    ApprovalGateError, CommandHandler, CoordinatorCore, CoordinatorState, OverlapDetector,
};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, TaskRepository,
};
use coordinator::domain::{AdapterType, AgentStatus, NewAgent, NewProject, TaskStatus};
use coordinator::messaging::{
    connect, ensure_streams, EventSubscriber, HeartbeatMonitor, RegistrationHandler,
};
use coordinator::tui::{
    AppState, CurrentScreen, ReviewTaskState, TerminalApp, TuiAction, TuiUpdateEvent,
};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize tracing subscriber to stderr (so TUI stdout is not corrupted)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting AgentMesh Coordinator v0.1.0");

    let _ = dotenvy::dotenv();
    let is_interactive = crossterm::tty::IsTty::is_tty(&std::io::stdout());

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    // 2. Attempt connection to PostgreSQL and NATS JetStream
    let pool_res = create_pool(&db_url).await;
    let nats_res = connect(&nats_url, None).await;

    match (pool_res, nats_res) {
        (Ok(pool), Ok((nats_client, jetstream))) => {
            info!("Connected to PostgreSQL and NATS JetStream successfully");

            // Run database migrations
            if let Err(e) = run_migrations(&pool).await {
                error!(error = %e, "Failed to run database migrations");
            }

            // Ensure JetStream streams
            if let Err(e) = ensure_streams(&jetstream).await {
                error!(error = %e, "Failed to ensure NATS JetStream streams");
            }

            // Spawn background listeners
            let reg_client = nats_client.clone();
            let reg_pool = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = RegistrationHandler::start_listener(reg_client, reg_pool).await {
                    error!(error = %e, "RegistrationHandler listener exited with error");
                }
            });

            let hb_client = nats_client.clone();
            let hb_pool = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = HeartbeatMonitor::start_listener(hb_client, hb_pool).await {
                    error!(error = %e, "HeartbeatMonitor listener exited with error");
                }
            });

            // Periodic agent heartbeat timeout monitor (every 5 seconds)
            let timeout_pool = pool.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let _ = HeartbeatMonitor::check_timeouts(&timeout_pool, Duration::from_secs(30)).await;
                }
            });

            // Setup TUI communication channels
            let (tui_tx, tui_rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();

            // Durable consumer for agent JetStream events -> forwarded to TUI
            let consumer_res = EventSubscriber::create_consumer(&jetstream).await;
            if let Ok(consumer) = consumer_res {
                let sub_pool = pool.clone();
                let sub_tx = tui_tx.clone();
                tokio::spawn(async move {
                    use futures::StreamExt;
                    if let Ok(mut messages) = consumer.messages().await {
                        while let Some(msg_result) = messages.next().await {
                            if let Ok(msg) = msg_result {
                                if let Ok(agent_msg) = serde_json::from_slice::<agent_protocol::AgentMessage>(&msg.payload) {
                                    let _ = msg.ack().await;
                                    let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg.clone()).await;
                                    let _ = sub_tx.send(TuiUpdateEvent::AgentMessage(agent_msg));
                                }
                            }
                        }
                    }
                });
            }

            // Seed default mock agents if fleet is empty
            let existing_agents = AgentRepository::list(&pool).await.unwrap_or_default();
            if existing_agents.is_empty() {
                info!("Seeding initial mock agents into the fleet");
                let a1_id = Uuid::new_v4();
                let _ = AgentRepository::create_with_id(
                    &pool,
                    a1_id,
                    &NewAgent {
                        human_owner: "Alice (Backend Lead)".to_string(),
                        api_key_hash: "seed_alice".to_string(),
                        adapter_type: AdapterType::Mock,
                        capabilities: vec!["backend".to_string(), "rust".to_string()],
                        nats_subject: format!("agents.{a1_id}.events"),
                    },
                )
                .await;
                let _ = AgentRepository::update_status(&pool, a1_id, AgentStatus::Idle).await;

                let a2_id = Uuid::new_v4();
                let _ = AgentRepository::create_with_id(
                    &pool,
                    a2_id,
                    &NewAgent {
                        human_owner: "Bob (Infra Lead)".to_string(),
                        api_key_hash: "seed_bob".to_string(),
                        adapter_type: AdapterType::Mock,
                        capabilities: vec!["infra".to_string(), "sql".to_string(), "nats".to_string()],
                        nats_subject: format!("agents.{a2_id}.events"),
                    },
                )
                .await;
                let _ = AgentRepository::update_status(&pool, a2_id, AgentStatus::Idle).await;
            }

            // Initialize coordinator core
            let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));

            // Build TUI AppState with live data
            let mut app_state = AppState::new();
            app_state.agents = AgentRepository::list(&pool).await.unwrap_or_default();

            // Check if there is an existing active project to resume
            let projects = ProjectRepository::list(&pool).await.unwrap_or_default();
            if let Some(active_proj) = projects.first() {
                coordinator.set_active_project(active_proj.id);
                app_state.active_project = Some(active_proj.clone());
                let (review_tasks, overlaps) = load_project_review_state(&pool, active_proj.id).await?;
                app_state.review_tasks = review_tasks;
                app_state.active_overlaps = overlaps;
                if !app_state.review_tasks.is_empty() {
                    app_state.current_screen = CurrentScreen::PlanReview;
                }
            }

            if !is_interactive {
                info!("Running in non-interactive mode. Infrastructure healthy, all background services active.");
                return Ok(());
            }

            // Launch interactive TUI connected to real coordinator
            let mut app = TerminalApp::new()?;
            let loop_pool = pool.clone();
            let mut loop_coord = coordinator;

            app.run_with_channel(&mut app_state, tui_rx, |action, state| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        handle_tui_action(&loop_pool, &mut loop_coord, action, state).await;
                    });
                });
            })?;
        }

        _ => {
            warn!("PostgreSQL or NATS JetStream not reachable. Falling back to Standalone Mock Mode.");

            if !is_interactive {
                info!("Non-interactive terminal detected. Start PostgreSQL & NATS with `docker compose up -d`.");
                return Ok(());
            }

            let mut state = AppState::new().with_mock_data();
            state.status_message = Some(
                "AgentMesh Demo Mode (PostgreSQL / NATS offline). Full interactive UI active. Tab to switch.".to_string(),
            );
            let mut app = TerminalApp::new()?;
            app.run(&mut state, |action, app_state| {
                match action {
                    TuiAction::ApproveTask { task_id } => {
                        if let Some(rt) = app_state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                            rt.task.status = TaskStatus::Approved;
                            app_state.status_message = Some(format!("Task {} approved (mock).", rt.task.short_id));
                        }
                    }
                    TuiAction::RejectTask { task_id } => {
                        if let Some(rt) = app_state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                            rt.task.status = TaskStatus::Rejected;
                            app_state.status_message = Some(format!("Task {} rejected (mock).", rt.task.short_id));
                        }
                    }
                    TuiAction::EditTaskDescription { task_id, new_description } => {
                        if let Some(rt) = app_state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                            rt.task.description = new_description;
                            rt.task.status = TaskStatus::Approved;
                            app_state.status_message = Some(format!("Task {} description updated & approved (mock).", rt.task.short_id));
                        }
                    }
                    TuiAction::AcknowledgeOverlap { warning_id } => {
                        app_state.active_overlaps.retain(|w| w.id != warning_id);
                        for rt in &mut app_state.review_tasks {
                            for w in &mut rt.overlap_warnings {
                                if w.id == warning_id {
                                    w.acknowledged = true;
                                }
                            }
                        }
                        app_state.status_message = Some("Resource overlap acknowledged (mock).".to_string());
                    }
                    TuiAction::RefreshData => {
                        app_state.status_message = Some("Refreshed (mock).".to_string());
                    }
                    _ => {}
                }
            })?;
        }
    }

    Ok(())
}

/// Loads tasks, dependencies, and overlap warnings for a project into `ReviewTaskState`.
async fn load_project_review_state(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<(Vec<ReviewTaskState>, Vec<coordinator::domain::OverlapWarning>)> {
    let tasks = TaskRepository::list_by_project(pool, project_id).await?;
    let overlaps = OverlapWarningRepository::list_by_project(pool, project_id).await?;

    let mut review_tasks = Vec::new();
    for task in tasks {
        let deps = TaskRepository::list_dependencies(pool, task.id).await.unwrap_or_default();
        let mut rts = ReviewTaskState::new(task);
        rts.dependencies = deps;
        review_tasks.push(rts);
    }
    OverlapDetector::populate_task_overlaps(&mut review_tasks, &overlaps);

    Ok((review_tasks, overlaps))
}

/// Dispatches a user action from the TUI to coordinator core and PostgreSQL.
async fn handle_tui_action(
    pool: &sqlx::PgPool,
    coordinator: &mut CoordinatorCore,
    action: TuiAction,
    state: &mut AppState,
) {
    match action {
        TuiAction::SubmitProject { name, description } => {
            state.status_message = Some(format!("Decomposing project '{name}' with AI planner..."));

            let project = match ProjectRepository::create(
                pool,
                &NewProject {
                    name: name.clone(),
                    description: description.clone(),
                },
            )
            .await
            {
                Ok(p) => p,
                Err(e) => {
                    state.status_message = Some(format!("Failed to create project: {e}"));
                    return;
                }
            };

            state.active_project = Some(project.clone());
            coordinator.set_active_project(project.id);
            let _ = coordinator.transition_state(CoordinatorState::Planning);

            // Fetch available agents for planning
            let agents = AgentRepository::list(pool).await.unwrap_or_default();
            let agent_contexts: Vec<AvailableAgentContext> = agents
                .iter()
                .map(|a| AvailableAgentContext {
                    agent_id: a.id,
                    human_owner: a.human_owner.clone(),
                    adapter_type: format!("{:?}", a.adapter_type),
                    capabilities: a.capabilities_list(),
                })
                .collect();

            // Run AI Planning
            let provider = create_provider_from_env().unwrap_or_else(|_| {
                std::sync::Arc::new(coordinator::ai::MockLlmProvider::new())
            });

            let plan_result = PlanningService::generate_and_persist_plan(
                pool,
                provider.as_ref(),
                &PlanningRequest {
                    project_id: project.id,
                    project_name: project.name.clone(),
                    project_description: project.description.clone(),
                    available_agents: agent_contexts,
                    existing_tasks: vec![],
                },
            )
            .await;

            match plan_result {
                Ok(_proposal_id) => {
                    let _ = coordinator.transition_state(CoordinatorState::HumanReview);

                    // Load tasks & detect resource overlaps
                    if let Ok((review_tasks, overlaps)) = load_project_review_state(pool, project.id).await {
                        let task_count = review_tasks.len();
                        let overlap_count = overlaps.len();
                        state.review_tasks = review_tasks;
                        state.active_overlaps = overlaps;
                        state.current_screen = CurrentScreen::PlanReview;
                        state.selected_task_index = 0;
                        state.status_message = Some(format!(
                            "Project planned: {task_count} tasks generated, {overlap_count} overlap warning(s). Review with [y/n/e/a]."
                        ));
                    }
                }
                Err(e) => {
                    state.status_message = Some(format!("AI Planning error: {e}"));
                }
            }
        }

        TuiAction::ApproveTask { task_id } => {
            match CommandHandler::execute_approve_task(pool, task_id, "Operator").await {
                Ok(task) => {
                    if let Some(rt) = state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                        rt.task.status = task.status;
                        rt.human_decision = Some(coordinator::domain::ApprovalStatus::Approved);
                    }
                    state.status_message = Some(format!("Task {} approved.", task.short_id));

                    // Trigger coordinator assignment cycle for newly approved tasks
                    let _ = coordinator.transition_state(CoordinatorState::Assigning);
                    if let Ok(assigned) = coordinator.run_assignment_cycle().await {
                        if !assigned.is_empty() {
                            state.status_message = Some(format!(
                                "Task {} approved and dispatched to agent.",
                                task.short_id
                            ));
                            // Refresh tasks
                            if let Some(proj) = &state.active_project {
                                if let Ok((rts, _)) = load_project_review_state(pool, proj.id).await {
                                    state.review_tasks = rts;
                                }
                            }
                        }
                    }
                }
                Err(ApprovalGateError::BlockedByCriticalOverlap { resources, .. }) => {
                    state.status_message = Some(format!(
                        "APPROVAL BLOCKED: Critical conflict on [{}]. Press [a] to acknowledge first.",
                        resources.join(", ")
                    ));
                }
                Err(e) => {
                    state.status_message = Some(format!("Approve error: {e}"));
                }
            }
        }

        TuiAction::RejectTask { task_id } => {
            match CommandHandler::execute_reject_task(pool, task_id, "Operator").await {
                Ok(task) => {
                    if let Some(rt) = state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                        rt.task.status = task.status;
                        rt.human_decision = Some(coordinator::domain::ApprovalStatus::Rejected);
                    }
                    state.status_message = Some(format!("Task {} rejected.", task.short_id));
                }
                Err(e) => {
                    state.status_message = Some(format!("Reject error: {e}"));
                }
            }
        }

        TuiAction::EditTaskDescription { task_id, new_description } => {
            match CommandHandler::execute_edit_and_approve_task(pool, task_id, &new_description, "Operator").await {
                Ok(task) => {
                    if let Some(rt) = state.review_tasks.iter_mut().find(|rt| rt.task.id == task_id) {
                        rt.task.description = task.description.clone();
                        rt.task.status = task.status;
                        rt.human_decision = Some(coordinator::domain::ApprovalStatus::EditedAndApproved);
                    }
                    state.status_message = Some(format!("Task {} updated and approved.", task.short_id));
                    let _ = coordinator.run_assignment_cycle().await;
                }
                Err(ApprovalGateError::BlockedByCriticalOverlap { resources, .. }) => {
                    state.status_message = Some(format!(
                        "APPROVAL BLOCKED: Critical conflict on [{}]. Press [a] to acknowledge first.",
                        resources.join(", ")
                    ));
                }
                Err(e) => {
                    state.status_message = Some(format!("Edit error: {e}"));
                }
            }
        }

        TuiAction::AcknowledgeOverlap { warning_id } => {
            match CommandHandler::execute_acknowledge_overlap(pool, warning_id).await {
                Ok(true) => {
                    state.active_overlaps.retain(|w| w.id != warning_id);
                    for rt in &mut state.review_tasks {
                        for w in &mut rt.overlap_warnings {
                            if w.id == warning_id {
                                w.acknowledged = true;
                            }
                        }
                    }
                    state.status_message = Some("Overlap warning acknowledged. Approval unblocked.".to_string());
                }
                Ok(false) => {
                    state.status_message = Some("Overlap warning not found.".to_string());
                }
                Err(e) => {
                    state.status_message = Some(format!("Acknowledge error: {e}"));
                }
            }
        }

        TuiAction::RefreshData => {
            state.agents = AgentRepository::list(pool).await.unwrap_or_default();
            if let Some(proj) = &state.active_project {
                if let Ok((rts, overlaps)) = load_project_review_state(pool, proj.id).await {
                    state.review_tasks = rts;
                    state.active_overlaps = overlaps;
                }
            }
            state.status_message = Some("Data refreshed from database.".to_string());
        }

        TuiAction::Quit => {
            state.should_quit = true;
        }
    }
}
