//! Verification Suite: Runtime Mock Data Removal & Empty State Resilience
//!
//! Asserts that:
//! 1. `AppState::new()` starts completely clean with zero mock data.
//! 2. All screens and widgets gracefully handle empty states without panics or crashes.
//! 3. Appropriate, human-friendly empty placeholder messages are displayed when no data exists.
//! 4. Navigation and keystrokes on empty states are safe and bounded.
//! 5. Terminal resizing on empty states behaves resiliently across various dimensions.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use uuid::Uuid;

use coordinator::domain::{
    AdapterType, Agent, AgentStatus, HealthStatus, Project, ProjectStatus, Task, TaskStatus,
};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render,
    screens::{
        DashboardScreen, DiagnosticsScreen, FleetInspectorScreen, PlanReviewScreen,
        ProjectInputScreen,
    },
    widgets::{AgentInspectorWidget, DagViewWidget, DetailsPaneWidget},
    AppState, CurrentScreen, InputMode, ReviewTaskState, TuiUpdateEvent,
};

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            s.push_str(buffer[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_clean_initial_app_state_has_zero_mock_data() {
    let state = AppState::new();

    // Verify all collections and models are strictly empty
    assert!(state.agents.is_empty(), "Initial agents must be empty");
    assert!(
        state.available_projects.is_empty(),
        "Initial available_projects must be empty"
    );
    assert!(
        state.active_project.is_none(),
        "Initial active_project must be None"
    );
    assert!(
        state.review_tasks.is_empty(),
        "Initial review_tasks must be empty"
    );
    assert!(
        state.active_overlaps.is_empty(),
        "Initial active_overlaps must be empty"
    );
    assert!(
        state.recent_events.is_empty(),
        "Initial recent_events must be empty"
    );
    assert!(
        state.event_ring_buffer.is_empty(),
        "Initial event_ring_buffer must be empty"
    );
    assert!(state.metrics.is_none(), "Initial metrics must be None");
    assert!(
        state.selected_timeline.is_none(),
        "Initial selected_timeline must be None"
    );

    // Initial navigation defaults
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(!state.should_quit);

    // Modals closed
    assert!(!state.show_command_palette);
    assert!(!state.show_agent_inspector);
    assert!(!state.show_help_modal);
    assert!(!state.show_event_payload_modal);
    assert!(!state.show_timeline_modal);
    assert!(state.pending_confirmation.is_none());
}

#[test]
fn test_render_empty_dashboard_screen() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| DashboardScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No project currently active."),
        "Expected project empty message in Dashboard"
    );
    assert!(
        output.contains("No agents"),
        "Expected agents empty message in Dashboard"
    );
    assert!(
        output.contains("No tasks currently in project."),
        "Expected tasks empty message in Dashboard"
    );
    assert!(
        output.contains("Zero resource conflicts detected across active tasks."),
        "Expected zero overlaps message in Dashboard"
    );
    assert!(
        output.contains("Total Tasks: 0"),
        "Expected 0 total tasks in Dashboard"
    );
    assert!(
        output.contains("Pending Review: 0"),
        "Expected 0 pending tasks in Dashboard"
    );
}

#[test]
fn test_render_empty_project_input_screen() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| ProjectInputScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No projects found."),
        "Expected 'No projects found.' in ProjectInputScreen"
    );
    assert!(
        output.contains("Press [N] or [i] to define a new project."),
        "Expected new project prompt in ProjectInputScreen"
    );
}

#[test]
fn test_render_empty_plan_review_screen() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| PlanReviewScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No tasks proposed yet. Go to [2: Proj]"),
        "Expected empty prompt pointing to [2: Proj] in PlanReviewScreen"
    );
}

#[test]
fn test_render_empty_dag_view_widget() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| DagViewWidget::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No tasks proposed yet. Go to [2: Proj] to submit a project."),
        "Expected empty prompt pointing to [2: Proj] in DagViewWidget"
    );
}

#[test]
fn test_render_empty_details_pane_widget() {
    let mut terminal = create_terminal(60, 25);
    let state = AppState::new();

    terminal
        .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No tasks available to inspect."),
        "Expected empty inspection message in DetailsPaneWidget"
    );
}

#[test]
fn test_render_empty_fleet_inspector_screen() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| FleetInspectorScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No agents"),
        "Expected empty fleet message in FleetInspectorScreen"
    );
    assert!(
        output.contains("No agent selected for capability and telemetry inspection."),
        "Expected empty inspector message in FleetInspectorScreen"
    );
}

#[test]
fn test_render_empty_agent_inspector_modal() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new();
    state.show_agent_inspector = true;

    terminal
        .draw(|f| AgentInspectorWidget::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No agent selected for capability and telemetry inspection."),
        "Expected empty inspector modal message in AgentInspectorWidget"
    );
}

