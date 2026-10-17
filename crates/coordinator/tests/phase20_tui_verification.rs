use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use agent_protocol::AgentMessage;
use coordinator::domain::{
    AdapterType, Agent, AgentStatus, ApprovalStatus, HealthStatus,
    Task, TaskStatus,
};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render, AppState, CurrentScreen, InputMode, ReviewTaskState, TuiAction, TuiUpdateEvent,
};

#[allow(dead_code)]
#[path = "../src/cli.rs"]
mod cli;
use cli::CliArgs;

// ============================================================================
// Deterministic Buffer Inspection & Synthetic Key Press Helpers
// ============================================================================

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

fn assert_buffer_contains(terminal: &Terminal<TestBackend>, expected: &str) {
    let content = buffer_to_string(terminal);
    assert!(
        content.contains(expected),
        "Buffer did not contain '{expected}'. Rendered buffer:\n{content}"
    );
}

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn sample_task(id: Uuid, project_id: Uuid, short_id: &str, title: &str, status: TaskStatus) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["crates/coordinator", "src/lib.rs"]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ============================================================================
// TIER 1: FEATURE COVERAGE (>=5 tests per feature)
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 1.1: Screen Navigation (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_screen_nav_initial_state() {
    let state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
    assert_eq!(state.current_screen.title(), "1. Dashboard");
    assert_eq!(state.input_mode, InputMode::Normal);
    assert!(!state.should_quit);
}

#[test]
fn test_tier1_screen_nav_tab_cycling() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    // Tab 1: Dashboard -> ProjectManagement
    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    // Tab 2: ProjectManagement -> PlanReview
    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    // Tab 3: PlanReview -> FleetInspector
    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    // Tab 4: FleetInspector -> Diagnostics
    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    // Tab 5: Diagnostics -> Dashboard
    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
}

#[test]
fn test_tier1_screen_nav_numeric_hotkeys() {
    let mut state = AppState::new();

    state.handle_key(press_key(KeyCode::Char('2')));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    state.handle_key(press_key(KeyCode::Char('3')));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    state.handle_key(press_key(KeyCode::Char('4')));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    state.handle_key(press_key(KeyCode::Char('5')));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    state.handle_key(press_key(KeyCode::Char('1')));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
}

#[test]
fn test_tier1_screen_nav_headers_render_correct_titles() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();

    // 1. Project Input Screen Title
    state.current_screen = CurrentScreen::ProjectInput;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Project Definition & Planning Input");

    // 2. Plan Review Screen Title
    state.current_screen = CurrentScreen::PlanReview;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Proposed Task Decomposition (Human Review)");

    // 3. Dashboard Screen Title
    state.current_screen = CurrentScreen::Dashboard;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "System Overview & Live Dashboard");

    // 4. Diagnostics Screen Title
    state.current_screen = CurrentScreen::Diagnostics;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "System Observability & Diagnostics");
}

#[test]
fn test_tier1_screen_nav_preserves_state_across_switches() {
    let mut state = AppState::new();
    state.project_name_input = "State Preservation Project".to_string();
    state.selected_task_index = 2;

    // Switch to Dashboard and back
    state.handle_key(press_key(KeyCode::Char('1')));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
    assert_eq!(state.project_name_input, "State Preservation Project");
    assert_eq!(state.selected_task_index, 2);

    state.handle_key(press_key(KeyCode::Char('2')));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);
    assert_eq!(state.project_name_input, "State Preservation Project");
}

// ----------------------------------------------------------------------------
// Tier 1.2: Plan Review Operator Workflow (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_plan_review_approve_task_y() {
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;

    let action = state.handle_key(press_key(KeyCode::Char('y')));
    assert_eq!(action, Some(TuiAction::ApproveTask { task_id: task0_id }));
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::Approved)
    );
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(state.selected_task_index, 1);
}

#[test]
fn test_tier1_plan_review_reject_task_n() {
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;

    let action = state.handle_key(press_key(KeyCode::Char('n')));
    assert_eq!(action, Some(TuiAction::RejectTask { task_id: task0_id }));
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::Rejected)
    );
    assert_eq!(state.selected_task_index, 1);
}

#[test]
fn test_tier1_plan_review_inline_edit_description() {
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;
    let orig_desc = state.review_tasks[0].task.description.clone();

    // Press 'e' to enter editing
    let action_edit = state.handle_key(press_key(KeyCode::Char('e')));
    assert_eq!(action_edit, None);
    assert_eq!(state.input_mode, InputMode::EditingDescription);
    assert_eq!(state.edit_buffer, orig_desc);

    // Type addition
    for c in " [reviewed by op]".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }

    // Press Enter to confirm and approve
    let action_save = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(state.input_mode, InputMode::Normal);
    match action_save {
        Some(TuiAction::EditTaskDescription {
            task_id,
            new_description,
        }) => {
            assert_eq!(task_id, task0_id);
            assert!(new_description.ends_with("[reviewed by op]"));
            assert_eq!(state.review_tasks[0].task.description, new_description);
        }
        other => panic!("Expected EditTaskDescription, got {other:?}"),
    }
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::EditedAndApproved)
    );
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
}

