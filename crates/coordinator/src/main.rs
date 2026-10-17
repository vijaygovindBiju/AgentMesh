use anyhow::Result;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

pub mod cli;
use cli::CliArgs;

use coordinator::ai::{
    create_provider_from_env, AvailableAgentContext, PlanningRequest, PlanningService,
};
use coordinator::coordinator::{
    ApprovalGateError, CommandHandler, CoordinatorCore, CoordinatorState, OverlapDetector,
};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, TaskRepository,
};
use coordinator::domain::{ApprovalStatus, NewProject, TaskStatus};
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

    let _ = dotenvy::dotenv();

    // 2. Parse command-line arguments
    let args = match CliArgs::parse() {
        Ok(a) => a,
        Err(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    };

    if args.show_help {
        println!("{}", CliArgs::help_text());
        return Ok(());
    }

    if args.show_version {
        println!("{}", CliArgs::version_text());
        return Ok(());
    }

    info!("Starting AgentMesh Coordinator v0.1.0");

    let stdout_is_tty = crossterm::tty::IsTty::is_tty(&std::io::stdout());
    let is_headless = args.is_headless(stdout_is_tty);

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token =
        std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    // 3. Attempt connection to PostgreSQL and NATS JetStream
    let pool_res = create_pool(&db_url).await;
    let nats_res = connect(&nats_url, Some(&nats_token)).await;

    // Handle --status flag before entering daemon/TUI loops
    if args.status {
        match pool_res {
            Ok(pool) => {
                let _ = run_migrations(&pool).await;
                print_system_status(&pool).await?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: Failed to connect to database at {db_url}: {e}");
                std::process::exit(1);
            }
        }
    }

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

            // Execute coordinator startup crash recovery & delivery reconciliation
            info!("Executing coordinator startup crash recovery & delivery reconciliation...");
            match coordinator::reliability::CoordinatorRecoveryService::recover_on_startup(
                &pool,
                Some(&jetstream),
            )
            .await
            {
                Ok(report) => {
                    info!(
                        republished_deliveries = report.pending_deliveries_republished,
                        reclaimed_tasks = report.sweep_summary.tasks_reclaimed.len(),
                        stale_agents = report.sweep_summary.agents_timed_out.len(),
                        "Startup recovery completed"
                    );
                }
                Err(e) => {
                    error!(error = %e, "Startup recovery failed");
                }
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

            // Resolve CLI target project if specified
            let cli_target_project_id = if let Some(ref target) = args.project_id {
                if let Ok(target_uuid) = Uuid::parse_str(target) {
                    Some(target_uuid)
                } else {
                    ProjectRepository::list(&pool).await.ok().and_then(|projs| {
                        projs
                            .into_iter()
                            .find(|p| p.name.eq_ignore_ascii_case(target))
                            .map(|p| p.id)
                    })
                }
            } else {
                None
            };

            // Periodic stale task sweeper worker (every 5 seconds)
            let sweeper_pool = pool.clone();
            let sweeper_js = jetstream.clone();
            let sweeper_proj = cli_target_project_id;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    match coordinator::reliability::StaleTaskSweeper::sweep(
                        &sweeper_pool,
                        chrono::Duration::seconds(30),
                    )
                    .await
                    {
                        Ok(res) => {
                            if !res.tasks_reclaimed.is_empty() || !res.agents_timed_out.is_empty() {
                                info!(
                                    reclaimed_count = res.tasks_reclaimed.len(),
                                    timeout_count = res.agents_timed_out.len(),
                                    "Stale task sweep reclaimed stuck tasks; triggering assignment"
                                );
                                let _ = coordinator::coordinator::AssignmentService::assign_ready_tasks(
                                    &sweeper_pool,
                                    sweeper_proj,
                                    Some(&sweeper_js),
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "StaleTaskSweeper execution error");
                        }
                    }
                }
            });

            // Periodic task assignment and outbox reconciliation worker (every 2 seconds)
            let assign_worker_pool = pool.clone();
            let assign_worker_js = jetstream.clone();
            let assign_worker_proj = cli_target_project_id;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    // Reconcile timed-out pending deliveries
                    let _ =
                        coordinator::coordinator::AssignmentService::reconcile_pending_deliveries(
                            &assign_worker_pool,
                            Some(&assign_worker_js),
                        )
                        .await;
                    // Assign any approved ready tasks
                    let _ = coordinator::coordinator::AssignmentService::assign_ready_tasks(
                        &assign_worker_pool,
                        assign_worker_proj,
                        Some(&assign_worker_js),
                    )
                    .await;
                }
            });

            // Setup TUI communication channels
            let (tui_tx, tui_rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();

            // Periodic live metrics and diagnostics worker (every 2 seconds)
            let metrics_pool = pool.clone();
            let metrics_tx = tui_tx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                let mut known_event_ids = std::collections::HashSet::<uuid::Uuid>::new();
                loop {
                    interval.tick().await;
                    if let Ok(m) =
                        coordinator::observability::MetricsCollector::collect(&metrics_pool).await
                    {
                        let _ = metrics_tx.send(TuiUpdateEvent::Metrics(m));
                    }
                    if let Ok(events) =
                        coordinator::observability::CoordinatorEventRepository::find_recent(
                            &metrics_pool,
                            50,
                        )
                        .await
                    {
                        // Deduplicate and merge SQL events into recent_events ring buffer instead of overwriting,
                        // preserving live NATS events
                        for ev in events.into_iter().rev() {
                            if known_event_ids.insert(ev.id) {
                                let _ = metrics_tx.send(TuiUpdateEvent::ObservabilityEvent(ev));
                            }
                        }
                    }
                }
            });

            // Durable consumer for agent JetStream events -> forwarded to TUI
            let consumer_res = EventSubscriber::create_consumer(&jetstream).await;
            if let Ok(consumer) = consumer_res {
                let sub_pool = pool.clone();
                let sub_tx = tui_tx.clone();
                let sub_js = jetstream.clone();
                tokio::spawn(async move {
                    use futures::StreamExt;
                    if let Ok(mut messages) = consumer.messages().await {
                        while let Some(msg_result) = messages.next().await {
                            if let Ok(msg) = msg_result {
                                if let Ok(agent_msg) =
                                    serde_json::from_slice::<agent_protocol::AgentMessage>(
                                        &msg.payload,
                                    )
                                {
                                    let _ = msg.ack().await;
                                    let _ = EventSubscriber::handle_agent_message_with_jetstream(
                                        &sub_pool,
                                        agent_msg.clone(),
                                        Some(&sub_js),
                                    )
                                    .await;
                                    let _ = sub_tx.send(TuiUpdateEvent::AgentMessage(agent_msg));
                                }
                            }
                        }
                    }
                });
            }

            // Initialize coordinator core
            let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));

            // Build TUI AppState with live data
            let mut app_state = AppState::new();
            app_state.pg_connected = true;
            app_state.nats_connected = true;
            app_state.agents = AgentRepository::list(&pool).await.unwrap_or_default();

            // Load all existing projects from database
            let projects = ProjectRepository::list(&pool).await.unwrap_or_default();
            app_state.available_projects = projects.clone();
            let selected_project = if let Some(ref target) = args.project_id {
                let found = if let Ok(target_uuid) = Uuid::parse_str(target) {
                    projects.iter().find(|p| p.id == target_uuid).cloned()
                } else {
                    projects
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(target))
                        .cloned()
                };

                if let Some(ref p) = found {
                    info!(project_id = %p.id, project_name = %p.name, "Target project selected via CLI");
                } else {
                    warn!(target = %target, "Specified project ID or name not found in database");
                }
                found
            } else {
                projects.first().cloned()
            };

            if let Some(active_proj) = selected_project {
                coordinator.set_active_project(active_proj.id);
                app_state.active_project = Some(active_proj.clone());
                let (review_tasks, overlaps) =
                    load_project_review_state(&pool, active_proj.id).await?;
                app_state.review_tasks = review_tasks;
                app_state.active_overlaps = overlaps;
                if !app_state.review_tasks.is_empty() {
                    app_state.current_screen = CurrentScreen::PlanReview;
                }
            }

            // Spawn auto-approval worker if --auto-approve is enabled
            if args.auto_approve {
                info!("Auto-approve mode active: proposed tasks without critical overlap warnings will be approved automatically");
                let auto_pool = pool.clone();
                let auto_js = jetstream.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(2));
                    loop {
                        interval.tick().await;
                        let projects = ProjectRepository::list(&auto_pool).await.unwrap_or_default();
                        for proj in projects {
                            if let Ok(tasks) = TaskRepository::list_by_status(
                                &auto_pool,
                                proj.id,
                                TaskStatus::HumanReview,
                            )
                            .await
                            {
                                for task in tasks {
                                    match CommandHandler::execute_approve_task(
                                        &auto_pool,
                                        task.id,
                                        "AutoApproveDaemon",
                                    )
                                    .await
                                    {
                                        Ok(approved) => {
                                            info!(
                                                task_id = %task.id,
                                                short_id = %approved.short_id,
                                                "Auto-approved task without critical conflicts"
                                            );
                                            let _ = coordinator::coordinator::AssignmentService::assign_ready_tasks(
                                                &auto_pool,
                                                None,
                                                Some(&auto_js),
                                            )
                                            .await;
                                        }
                                        Err(ApprovalGateError::BlockedByCriticalOverlap {
                                            resources,
                                            ..
                                        }) => {
                                            warn!(
                                                task_id = %task.id,
                                                ?resources,
                                                "Auto-approval skipped: task blocked by critical resource conflict"
                                            );
                                        }
                                        Err(e) => {
                                            warn!(
                                                task_id = %task.id,
                                                error = %e,
                                                "Auto-approval error"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }

            if is_headless {
                info!("Running in non-interactive daemon mode. Registration, heartbeat, and JetStream listeners active.");
                drop(tui_rx);
                tokio::signal::ctrl_c().await?;
                info!("Coordinator shutting down gracefully");
                return Ok(());
            }

            // Launch interactive TUI connected to real coordinator
            let mut app = TerminalApp::new()?;
            let loop_pool = pool.clone();
            let mut loop_coord = coordinator;
            let action_tx = tui_tx.clone();

            app.run_with_channel(&mut app_state, tui_rx, |action, state| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        handle_tui_action(&loop_pool, &mut loop_coord, action, state, &action_tx).await;
                    });
                });
            })?;
        }

        (Err(e_pool), _) => {
            error!(error = %e_pool, "Failed to connect to PostgreSQL database at {db_url}");
            eprintln!("Error: Cannot connect to PostgreSQL database at {db_url}: {e_pool}\nPlease start infrastructure with: docker compose up -d");
            std::process::exit(1);
        }
        (_, Err(e_nats)) => {
            error!(error = %e_nats, "Failed to connect to NATS JetStream at {nats_url}");
            eprintln!("Error: Cannot connect to NATS JetStream at {nats_url}: {e_nats}\nPlease start infrastructure with: docker compose up -d");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Prints real-time system metrics, fleet status, and task summary to stdout for `--status`.
async fn print_system_status(pool: &sqlx::PgPool) -> Result<()> {
    let metrics = coordinator::observability::MetricsCollector::collect(pool).await?;
    let agents = AgentRepository::list(pool).await.unwrap_or_default();
    let projects = ProjectRepository::list(pool).await.unwrap_or_default();

    println!("========================================================================");
    println!("                     AgentMesh Fleet & System Status                    ");
    println!("========================================================================");
    println!("Projects ({} total):", projects.len());
    for p in projects.iter().take(5) {
        println!("  - [{}] {} (Status: {:?})", p.id, p.name, p.status);
    }
    if projects.len() > 5 {
        println!("  ... and {} more", projects.len() - 5);
    }
    println!();
    println!("Tasks Summary:");
    println!("  Total Tasks:          {}", metrics.total_tasks);
    println!("  Proposed / Review:    {}", metrics.tasks_proposed);
    println!("  Approved:             {}", metrics.tasks_approved);
    println!("  Executing / Assigned: {}", metrics.tasks_executing);
    println!("  Completed:            {}", metrics.tasks_completed);
    println!("  Failed:               {}", metrics.tasks_failed);
    println!("  Blocked:              {}", metrics.tasks_blocked);
    println!();
    println!("Agent Fleet Summary:");
    println!(
        "  Total Agents:         {} ({} Idle, {} Busy, {} Offline)",
        metrics.total_agents, metrics.agents_idle, metrics.agents_busy, metrics.agents_offline
    );
    println!(
        "  Fleet Health:         {} Healthy, {} Degraded, {} Unhealthy",
        metrics.agents_healthy, metrics.agents_degraded, metrics.agents_unhealthy
    );
    println!();
    if !agents.is_empty() {
        println!("Connected Agents:");
        for a in &agents {
            println!(
                "  - [{}] {:?} | Status: {:?} | Capabilities: [{}]",
                a.id,
                a.adapter_type,
                a.status,
                a.capabilities_list().join(", ")
            );
        }
        println!();
    }
    println!("Delivery & Reliability:");
    println!("  Total Deliveries:     {}", metrics.total_deliveries);
    println!("  Success Rate:         {:.1}%", metrics.delivery_success_rate_percent);
    println!("  Resource Conflicts:   {}", metrics.total_conflicts);
    println!("  Audit Alerts:         {}", metrics.total_audit_alerts);
    println!("========================================================================");

    Ok(())
}

/// Loads tasks, dependencies, and overlap warnings for a project into `ReviewTaskState`.
async fn load_project_review_state(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<(
    Vec<ReviewTaskState>,
    Vec<coordinator::domain::OverlapWarning>,
)> {
    let tasks = TaskRepository::list_by_project(pool, project_id).await?;
    let overlaps = OverlapWarningRepository::list_by_project(pool, project_id).await?;

    let mut review_tasks = Vec::new();
    for task in tasks {
        let deps = TaskRepository::list_dependencies(pool, task.id)
            .await
            .unwrap_or_default();
        let mut rts = ReviewTaskState::new(task);
        rts.dependencies = deps;
        review_tasks.push(rts);
    }
    OverlapDetector::populate_task_overlaps(&mut review_tasks, &overlaps);

    Ok((review_tasks, overlaps))
}

/// Dispatches a user action from the TUI to coordinator core and PostgreSQL.
/// Long operations (such as AI planning) run asynchronously without blocking the TUI event loop.
async fn handle_tui_action(
    pool: &sqlx::PgPool,
    coordinator: &mut CoordinatorCore,
    action: TuiAction,
    state: &mut AppState,
    action_tx: &tokio::sync::mpsc::UnboundedSender<TuiUpdateEvent>,
) {
    match action {
        TuiAction::SubmitProject { name, description } => {
            state.status_message = Some(format!("Decomposing project '{name}' with AI planner in background..."));
            state.current_screen = CurrentScreen::PlanReview;

            let pool = pool.clone();
            let action_tx = action_tx.clone();

            let new_project = NewProject {
                name: name.clone(),
                description: description.clone(),
            };
            let project = match ProjectRepository::create(&pool, &new_project).await {
                Ok(p) => {
                    coordinator.set_active_project(p.id);
                    state.active_project = Some(p.clone());
                    state.review_tasks.clear();
                    state.active_overlaps.clear();
                    state.selected_task_index = 0;
                    state.selected_dashboard_task_index = 0;
                    let all_projects = ProjectRepository::list(&pool).await.unwrap_or_default();
                    state.available_projects = all_projects.clone();
                    let _ = action_tx.send(TuiUpdateEvent::Projects(all_projects));
                    let _ = action_tx.send(TuiUpdateEvent::ActiveProject(p.clone()));
                    let _ = action_tx.send(TuiUpdateEvent::Tasks(vec![]));
                    let _ = action_tx.send(TuiUpdateEvent::Overlaps(vec![]));
                    p
                }
                Err(e) => {
                    state.status_message = Some(format!("Failed to create project: {e}"));
                    return;
                }
            };

            tokio::spawn(async move {

                // Discover repository Git identity if running inside a Git repository
                let current_repo = std::env::current_dir().unwrap_or_default();
                if let Ok(git_id) = coordinator::git::RepositoryIdentity::discover(&current_repo).await {
                    let _ = ProjectRepository::update_git_identity(
                        &pool,
                        project.id,
                        &git_id.repo_root.to_string_lossy(),
                        &git_id.base_branch,
                        Some(&git_id.head_commit_sha),
                    )
                    .await;
                }

                // Fetch available agents for planning
                let agents = AgentRepository::list(&pool).await.unwrap_or_default();
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
                let provider = match create_provider_from_env() {
                    Ok(p) => p,
                    Err(e) => {
                        error!(error = %e, "Failed to initialize AI planning provider");
                        let _ = action_tx.send(TuiUpdateEvent::StatusMessage(format!(
                            "AI Planning failed: {e}"
                        )));
                        return;
                    }
                };

                let repo_context = coordinator::ai::RepositoryScanner::scan(&current_repo)
                    .await
                    .ok();

                let plan_result = PlanningService::generate_and_persist_plan(
                    &pool,
                    provider.as_ref(),
                    &PlanningRequest {
                        project_id: project.id,
                        project_name: project.name.clone(),
                        project_description: project.description.clone(),
                        available_agents: agent_contexts,
                        existing_tasks: vec![],
                        repo_context,
                    },
                )
                .await;

                match plan_result {
                    Ok(_proposal_id) => {
                        // Load tasks & detect resource overlaps
                        if let Ok((review_tasks, overlaps)) =
                            load_project_review_state(&pool, project.id).await
                        {
                            let task_count = review_tasks.len();
                            let overlap_count = overlaps.len();
                            let _ = action_tx.send(TuiUpdateEvent::Tasks(review_tasks));
                            let _ = action_tx.send(TuiUpdateEvent::Overlaps(overlaps));
                            let _ = action_tx.send(TuiUpdateEvent::StatusMessage(format!(
                                "Project '{name}' planned: {task_count} tasks generated, {overlap_count} overlap warning(s). Review with [y/n/e/a]."
                            )));
                        }
                    }
                    Err(e) => {
                        let _ = action_tx.send(TuiUpdateEvent::StatusMessage(format!("AI Planning error: {e}")));
                    }
                }
            });
        }

        TuiAction::ApproveTask { task_id } => {
            match CommandHandler::execute_approve_task(pool, task_id, "Operator").await {
                Ok(task) => {
                    if let Some(rt) = state
                        .review_tasks
                        .iter_mut()
                        .find(|rt| rt.task.id == task_id)
                    {
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
                                if let Ok((rts, _)) = load_project_review_state(pool, proj.id).await
                                {
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
                    if let Some(rt) = state
                        .review_tasks
                        .iter_mut()
                        .find(|rt| rt.task.id == task_id)
                    {
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

        TuiAction::EditTaskDescription {
            task_id,
            new_description,
        } => {
            match CommandHandler::execute_edit_and_approve_task(
                pool,
                task_id,
                &new_description,
                "Operator",
            )
            .await
            {
                Ok(task) => {
                    if let Some(rt) = state
                        .review_tasks
                        .iter_mut()
                        .find(|rt| rt.task.id == task_id)
                    {
                        rt.task.description = task.description.clone();
                        rt.task.status = task.status;
                        rt.human_decision =
                            Some(coordinator::domain::ApprovalStatus::EditedAndApproved);
                    }
                    state.status_message =
                        Some(format!("Task {} updated and approved.", task.short_id));
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
                    state.status_message =
                        Some("Overlap warning acknowledged. Approval unblocked.".to_string());
                }
                Ok(false) => {
                    state.status_message = Some("Overlap warning not found.".to_string());
                }
                Err(e) => {
                    state.status_message = Some(format!("Acknowledge error: {e}"));
                }
            }
        }

        TuiAction::CancelTask { task_id } => {
            let cmd = coordinator::coordinator::CoordinatorCommand::CancelTask {
                task_id,
                reason: "Cancelled by operator via TUI".to_string(),
            };
            match coordinator.handle_command(cmd).await {
                Ok(_) => {
                    if let Some(rt) = state
                        .review_tasks
                        .iter_mut()
                        .find(|rt| rt.task.id == task_id)
                    {
                        rt.task.status = TaskStatus::Cancelled;
                    }
                    state.status_message = Some(format!("Task {task_id} cancelled."));
                }
                Err(e) => {
                    state.status_message = Some(format!("Cancel error: {e}"));
                }
            }
        }

        TuiAction::RefreshData => {
            state.agents = AgentRepository::list(pool).await.unwrap_or_default();
            state.available_projects = ProjectRepository::list(pool).await.unwrap_or_default();
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

        TuiAction::LoadProject { project_id } => {
            state.status_message = Some(format!("Loading project {project_id}..."));
            if let Ok(Some(proj)) = ProjectRepository::find_by_id(pool, project_id).await {
                coordinator.set_active_project(proj.id);
                state.active_project = Some(proj.clone());
                let _ = action_tx.send(TuiUpdateEvent::ActiveProject(proj.clone()));
                if let Ok((rts, overlaps)) = load_project_review_state(pool, project_id).await {
                    let _ = action_tx.send(TuiUpdateEvent::Tasks(rts.clone()));
                    let _ = action_tx.send(TuiUpdateEvent::Overlaps(overlaps.clone()));
                    state.review_tasks = rts;
                    state.active_overlaps = overlaps;
                    state.selected_task_index = 0;
                    state.selected_dashboard_task_index = 0;
                }
                state.status_message = Some(format!("Loaded project '{}'.", proj.name));
            } else {
                state.status_message = Some(format!("Project {project_id} not found."));
            }
        }

        TuiAction::RetryTask { task_id } => {
            state.status_message = Some(format!("Retrying task {task_id}..."));
            match CommandHandler::execute_retry_task(pool, task_id, "operator").await {
                Ok(updated_task) => {
                    if let Some(rt) = state
                        .review_tasks
                        .iter_mut()
                        .find(|rt| rt.task.id == task_id)
                    {
                        rt.task = updated_task;
                        rt.human_decision = Some(ApprovalStatus::Approved);
                    }
                    state.status_message = Some(format!("Task {task_id} reset to Approved."));
                    let _ = action_tx.send(TuiUpdateEvent::StatusMessage(format!(
                        "Task {task_id} retried and reset to Approved."
                    )));
                    if let Some(proj) = &state.active_project {
                        if let Ok((rts, _)) = load_project_review_state(pool, proj.id).await {
                            let _ = action_tx.send(TuiUpdateEvent::Tasks(rts.clone()));
                            state.review_tasks = rts;
                        }
                    }
                }
                Err(e) => {
                    state.status_message = Some(format!("Retry error: {e}"));
                }
            }
        }

        TuiAction::InspectTaskTimeline { task_id } => {
            state.status_message = Some(format!("Inspecting timeline for task {task_id}..."));
            let query_pool = pool.clone();
            let query_tx = action_tx.clone();
            tokio::spawn(async move {
                if let Ok(timeline) = coordinator::observability::TimelineService::build_task_timeline(&query_pool, task_id).await {
                    let _ = query_tx.send(TuiUpdateEvent::Timeline(timeline));
                }
            });
            state.show_timeline_modal = true;
        }
    }
}