#[test]
fn test_render_empty_diagnostics_screen() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();

    terminal
        .draw(|f| DiagnosticsScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("Observability event pipeline initialized"),
        "Expected initialized event log message in DiagnosticsScreen"
    );
    assert!(
        output.contains("No agents"),
        "Expected empty fleet health in DiagnosticsScreen"
    );
    assert!(
        output.contains("No timeline loaded"),
        "Expected empty timeline message in DiagnosticsScreen"
    );
    assert!(
        output.contains("Total: 0"),
        "Expected 0 total tasks in metrics KPI"
    );
}

#[test]
fn test_render_empty_state_across_all_screens_top_level() {
    let mut terminal = create_terminal(160, 40);
    let mut state = AppState::new();
    state.pg_connected = true;
    state.nats_connected = true;

    let screens = [
        CurrentScreen::Dashboard,
        CurrentScreen::ProjectManagement,
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::FleetInspector,
        CurrentScreen::Diagnostics,
    ];

    for screen in screens {
        state.current_screen = screen;
        terminal
            .draw(|f| render(f, &state))
            .unwrap_or_else(|_| panic!("Failed to render top-level UI for screen {:?}", screen));

        let output = buffer_to_string(&terminal);
        // Verify header in live empty state
        assert!(
            output.contains("[PG: ● OK]"),
            "Header must indicate PG connected"
        );
        assert!(
            output.contains("[NATS: ● OK]"),
            "Header must indicate NATS connected"
        );
        assert!(
            output.contains("[Agents: 0/0]"),
            "Header must display 0/0 agents on empty state"
        );
        assert!(
            output.contains("[Pending: 0]"),
            "Header must display 0 pending tasks on empty state"
        );
        assert!(
            output.contains("No Project Selected"),
            "Header must indicate no project selected on empty state"
        );
    }
}

#[test]
fn test_key_navigation_on_empty_state_does_not_panic() {
    let mut state = AppState::new();

    let test_keys = [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Char('c'),
        KeyCode::Char('r'),
        KeyCode::Char('y'),
        KeyCode::Char('n'),
        KeyCode::Char('a'),
        KeyCode::Char('d'),
        KeyCode::Char('p'),
        KeyCode::Char('['),
        KeyCode::Char(']'),
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Char('1'),
        KeyCode::Char('2'),
        KeyCode::Char('3'),
        KeyCode::Char('4'),
        KeyCode::Char('5'),
    ];

    for key_code in test_keys {
        let _ = state.handle_key(make_key(key_code));
        assert!(
            !state.should_quit,
            "State should not quit on standard navigation key {:?}",
            key_code
        );
    }
}

#[test]
fn test_live_events_populate_clean_state_without_mock_residue() {
    let mut state = AppState::new();
    assert!(state.agents.is_empty());
    assert!(state.available_projects.is_empty());
    assert!(state.review_tasks.is_empty());

    // 1. Live project update
    let project = Project {
        id: Uuid::new_v4(),
        name: "Real Production API".to_string(),
        description: "Live coordinator production mesh".to_string(),
        status: ProjectStatus::Active,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::Projects(vec![project.clone()]));
    state.apply_update(TuiUpdateEvent::ActiveProject(project.clone()));

    assert_eq!(state.available_projects.len(), 1);
    assert_eq!(state.active_project.as_ref().unwrap().name, "Real Production API");

    // 2. Live agent update
    let agent_id = Uuid::new_v4();
    let agent = Agent {
        id: agent_id,
        human_owner: "Real Operator (Live)".to_string(),
        api_key_hash: "hash".to_string(),
        adapter_type: AdapterType::Agy,
        capabilities: serde_json::json!(["rust", "backend"]),
        nats_subject: format!("agents.{agent_id}.events"),
        status: AgentStatus::Idle,
        current_task_id: None,
        last_seen: Some(chrono::Utc::now()),
        created_at: chrono::Utc::now(),
        capability_profile: None,
        health_status: HealthStatus::Healthy,
        consecutive_failures: 0,
        tasks_completed_count: 0,
        tasks_failed_count: 0,
        last_error: None,
        heartbeat_latency_ms: Some(15),
        max_concurrency: 1,
        active_tasks_count: 0,
        is_draining: false,
        role: "worker".to_string(),
        permissions: serde_json::json!({}),
        is_revoked: false,
        api_key_created_at: chrono::Utc::now(),
        api_key_expires_at: None,
    };
    state.apply_update(TuiUpdateEvent::Agents(vec![agent]));
    assert_eq!(state.agents.len(), 1);
    assert_eq!(state.active_agent_count(), (1, 1));

    // 3. Live task update
    let task = Task {
        id: Uuid::new_v4(),
        project_id: project.id,
        short_id: "REAL-1".to_string(),
        title: "Implement Live Production Endpoint".to_string(),
        description: "Purely live data endpoint without mock residue".to_string(),
        affected_resources: serde_json::json!(["src/api.rs"]),
        status: TaskStatus::HumanReview,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::Tasks(vec![ReviewTaskState::new(task)]));
    assert_eq!(state.review_tasks.len(), 1);
    assert_eq!(state.pending_approvals_count(), 1);

    // 4. Live coordinator event
    let event = CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        event_type: "coordinator.live_mesh_ready".to_string(),
        project_id: Some(project.id),
        task_id: None,
        agent_id: None,
        message: "Mesh initialized purely on live database & NATS events".to_string(),
        payload: serde_json::json!({ "mode": "live" }),
    };
    state.apply_update(TuiUpdateEvent::ObservabilityEvent(event));
    assert_eq!(state.recent_events.len(), 1);

    // 5. Clean reset / empty update
    state.apply_update(TuiUpdateEvent::Projects(vec![]));
    state.apply_update(TuiUpdateEvent::Agents(vec![]));
    state.apply_update(TuiUpdateEvent::Tasks(vec![]));

    assert!(state.available_projects.is_empty());
    assert!(state.agents.is_empty());
    assert!(state.review_tasks.is_empty());
    assert_eq!(state.active_agent_count(), (0, 0));
    assert_eq!(state.pending_approvals_count(), 0);
}