#[test]
fn test_tier1_plan_review_details_toggle_enter() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    assert!(state.show_details_pane);

    // Details Pane visible
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Dependencies (Prerequisites)");
    assert_buffer_contains(&terminal, "Task Details & Inspector");

    // Toggle off
    state.handle_key(press_key(KeyCode::Enter));
    assert!(!state.show_details_pane);

    // Toggle on again
    state.handle_key(press_key(KeyCode::Enter));
    assert!(state.show_details_pane);
}

#[test]
fn test_tier1_plan_review_acknowledge_overlap_a() {
    let mut state = AppState::new().with_mock_data();
    assert_eq!(state.selected_task_index, 0);
    assert!(!state.review_tasks[0].overlap_warnings.is_empty());
    let warning_id = state.review_tasks[0].overlap_warnings[0].id;

    let action = state.handle_key(press_key(KeyCode::Char('a')));
    assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id }));
    assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);
    assert!(state.active_overlaps.iter().all(|w| w.id != warning_id));
}

#[test]
fn test_tier1_plan_review_cancel_task_c() {
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;

    let action = state.handle_key(press_key(KeyCode::Char('c')));
    assert_eq!(action, Some(TuiAction::CancelTask { task_id: task0_id }));
    assert!(state.status_message.as_ref().unwrap().contains("Cancelling task"));
}

// ----------------------------------------------------------------------------
// Tier 1.3: Dashboard Rendering & Metrics KPI Buffer Inspection (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_dashboard_renders_active_project_summary() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Active Project:");
    assert_buffer_contains(&terminal, "Cloud Migration Mesh");
    assert_buffer_contains(&terminal, "Scope:");
}

#[test]
fn test_tier1_dashboard_renders_fleet_agents_table() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Registered Agents (Mesh Fleet)");
    assert_buffer_contains(&terminal, "agent-backend");
    assert_buffer_contains(&terminal, "Alice");
    assert_buffer_contains(&terminal, "agent-infra");
    assert_buffer_contains(&terminal, "Bob");
}

#[test]
fn test_tier1_dashboard_renders_agent_status_badges() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "IDLE");
    assert_buffer_contains(&terminal, "BUSY");
}

#[test]
fn test_tier1_dashboard_renders_tasks_overview_table() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "All Tasks Overview");
    assert_buffer_contains(&terminal, "PLAN-1");
    assert_buffer_contains(&terminal, "PLAN-2");
    assert_buffer_contains(&terminal, "PLAN-3");
}

#[test]
fn test_tier1_dashboard_renders_task_metrics_kpi() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Task Status Metrics");
    assert_buffer_contains(&terminal, "Total Tasks:");
    assert_buffer_contains(&terminal, "Pending Review:");
    assert_buffer_contains(&terminal, "Approved / Queued:");
}

#[test]
fn test_tier1_dashboard_renders_overlap_warnings_list() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Active Overlap Warnings");
    assert_buffer_contains(&terminal, "crates/db");
    assert_buffer_contains(&terminal, "Critical");
}

// ----------------------------------------------------------------------------
// Tier 1.4: Diagnostics KPI Bar & Event Stream Buffer Inspection (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_diagnostics_renders_operational_kpi_bar() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Operational KPI Metrics");
    assert_buffer_contains(&terminal, "[Tasks]");
    assert_buffer_contains(&terminal, "[Deliveries]");
    assert_buffer_contains(&terminal, "Success Rate: 100.0%");
    assert_buffer_contains(&terminal, "[Fleet]");
}

#[test]
fn test_tier1_diagnostics_renders_fleet_health_table() {
    let mut terminal = create_terminal(160, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Fleet Health & Diagnostics");
    assert_buffer_contains(&terminal, "agent-backend (Alice)");
    assert_buffer_contains(&terminal, "agent-infra (Bob)");
    assert_buffer_contains(&terminal, "HEALTHY");
}

#[test]
fn test_tier1_diagnostics_renders_recent_events_stream() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Coordinator Events Stream");
    assert_buffer_contains(&terminal, "coordinator.started");
    assert_buffer_contains(&terminal, "Coordinator engine started successfully.");
    assert_buffer_contains(&terminal, "task.overlap_detected");
}

#[test]
fn test_tier1_diagnostics_renders_task_timeline_drilldown() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Task Timeline Drill-Down");
}

#[test]
fn test_tier1_diagnostics_event_stream_navigation() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;
    assert_eq!(state.selected_event_index, 0);

    // Down navigation
    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_event_index, 1);

    // Clamped at end
    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_event_index, 1);

    // Up navigation
    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.selected_event_index, 0);
}

// ----------------------------------------------------------------------------
// Tier 1.5: Project Input Submission & Validation (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_project_input_rendering_initial() {
    let mut terminal = create_terminal(100, 25);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectInput;

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Project Definition & Planning Input");
    assert_buffer_contains(&terminal, "Enter project details below.");
    assert_buffer_contains(&terminal, "Project Name");
    assert_buffer_contains(&terminal, "Project Description");
}

#[test]
fn test_tier1_project_input_enter_edit_mode() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectInput;
    assert_eq!(state.input_mode, InputMode::Normal);

    state.handle_key(press_key(KeyCode::Char('i')));
    assert_eq!(state.input_mode, InputMode::EnteringProject);
}

#[test]
fn test_tier1_project_input_tab_field_focus() {
    let mut state = AppState::new();
    state.input_mode = InputMode::EnteringProject;
    assert_eq!(state.project_focus_field, 0); // Name field

    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.project_focus_field, 1); // Description field

    state.handle_key(press_key(KeyCode::BackTab));
    assert_eq!(state.project_focus_field, 0); // Back to Name field
}

#[test]
fn test_tier1_project_input_text_typing_and_backspace() {
    let mut state = AppState::new();
    state.input_mode = InputMode::EnteringProject;

    // Type into Name field
    for c in "AgentMesh-V2".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    assert_eq!(state.project_name_input, "AgentMesh-V2");

    // Backspace
    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.project_name_input, "AgentMesh-V");

    // Switch to Description and type
    state.handle_key(press_key(KeyCode::Tab));
    for c in "Autonomous coordinator".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    assert_eq!(state.project_desc_input, "Autonomous coordinator");
}

#[test]
fn test_tier1_project_input_validation_empty_name() {
    let mut state = AppState::new();
    state.input_mode = InputMode::EnteringProject;
    state.project_name_input = "".to_string();
    state.project_desc_input = "Description without name".to_string();

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, None);
    assert_eq!(state.input_mode, InputMode::EnteringProject);
    assert!(state.status_message.as_ref().unwrap().contains("Project name cannot be empty"));
}

#[test]
fn test_tier1_project_input_successful_submission() {
    let mut state = AppState::new();
    state.input_mode = InputMode::EnteringProject;
    state.project_name_input = "Cloud Modernization".to_string();
    state.project_desc_input = "Microservices transition plan".to_string();

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(
        action,
        Some(TuiAction::SubmitProject {
            name: "Cloud Modernization".to_string(),
            description: "Microservices transition plan".to_string(),
        })
    );
    assert!(state.status_message.as_ref().unwrap().contains("Submitted project 'Cloud Modernization'"));
}

// ----------------------------------------------------------------------------
// Tier 1.6: CLI Argument Parsing Contract (7 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_cli_headless_flag() {
    let args = vec!["coordinator", "--headless"];
    let parsed = CliArgs::from_args(args).unwrap();
    assert!(parsed.headless);
    assert!(!parsed.tui);

    let short_args = vec!["coordinator", "-d"];
    let parsed_short = CliArgs::from_args(short_args).unwrap();
    assert!(parsed_short.headless);
}

#[test]
fn test_tier1_cli_tui_flag() {
    let args = vec!["coordinator", "--tui"];
    let parsed = CliArgs::from_args(args).unwrap();
    assert!(parsed.tui);
    assert!(!parsed.headless);
}

#[test]
fn test_tier1_cli_project_flag() {
    let args1 = vec!["coordinator", "--project", "proj-uuid-1234"];
    let parsed1 = CliArgs::from_args(args1).unwrap();
    assert_eq!(parsed1.project_id, Some("proj-uuid-1234".to_string()));

    let args2 = vec!["coordinator", "--project=proj-uuid-5678"];
    let parsed2 = CliArgs::from_args(args2).unwrap();
    assert_eq!(parsed2.project_id, Some("proj-uuid-5678".to_string()));

    let err_args = vec!["coordinator", "--project"];
    assert!(CliArgs::from_args(err_args).is_err());
}

#[test]
fn test_tier1_cli_auto_approve_flag() {
    let args = vec!["coordinator", "--auto-approve"];
    let parsed = CliArgs::from_args(args).unwrap();
    assert!(parsed.auto_approve);
}

#[test]
fn test_tier1_cli_status_flag() {
    let args = vec!["coordinator", "--status"];
    let parsed = CliArgs::from_args(args).unwrap();
    assert!(parsed.status);
}

#[test]
fn test_tier1_cli_help_and_version_flags() {
    let help_args = vec!["coordinator", "--help"];
    let parsed_help = CliArgs::from_args(help_args).unwrap();
    assert!(parsed_help.show_help);

    let short_help = vec!["coordinator", "-h"];
    let parsed_short_help = CliArgs::from_args(short_help).unwrap();
    assert!(parsed_short_help.show_help);

    let ver_args = vec!["coordinator", "--version"];
    let parsed_ver = CliArgs::from_args(ver_args).unwrap();
    assert!(parsed_ver.show_version);

    let short_ver = vec!["coordinator", "-V"];
    let parsed_short_ver = CliArgs::from_args(short_ver).unwrap();
    assert!(parsed_short_ver.show_version);
}