#[test]
fn test_terminal_resize_on_empty_state_does_not_panic() {
    let state = AppState::new();

    let resolutions = [
        (40, 10),
        (60, 18),
        (70, 20),
        (80, 24),
        (100, 30),
        (120, 40),
        (160, 50),
    ];

    for (w, h) in resolutions {
        let mut terminal = create_terminal(w, h);
        terminal
            .draw(|f| render(f, &state))
            .unwrap_or_else(|_| panic!("Render failed at resolution {}x{}", w, h));
    }
}

#[test]
fn test_agent_inspector_telemetry_renders_na_without_mock_runtime_string() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new();
    let agent_id = Uuid::new_v4();

    let agent = Agent {
        id: agent_id,
        human_owner: "Custom Runner (Ops)".to_string(),
        api_key_hash: "hash".to_string(),
        adapter_type: AdapterType::Agy,
        capabilities: serde_json::json!(["bash", "curl"]),
        nats_subject: format!("agents.{agent_id}.events"),
        status: AgentStatus::Idle,
        current_task_id: None,
        last_seen: Some(chrono::Utc::now()),
        created_at: chrono::Utc::now(),
        capability_profile: Some(serde_json::json!({
            "runtime": {
                "os": "linux",
                "arch": "x86_64",
                "adapter_type": "Agy",
                "agy_version": null,
                "cpu_count": 8,
                "memory_mb": 16384
            },
            "commands": ["git", "bash"],
            "mcp_servers": [],
            "specialized_capabilities": ["ci"]
        })),
        health_status: HealthStatus::Healthy,
        consecutive_failures: 0,
        tasks_completed_count: 0,
        tasks_failed_count: 0,
        last_error: None,
        heartbeat_latency_ms: Some(12),
        max_concurrency: 1,
        active_tasks_count: 0,
        is_draining: false,
        role: "worker".to_string(),
        permissions: serde_json::json!({}),
        is_revoked: false,
        api_key_created_at: chrono::Utc::now(),
        api_key_expires_at: None,
    };

    state.agents.push(agent);
    state.selected_agent_index = 0;
    state.show_agent_inspector = true;

    terminal
        .draw(|f| AgentInspectorWidget::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        !output.contains("Mock runtime"),
        "Inspector must not contain hardcoded 'Mock runtime' string when agy_version is null"
    );
    assert!(
        output.contains("Antigravity CLI: N/A"),
        "Inspector should display 'Antigravity CLI: N/A' for absent agy_version"
    );
}

#[test]
fn test_project_submission_resets_form_and_cleans_inputs() {
    let mut state = AppState::new();
    state.input_mode = InputMode::EnteringProject;
    state.project_name_input = "Production Pipeline".to_string();
    state.project_desc_input = "Deploy live distributed mesh".to_string();

    let action = state.handle_key(make_key(KeyCode::Enter));

    match action {
        Some(coordinator::tui::TuiAction::SubmitProject { name, description }) => {
            assert_eq!(name, "Production Pipeline");
            assert_eq!(description, "Deploy live distributed mesh");
        }
        other => panic!("Expected SubmitProject action, got {:?}", other),
    }

    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(
        state.project_name_input.is_empty(),
        "project_name_input must be cleared after submission"
    );
    assert!(
        state.project_desc_input.is_empty(),
        "project_desc_input must be cleared after submission"
    );
    assert_eq!(state.project_focus_field, 0);
    assert_eq!(state.project_input_cursor, 0);
}

#[test]
fn test_diagnostics_screen_timeline_drilldown_guides_to_proj_screen_when_empty() {
    let mut terminal = create_terminal(120, 35);
    let state = AppState::new();
    assert!(state.review_tasks.is_empty());

    terminal
        .draw(|f| DiagnosticsScreen::render(f, f.area(), &state))
        .unwrap();

    let output = buffer_to_string(&terminal);
    assert!(
        output.contains("No timeline loaded. Go to [2: Proj] to submit a project."),
        "Diagnostics timeline must guide operator to [2: Proj] when review_tasks is empty"
    );
}