#[test]
fn test_tier1_cli_is_headless_evaluation() {
    // 1. Explicit --headless always evaluates to headless true regardless of TTY
    let args_headless = CliArgs {
        headless: true,
        tui: false,
        project_id: None,
        auto_approve: false,
        status: false,
        show_help: false,
        show_version: false,
    };
    assert!(args_headless.is_headless(true));
    assert!(args_headless.is_headless(false));

    // 2. Explicit --tui evaluates to headless false even when stdout is non-TTY
    let args_tui = CliArgs {
        headless: false,
        tui: true,
        project_id: None,
        auto_approve: false,
        status: false,
        show_help: false,
        show_version: false,
    };
    assert!(!args_tui.is_headless(true));
    assert!(!args_tui.is_headless(false));

    // 3. Default fallback: interactive TTY -> interactive (not headless), non-TTY -> headless
    let args_default = CliArgs {
        headless: false,
        tui: false,
        project_id: None,
        auto_approve: false,
        status: false,
        show_help: false,
        show_version: false,
    };
    assert!(!args_default.is_headless(true));
    assert!(args_default.is_headless(false));

    // 4. Mutual exclusion check
    let invalid_both = vec!["coordinator", "--headless", "--tui"];
    assert!(CliArgs::from_args(invalid_both).is_err());
}

// ============================================================================
// TIER 2: BOUNDARY & CORNER CASES (>=5 tests per feature)
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 2.1: Long Task Lists (15+ tasks) & Scrolling Window (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier2_scrolling_window_keeps_selected_card_visible() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=20 {
        let task = sample_task(
            Uuid::new_v4(),
            proj_id,
            &format!("TASK-{i:02}"),
            &format!("Stress task number {i}"),
            TaskStatus::HumanReview,
        );
        state.review_tasks.push(ReviewTaskState::new(task));
    }
    assert_eq!(state.review_tasks.len(), 20);

    // Select last task (index 19)
    state.selected_task_index = 19;
    terminal.draw(|f| render(f, &state)).unwrap();

    // Verify TASK-20 is visible in rendered buffer
    assert_buffer_contains(&terminal, "TASK-20");
}

#[test]
fn test_tier2_scrolling_window_navigate_top_to_bottom() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=18 {
        let task = sample_task(
            Uuid::new_v4(),
            proj_id,
            &format!("TASK-{i:02}"),
            &format!("Task item {i}"),
            TaskStatus::HumanReview,
        );
        state.review_tasks.push(ReviewTaskState::new(task));
    }

    for expected_idx in 1..18 {
        state.handle_key(press_key(KeyCode::Down));
        assert_eq!(state.selected_task_index, expected_idx);
        terminal.draw(|f| render(f, &state)).unwrap();
    }
}

#[test]
fn test_tier2_scrolling_window_clamp_at_bottom() {
    let mut state = AppState::new().with_mock_data();
    let max_idx = state.review_tasks.len() - 1;
    state.selected_task_index = max_idx;

    // Further Down keypresses must clamp
    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_task_index, max_idx);
    state.handle_key(press_key(KeyCode::Char('j')));
    assert_eq!(state.selected_task_index, max_idx);
}

#[test]
fn test_tier2_scrolling_window_clamp_at_top() {
    let mut state = AppState::new().with_mock_data();
    state.selected_task_index = 0;

    // Up keypresses at 0 must clamp
    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.selected_task_index, 0);
    state.handle_key(press_key(KeyCode::Char('k')));
    assert_eq!(state.selected_task_index, 0);
}

#[test]
fn test_tier2_scrolling_window_empty_tasks_list() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.review_tasks.clear();

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "No tasks proposed yet");
}

#[test]
fn test_tier2_scrolling_window_single_task() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let task = sample_task(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "SOLO-01",
        "Single task in plan",
        TaskStatus::HumanReview,
    );
    state.review_tasks.push(ReviewTaskState::new(task));

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "SOLO-01");

    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_task_index, 0);
}

// ----------------------------------------------------------------------------
// Tier 2.2: Terminal Resize & Responsiveness (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier2_terminal_resize_standard_80x24() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new().with_mock_data();

    for screen in [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ] {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).expect("Render 80x24 standard");
    }
}

#[test]
fn test_tier2_terminal_resize_compact_100x30() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new().with_mock_data();

    for screen in [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ] {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).expect("Render 100x30 compact");
    }
}

#[test]
fn test_tier2_terminal_resize_wide_160x50() {
    let mut terminal = create_terminal(160, 50);
    let mut state = AppState::new().with_mock_data();

    for screen in [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ] {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).expect("Render 160x50 wide");
    }
}

#[test]
fn test_tier2_terminal_resize_minimal_70x20() {
    let mut terminal = create_terminal(70, 20);
    let mut state = AppState::new().with_mock_data();

    for screen in [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ] {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).expect("Render 70x20 minimal");
    }
}

#[test]
fn test_tier2_terminal_resize_narrow_60x18() {
    let mut terminal = create_terminal(60, 18);
    let mut state = AppState::new().with_mock_data();

    for screen in [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ] {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).expect("Render 60x18 boundary");
    }
}

// ----------------------------------------------------------------------------
// Tier 2.3: Description Editor Buffer Bounds (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier2_editor_empty_buffer_navigation() {
    let mut state = AppState::new().with_mock_data();
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = String::new();
    state.edit_cursor = 0;

    // Left and Right arrows in empty buffer do not panic
    state.handle_key(press_key(KeyCode::Left));
    assert_eq!(state.edit_cursor, 0);

    state.handle_key(press_key(KeyCode::Right));
    assert_eq!(state.edit_cursor, 0);
}

#[test]
fn test_tier2_editor_backspace_at_start() {
    let mut state = AppState::new().with_mock_data();
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = "Initial buffer".to_string();
    state.edit_cursor = 0;

    // Backspace at 0 does nothing and does not panic
    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.edit_cursor, 0);
    assert_eq!(state.edit_buffer, "Initial buffer");
}

#[test]
fn test_tier2_editor_cursor_bounds_clamping() {
    let mut state = AppState::new().with_mock_data();
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = "ABC".to_string();
    state.edit_cursor = 3;

    // Cannot move beyond buffer length
    state.handle_key(press_key(KeyCode::Right));
    assert_eq!(state.edit_cursor, 3);

    // Navigate to beginning
    state.handle_key(press_key(KeyCode::Left));
    state.handle_key(press_key(KeyCode::Left));
    state.handle_key(press_key(KeyCode::Left));
    assert_eq!(state.edit_cursor, 0);

    // Cannot move left of 0
    state.handle_key(press_key(KeyCode::Left));
    assert_eq!(state.edit_cursor, 0);
}

#[test]
fn test_tier2_editor_esc_cancellation_restores_original() {
    let mut state = AppState::new().with_mock_data();
    let original_desc = state.review_tasks[0].task.description.clone();

    state.handle_key(press_key(KeyCode::Char('e')));
    assert_eq!(state.input_mode, InputMode::EditingDescription);

    // Corrupt buffer
    for _ in 0..5 {
        state.handle_key(press_key(KeyCode::Backspace));
    }
    for c in "DIRTY_UNCONFIRMED_TEXT".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }

    // Esc cancels and restores original
    let action = state.handle_key(press_key(KeyCode::Esc));
    assert_eq!(action, None);
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(state.review_tasks[0].task.description, original_desc);
}

#[test]
fn test_tier2_editor_symbols_and_punctuation_integrity() {
    let mut state = AppState::new().with_mock_data();
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = String::new();
    state.edit_cursor = 0;

    for c in "[AgentMesh] -> (verified: 100%)!".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    assert_eq!(state.edit_buffer, "[AgentMesh] -> (verified: 100%)!");
    assert_eq!(state.edit_cursor, state.edit_buffer.len());
}

// ----------------------------------------------------------------------------
// Tier 2.4: Empty Event Stream vs 100+ Events (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier2_event_stream_empty() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Diagnostics;
    state.recent_events.clear();

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Observability event pipeline initialized");
}

#[test]
fn test_tier2_event_stream_truncation_to_max_capacity() {
    let mut state = AppState::new();
    for i in 1..=250 {
        let event = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "benchmark.event".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Event payload #{i}"),
            payload: json!({ "seq": i }),
        };
        state.apply_update(TuiUpdateEvent::ObservabilityEvent(event));
    }

    // Recent events buffer is capped at 200 entries max
    assert_eq!(state.recent_events.len(), 200);
}

#[test]
fn test_tier2_event_stream_scrolling_bounds() {
    let mut state = AppState::new();
    for i in 1..=30 {
        state.recent_events.push(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "event.test".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Message {i}"),
            payload: json!({}),
        });
    }

    state.current_screen = CurrentScreen::Diagnostics;
    state.selected_event_index = 29;

    // Clamps at max
    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_event_index, 29);

    // Clamps at 0
    state.selected_event_index = 0;
    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.selected_event_index, 0);
}

#[test]
fn test_tier2_event_stream_single_event() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;
    state.recent_events = vec![CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "system.isolated".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Single isolated event".to_string(),
        payload: json!({}),
    }];

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "system.isolated");
    assert_buffer_contains(&terminal, "Single isolated event");
}

#[test]
fn test_tier2_event_stream_rapid_burst() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;

    for i in 0..250 {
        state.apply_update(TuiUpdateEvent::ObservabilityEvent(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: format!("burst.event.{i}"),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Rapid burst notification {i}"),
            payload: json!({ "index": i }),
        }));
    }

    assert_eq!(state.recent_events.len(), 200);
    terminal.draw(|f| render(f, &state)).expect("Render after rapid burst");
}

// ----------------------------------------------------------------------------
// Tier 2.5: Agent Fleet Status Diversity (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier2_fleet_rendering_no_agents() {
    let mut terminal = create_terminal(140, 30);
    let mut state = AppState::new();
    state.agents.clear();

    state.current_screen = CurrentScreen::Dashboard;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "No agents currently register");

    state.current_screen = CurrentScreen::Diagnostics;
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "No agents connect");
}

#[test]
fn test_tier2_fleet_rendering_all_statuses() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Dashboard;

    state.agents = vec![
        Agent::mock(Uuid::new_v4(), "agent-idle", AdapterType::Mock, vec![], AgentStatus::Idle),
        Agent::mock(Uuid::new_v4(), "agent-busy", AdapterType::Agy, vec![], AgentStatus::Busy),
        Agent::mock(Uuid::new_v4(), "agent-blocked", AdapterType::Mock, vec![], AgentStatus::Blocked),
        Agent::mock(Uuid::new_v4(), "agent-error", AdapterType::Agy, vec![], AgentStatus::Error),
        Agent::mock(Uuid::new_v4(), "agent-offline", AdapterType::Mock, vec![], AgentStatus::Offline),
    ];

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "IDLE");
    assert_buffer_contains(&terminal, "BUSY");
    assert_buffer_contains(&terminal, "BLOCKED");
    assert_buffer_contains(&terminal, "ERROR");
    assert_buffer_contains(&terminal, "OFFLINE");
}

#[test]
fn test_tier2_fleet_rendering_unhealthy_agents() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;

    let mut agent_unhealthy = Agent::mock(
        Uuid::new_v4(),
        "agent-failing",
        AdapterType::Agy,
        vec![],
        AgentStatus::Error,
    );
    agent_unhealthy.health_status = HealthStatus::Unhealthy;
    agent_unhealthy.consecutive_failures = 6;
    agent_unhealthy.tasks_failed_count = 12;
    agent_unhealthy.last_error = Some("Out of memory on container".to_string());

    state.agents = vec![agent_unhealthy];
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "UNHEALTHY");
}

#[test]
fn test_tier2_fleet_rendering_large_fleet() {
    let mut terminal = create_terminal(140, 40);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Dashboard;

    for i in 1..=30 {
        state.agents.push(Agent::mock(
            Uuid::new_v4(),
            &format!("worker-node-{i:02}"),
            AdapterType::Agy,
            vec!["rust".to_string()],
            if i % 2 == 0 { AgentStatus::Busy } else { AgentStatus::Idle },
        ));
    }

    assert_eq!(state.agents.len(), 30);
    terminal.draw(|f| render(f, &state)).expect("Render 30 agents fleet");
}

#[test]
fn test_tier2_fleet_rendering_agent_with_long_strings() {
    let mut terminal = create_terminal(140, 35);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Dashboard;

    let long_name = "agent-supercalifragilisticexpialidocious-cluster-worker-99";
    state.agents.push(Agent::mock(
        Uuid::new_v4(),
        long_name,
        AdapterType::Agy,
        vec!["machine-learning".to_string(), "cuda".to_string()],
        AgentStatus::Idle,
    ));

    terminal.draw(|f| render(f, &state)).expect("Render long agent strings");
    let content = buffer_to_string(&terminal);
    assert!(!content.is_empty());
}

// ============================================================================
// TIER 3: CROSS-FEATURE COMBINATIONS & STATE TRANSITIONS
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 3.1: Task Lifecycle Event Stream Projections (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier3_lifecycle_projection_task_started() {
    let mut state = AppState::new().with_mock_data();
    let task_id = state.review_tasks[0].task.id;
    let agent_id = state.agents[0].id;

    let msg = AgentMessage::TaskStarted {
        agent_id,
        task_id,
        idempotency_key: "idem-001".to_string(),
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Executing);
    assert_eq!(state.review_tasks[0].task.assigned_agent_id, Some(agent_id));
    assert_eq!(state.agents[0].status, AgentStatus::Busy);
    assert!(state.status_message.as_ref().unwrap().contains("EXECUTING"));
}

#[test]
fn test_tier3_lifecycle_projection_progress_update() {
    let mut state = AppState::new().with_mock_data();
    let task_id = state.review_tasks[0].task.id;
    let agent_id = state.agents[0].id;

    let msg = AgentMessage::ProgressUpdate {
        agent_id,
        task_id,
        percent: 50,
        message: "Compiling crates/coordinator".to_string(),
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert!(state.status_message.as_ref().unwrap().contains("50%"));
    assert!(state.status_message.as_ref().unwrap().contains("Compiling crates/coordinator"));
}

#[test]
fn test_tier3_lifecycle_projection_blocked() {
    let mut state = AppState::new().with_mock_data();
    let task_id = state.review_tasks[0].task.id;
    let agent_id = state.agents[0].id;

    let msg = AgentMessage::Blocked {
        agent_id,
        task_id,
        reason: "Waiting for dependency PLAN-1 to complete".to_string(),
        blocking_task_id: None,
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Blocked);
    assert_eq!(state.agents[0].status, AgentStatus::Blocked);
    assert!(state.status_message.as_ref().unwrap().contains("BLOCKED"));
}

#[test]
fn test_tier3_lifecycle_projection_completed() {
    let mut state = AppState::new().with_mock_data();
    let task_id = state.review_tasks[0].task.id;
    let agent_id = state.agents[0].id;

    let msg = AgentMessage::Completed {
        agent_id,
        task_id,
        summary: "Database migrations successfully applied".to_string(),
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Completed);
    assert_eq!(state.agents[0].status, AgentStatus::Idle);
    assert_eq!(state.agents[0].current_task_id, None);
    assert!(state.status_message.as_ref().unwrap().contains("COMPLETED"));
}

#[test]
fn test_tier3_lifecycle_projection_failed() {
    let mut state = AppState::new().with_mock_data();
    let task_id = state.review_tasks[0].task.id;
    let agent_id = state.agents[0].id;

    let msg = AgentMessage::Failed {
        agent_id,
        task_id,
        error: "Compilation failed with exit code 101".to_string(),
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Failed);
    assert_eq!(state.agents[0].status, AgentStatus::Error);
    assert_eq!(state.agents[0].current_task_id, None);
    assert!(state.status_message.as_ref().unwrap().contains("FAILED"));
}

// ----------------------------------------------------------------------------
// Tier 3.2: Overlap Warning Acknowledgment Unblocks Approval
// ----------------------------------------------------------------------------

#[test]
fn test_tier3_overlap_acknowledgment_unblocks_approval() {
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;
    let warning_id = state.review_tasks[0].overlap_warnings[0].id;

    // Acknowledge overlap
    let ack_action = state.handle_key(press_key(KeyCode::Char('a')));
    assert_eq!(ack_action, Some(TuiAction::AcknowledgeOverlap { warning_id }));
    assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);

    // Now approve task
    let approve_action = state.handle_key(press_key(KeyCode::Char('y')));
    assert_eq!(approve_action, Some(TuiAction::ApproveTask { task_id: task0_id }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
}

// ----------------------------------------------------------------------------
// Tier 3.3: Visual DAG View Toggle & ASCII Tree Buffer Verification
// ----------------------------------------------------------------------------

#[test]
fn test_tier3_dag_view_toggle_and_rendering() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;
    assert!(!state.show_dag_view);

    // Press [G] to toggle DAG View
    state.handle_key(press_key(KeyCode::Char('g')));
    assert!(state.show_dag_view);

    // Render DAG View frame
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Task Dependency Graph (DAG)");
    assert_buffer_contains(&terminal, "PLAN-1");

    // Press [G] to toggle back to card view
    state.handle_key(press_key(KeyCode::Char('g')));
    assert!(!state.show_dag_view);
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Proposed Task Decomposition (Human Review)");
}

// ----------------------------------------------------------------------------
// Tier 3.4: Help Modal Cheatsheet Overlay Verification
// ----------------------------------------------------------------------------

#[test]
fn test_tier3_help_modal_toggle_and_rendering() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    assert!(!state.show_help_modal);

    // Press [?] to toggle help modal
    state.handle_key(press_key(KeyCode::Char('?')));
    assert!(state.show_help_modal);

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Operator Keyboard Shortcuts & Help Reference");
    assert_buffer_contains(&terminal, "Direct screen jump");
    assert_buffer_contains(&terminal, "Cycle screens");

    // Press [?] or [Esc] to dismiss modal
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_help_modal);
}

// ----------------------------------------------------------------------------
// Tier 3.5: Connected Agent Capability Inspection Modal
// ----------------------------------------------------------------------------

#[test]
fn test_tier3_agent_capability_inspection_modal() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;
    assert!(!state.show_agent_inspector);

    // Press [C] on Dashboard to toggle agent capability inspector
    state.handle_key(press_key(KeyCode::Char('c')));
    assert!(state.show_agent_inspector);

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "Connected Agent Capability Profile");
    assert_buffer_contains(&terminal, "agent-backend");

    // Press [Esc] to close inspector
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_agent_inspector);
}

// ============================================================================
// TIER 4: REAL-WORLD OPERATOR WORKFLOWS
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 4.1: End-to-End Operator Review Flow
// ----------------------------------------------------------------------------

#[test]
fn test_tier4_e2e_operator_review_flow() {
    let mut terminal = create_terminal(140, 40);
    let mut state = AppState::new();

    // 1. Operator lands on Dashboard and navigates to ProjectManagement / ProjectInput via [2]
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
    state.handle_key(press_key(KeyCode::Char('2')));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);
    terminal.draw(|f| render(f, &state)).unwrap();

    // 2. Enters project name and description
    state.handle_key(press_key(KeyCode::Char('i')));
    for c in "Distributed Mesh Architecture".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    state.handle_key(press_key(KeyCode::Tab));
    for c in "Phase 20 operator management".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }

    // 3. Submits project
    let submit_action = state.handle_key(press_key(KeyCode::Enter));
    assert!(matches!(submit_action, Some(TuiAction::SubmitProject { .. })));

    // 4. Coordinator proposes 3 tasks
    let proj_id = Uuid::new_v4();
    let task1 = sample_task(Uuid::new_v4(), proj_id, "TASK-1", "Database schema init", TaskStatus::HumanReview);
    let task2 = sample_task(Uuid::new_v4(), proj_id, "TASK-2", "NATS JetStream handler", TaskStatus::HumanReview);
    let task3 = sample_task(Uuid::new_v4(), proj_id, "TASK-3", "TUI visual verification", TaskStatus::HumanReview);

    state.review_tasks = vec![
        ReviewTaskState::new(task1.clone()),
        ReviewTaskState::new(task2.clone()),
        ReviewTaskState::new(task3.clone()),
    ];

    // 5. Operator switches to PlanReview via [3]
    state.handle_key(press_key(KeyCode::Char('3')));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "TASK-1");

    // 6. Operator approves Task 1 with [Y]
    let approve1 = state.handle_key(press_key(KeyCode::Char('y')));
    assert_eq!(approve1, Some(TuiAction::ApproveTask { task_id: task1.id }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(state.selected_task_index, 1);

    // 7. Operator edits Task 2 description with [E]
    state.handle_key(press_key(KeyCode::Char('e')));
    for c in " (with idempotency guard)".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    let edit2 = state.handle_key(press_key(KeyCode::Enter));
    assert!(matches!(edit2, Some(TuiAction::EditTaskDescription { .. })));
    assert_eq!(state.review_tasks[1].task.status, TaskStatus::Approved);
    assert!(state.review_tasks[1].task.description.contains("(with idempotency guard)"));

    // 8. Operator approves Task 3
    state.selected_task_index = 2;
    let approve3 = state.handle_key(press_key(KeyCode::Char('y')));
    assert_eq!(approve3, Some(TuiAction::ApproveTask { task_id: task3.id }));
    assert_eq!(state.review_tasks[2].task.status, TaskStatus::Approved);

    // All tasks now approved
    assert!(state.review_tasks.iter().all(|rt| rt.task.status == TaskStatus::Approved));
}

// ----------------------------------------------------------------------------
// Tier 4.2: Multi-Agent Operator Fleet Monitoring Flow
// ----------------------------------------------------------------------------

#[test]
fn test_tier4_e2e_operator_monitoring_flow() {
    let mut terminal = create_terminal(140, 40);
    let mut state = AppState::new().with_mock_data();
    let task0_id = state.review_tasks[0].task.id;
    let agent0_id = state.agents[0].id;
    let task1_id = state.review_tasks[1].task.id;
    let agent1_id = state.agents[1].id;

    // 1. Operator switches to Dashboard via [1]
    state.handle_key(press_key(KeyCode::Char('1')));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
    terminal.draw(|f| render(f, &state)).unwrap();

    // 2. Agent 0 picks up Task 0
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::TaskStarted {
        agent_id: agent0_id,
        task_id: task0_id,
        idempotency_key: "flow-idem-001".to_string(),
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Executing);
    assert_eq!(state.agents[0].status, AgentStatus::Busy);

    // 3. Agent 0 reports progress
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::ProgressUpdate {
        agent_id: agent0_id,
        task_id: task0_id,
        percent: 75,
        message: "Migration batch 75% complete".to_string(),
        timestamp: Utc::now(),
    }));

    // 4. Agent 1 becomes blocked on Task 1
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::Blocked {
        agent_id: agent1_id,
        task_id: task1_id,
        reason: "Waiting for DB migration to finish".to_string(),
        blocking_task_id: None,
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[1].task.status, TaskStatus::Blocked);
    assert_eq!(state.agents[1].status, AgentStatus::Blocked);

    // 5. Operator switches to Diagnostics to inspect KPIs & event stream via [5]
    state.handle_key(press_key(KeyCode::Char('5')));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);
    terminal.draw(|f| render(f, &state)).unwrap();

    // 6. Agent 0 completes task
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::Completed {
        agent_id: agent0_id,
        task_id: task0_id,
        summary: "Migration successful".to_string(),
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Completed);
    assert_eq!(state.agents[0].status, AgentStatus::Idle);

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "COMPLETED");
}
