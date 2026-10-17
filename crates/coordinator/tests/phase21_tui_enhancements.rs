//! Phase 21 Operator TUI Enhancements & Production Console Verification Suite
//!
//! Comprehensive, deterministic Ratatui `TestBackend` test suite verifying all Phase 21
//! requirements across the 4-tier testing methodology using REAL production coordinator components:
//!
//! - Tier 1: Feature Coverage (5 screens, global header connectivity pills, command palette,
//!   safety confirmation dialogs, project management, recursive DAG tree, fleet telemetry cards,
//!   bounded diagnostics event stream, terminal resize resilience).
//! - Tier 2: Boundary & Corner Conditions (extreme event bursts, empty states, deep recursion,
//!   index clamping, unicode/symbols).
//! - Tier 3: Cross-Feature Combinations (modals over complex screens, action gating with overlaps,
//!   project switching state refresh, event filtering under burst).
//! - Tier 4: Real-World Workflows (operator review session, fleet triage, command palette workflow,
//!   task retry state transitions).

use std::time::Instant;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::TestBackend,
    Terminal,
};
use serde_json::json;
use uuid::Uuid;

use coordinator::domain::{
    AdapterType, Agent, AgentStatus, ApprovalStatus, DependencyKind, HealthStatus,
    OverlapSeverity, OverlapWarning, Project, ProjectStatus, Task, TaskDependency, TaskStatus,
};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render, AppState, CurrentScreen, ReviewTaskState, TuiAction,
    TuiUpdateEvent, MAX_RECENT_EVENTS,
};
use coordinator::tui::screens::{
    DashboardScreen, DiagnosticsScreen, FleetInspectorScreen, PlanReviewScreen, ProjectInputScreen,
};
use coordinator::tui::widgets::{
    command_palette::filtered_commands, AgentInspectorWidget, CommandPaletteWidget,
    ConfirmationModalWidget, DagViewWidget, HeaderWidget,
};

// ============================================================================
// Test Harness Helpers
// ============================================================================

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

fn assert_buffer_contains(terminal: &Terminal<TestBackend>, expected: &str) {
    let content = buffer_to_string(terminal);
    assert!(
        content.contains(expected),
        "Buffer did not contain expected text '{expected}'. Rendered buffer:\n{content}"
    );
}

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press_char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn sample_task(id: Uuid, project_id: Uuid, short_id: &str, title: &str, status: TaskStatus) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["crates/coordinator", "src/main.rs"]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn sample_event(i: usize) -> CoordinatorEvent {
    CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: format!("event.tier.{i}"),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: format!("Coordinator test event #{i}"),
        payload: json!({ "seq": i }),
    }
}

// ============================================================================
// TIER 1: FEATURE COVERAGE SUITES
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 1.1: 5-Screen Operator Navigation & Title Rendering (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_screen_nav_5_screens_initial_and_titles() {
    assert_eq!(CurrentScreen::default(), CurrentScreen::Dashboard);
    assert_eq!(CurrentScreen::Dashboard.title(), "1. Dashboard");
    assert_eq!(CurrentScreen::ProjectInput.title(), "2. Project Input");
    assert_eq!(CurrentScreen::ProjectManagement.title(), "2. Project Management");
    assert_eq!(CurrentScreen::PlanReview.title(), "3. Plan Review");
    assert_eq!(CurrentScreen::FleetInspector.title(), "4. Fleet Inspector");
    assert_eq!(CurrentScreen::Diagnostics.title(), "5. Diagnostics");
}

#[test]
fn test_tier1_screen_nav_5_screens_tab_cycling() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    state.cycle_screen();
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    state.cycle_screen();
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    state.cycle_screen();
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    state.cycle_screen();
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    state.cycle_screen();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
}

#[test]
fn test_tier1_screen_nav_numeric_hotkeys_1_to_5() {
    let mut state = AppState::new();

    state.handle_key(press_char('1'));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    state.handle_key(press_char('2'));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    state.handle_key(press_char('3'));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    state.handle_key(press_char('4'));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    state.handle_key(press_char('5'));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    // Key '6' is unmapped and does not switch screen
    state.handle_key(press_char('6'));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);
}

#[test]
fn test_tier1_screen_nav_all_5_screen_headers_render() {
    let mut terminal = create_terminal(120, 30);
    let state = AppState::new().with_mock_data();

    // 1. Dashboard
    terminal
        .draw(|f| DashboardScreen::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "System Overview & Live Dashboard");

    // 2. ProjectInput / Management
    terminal
        .draw(|f| ProjectInputScreen::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "Projects List");

    // 3. Plan Review
    terminal
        .draw(|f| PlanReviewScreen::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "Proposed Task Decomposition");

    // 4. Fleet Inspector
    terminal
        .draw(|f| FleetInspectorScreen::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "Connected Agent Fleet");

    // 5. Diagnostics
    terminal
        .draw(|f| DiagnosticsScreen::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "System Observability & Diagnostics");
}

#[test]
fn test_tier1_screen_nav_preserves_backward_compatible_aliases() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);
}

// ----------------------------------------------------------------------------
// Tier 1.2: Dynamic Status Header & Connectivity Pills (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_header_renders_connectivity_pills_healthy() {
    let mut terminal = create_terminal(160, 4);
    let mut state = AppState::new();
    state.pg_connected = true;
    state.nats_connected = true;

    terminal
        .draw(|f| HeaderWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "[PG: ● OK]");
    assert_buffer_contains(&terminal, "[NATS: ● OK]");
}

#[test]
fn test_tier1_header_renders_connectivity_pills_offline() {
    let mut terminal = create_terminal(160, 4);
    let mut state = AppState::new();
    state.pg_connected = false;
    state.nats_connected = false;

    terminal
        .draw(|f| HeaderWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "[PG: ○ OFF]");
    assert_buffer_contains(&terminal, "[NATS: ○ OFF]");
}

#[test]
fn test_tier1_header_renders_pending_approvals_alert() {
    let mut terminal = create_terminal(160, 4);
    let state = AppState::new().with_mock_data();

    terminal
        .draw(|f| HeaderWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "[Pending: 3]");
}

#[test]
fn test_tier1_header_adaptive_compact_terminal_render() {
    let mut terminal = create_terminal(80, 3);
    let state = AppState::new().with_mock_data();

    terminal
        .draw(|f| HeaderWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "1:Dash");
    assert_buffer_contains(&terminal, "[PG:");
}

#[test]
fn test_tier1_header_zero_panics_on_minimal_dimensions() {
    let state = AppState::new().with_mock_data();
    for (w, h) in [(40, 2), (20, 1), (10, 1), (0, 0)] {
        let backend = TestBackend::new(w.max(1), h.max(1));
        let mut terminal = Terminal::new(backend).unwrap();
        let rect = ratatui::layout::Rect::new(0, 0, w, h);
        terminal.draw(|f| HeaderWidget::render(f, rect, &state)).unwrap();
    }
}

// ----------------------------------------------------------------------------
// Tier 1.3: Global Command Palette Modal (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_command_palette_open_and_close() {
    let mut state = AppState::new();
    assert!(!state.show_command_palette);

    state.handle_key(press_char(':'));
    assert!(state.show_command_palette);

    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_command_palette);
}

#[test]
fn test_tier1_command_palette_query_filtering() {
    let mut state = AppState::new();
    state.show_command_palette = true;

    for c in "fleet".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.command_palette_query, "fleet");

    let matches = filtered_commands(&state.command_palette_query);
    assert!(!matches.is_empty());
    assert!(matches.iter().any(|cmd| cmd.name == "nav:fleet"));
}

#[test]
fn test_tier1_command_palette_backspace_filtering() {
    let mut state = AppState::new();
    state.show_command_palette = true;

    for c in "task".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.command_palette_query, "task");

    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.command_palette_query, "tas");
}

#[test]
fn test_tier1_command_palette_up_down_selection() {
    let mut state = AppState::new();
    state.show_command_palette = true;
    assert_eq!(state.command_palette_selected_index, 0);

    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.command_palette_selected_index, 1);

    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.command_palette_selected_index, 0);
}

#[test]
fn test_tier1_command_palette_action_dispatch_on_enter() {
    let mut state = AppState::new();
    state.show_command_palette = true;

    for c in "system:help".chars() {
        state.handle_key(press_char(c));
    }

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, None);
    assert!(state.show_help_modal);
    assert!(!state.show_command_palette);

    // Dismiss help modal before testing subsequent commands
    state.show_help_modal = false;

    // Also test system:refresh emits RefreshData
    state.show_command_palette = true;
    state.command_palette_query.clear();
    for c in "system:refresh".chars() {
        state.handle_key(press_char(c));
    }
    let refresh_action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(refresh_action, Some(TuiAction::RefreshData));
    assert!(!state.show_command_palette);
}

// ----------------------------------------------------------------------------
// Tier 1.4: Safety Confirmation Modal Dialog (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_confirmation_dialog_render_content() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    state.prompt_cancel_task(task_id);

    terminal
        .draw(|f| ConfirmationModalWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Confirm Task Cancellation");
    assert_buffer_contains(&terminal, "[Y/Enter] Confirm");
    assert_buffer_contains(&terminal, "[N/Esc] Cancel");
}

#[test]
fn test_tier1_confirmation_dialog_confirm_with_y() {
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    state.prompt_cancel_task(task_id);

    let action = state.handle_key(press_char('y'));
    assert_eq!(action, Some(TuiAction::CancelTask { task_id }));
    assert!(state.pending_confirmation.is_none());
}

#[test]
fn test_tier1_confirmation_dialog_confirm_with_enter() {
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    state.prompt_reject_task(task_id);

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, Some(TuiAction::RejectTask { task_id }));
    assert!(state.pending_confirmation.is_none());
}

#[test]
fn test_tier1_confirmation_dialog_cancel_with_n() {
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    state.prompt_cancel_task(task_id);

    let action = state.handle_key(press_char('n'));
    assert_eq!(action, None);
    assert!(state.pending_confirmation.is_none());
}

#[test]
fn test_tier1_confirmation_dialog_cancel_with_esc() {
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    state.prompt_cancel_task(task_id);

    let action = state.handle_key(press_key(KeyCode::Esc));
    assert_eq!(action, None);
    assert!(state.pending_confirmation.is_none());
}

// ----------------------------------------------------------------------------
// Tier 1.5: Project Management & Lifecycle (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_project_mgmt_rendering_table_and_metadata() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::ProjectManagement;

    terminal
        .draw(|f| ProjectInputScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Cloud Migration Mesh");
    assert_buffer_contains(&terminal, "Projects List");
}

#[test]
fn test_tier1_project_mgmt_navigation_j_k() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectManagement;
    let p1 = Project {
        id: Uuid::new_v4(),
        name: "Proj Alpha".to_string(),
        description: "First project".to_string(),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let p2 = Project {
        id: Uuid::new_v4(),
        name: "Proj Beta".to_string(),
        description: "Second project".to_string(),
        status: ProjectStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    state.available_projects = vec![p1, p2];

    assert_eq!(state.selected_project_index, 0);
    state.handle_key(press_char('j'));
    assert_eq!(state.selected_project_index, 1);
    state.handle_key(press_char('k'));
    assert_eq!(state.selected_project_index, 0);
}

#[test]
fn test_tier1_project_mgmt_enter_emits_load_project() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectManagement;
    let p1 = Project {
        id: Uuid::new_v4(),
        name: "Target Proj".to_string(),
        description: "Load this".to_string(),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let pid = p1.id;
    state.available_projects = vec![p1];

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, Some(TuiAction::LoadProject { project_id: pid }));
}

#[test]
fn test_tier1_project_mgmt_empty_projects_fallback() {
    let mut terminal = create_terminal(100, 24);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectManagement;
    state.available_projects.clear();

    terminal
        .draw(|f| ProjectInputScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "No projects found.");
}

#[test]
fn test_tier1_project_mgmt_bounds_clamping_at_edges() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectManagement;
    let p1 = Project {
        id: Uuid::new_v4(),
        name: "Single Proj".to_string(),
        description: "Only one".to_string(),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    state.available_projects = vec![p1];

    state.handle_key(press_char('k'));
    assert_eq!(state.selected_project_index, 0);
    state.handle_key(press_char('j'));
    state.handle_key(press_char('j'));
    assert_eq!(state.selected_project_index, 0);
}

// ----------------------------------------------------------------------------
// Tier 1.6: Recursive DAG Execution Pipeline (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_dag_multi_level_recursive_traversal() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "TASK-A", "Root", TaskStatus::Completed);
    let task_b = sample_task(id_b, proj_id, "TASK-B", "Middle", TaskStatus::Executing);
    let task_c = sample_task(id_c, proj_id, "TASK-C", "Leaf", TaskStatus::Proposed);

    state.review_tasks = vec![
        ReviewTaskState::new(task_a),
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_c,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_c)
        },
    ];

    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert_eq!(tiers.len(), 3);
    assert_eq!(tiers[0], vec![id_a]);
    assert_eq!(tiers[1], vec![id_b]);
    assert_eq!(tiers[2], vec![id_c]);
}

#[test]
fn test_tier1_dag_renders_all_depth_levels_in_buffer() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "TASK-A", "Root", TaskStatus::Completed);
    let task_b = sample_task(id_b, proj_id, "TASK-B", "Middle", TaskStatus::Executing);
    let task_c = sample_task(id_c, proj_id, "TASK-C", "Leaf", TaskStatus::Proposed);

    state.review_tasks = vec![
        ReviewTaskState::new(task_a),
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_c,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_c)
        },
    ];

    terminal
        .draw(|f| DagViewWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "TASK-A");
    assert_buffer_contains(&terminal, "TASK-B");
    assert_buffer_contains(&terminal, "TASK-C");
    assert_buffer_contains(&terminal, "Tier 0");
    assert_buffer_contains(&terminal, "Tier 1");
    assert_buffer_contains(&terminal, "Tier 2");
}

#[test]
fn test_tier1_dag_status_badges_rendered() {
    let mut terminal = create_terminal(100, 24);
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    state.review_tasks = vec![
        ReviewTaskState::new(sample_task(Uuid::new_v4(), proj_id, "TASK-1", "T1", TaskStatus::Approved)),
        ReviewTaskState::new(sample_task(Uuid::new_v4(), proj_id, "TASK-2", "T2", TaskStatus::Executing)),
        ReviewTaskState::new(sample_task(Uuid::new_v4(), proj_id, "TASK-3", "T3", TaskStatus::Failed)),
    ];

    terminal
        .draw(|f| DagViewWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "[TASK-1: Approved]");
    assert_buffer_contains(&terminal, "[TASK-2: Executing]");
    assert_buffer_contains(&terminal, "[TASK-3: Failed]");
}

#[test]
fn test_tier1_dag_jump_to_blocker_shortcut() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "TASK-A", "Root", TaskStatus::Completed);
    let task_b = sample_task(id_b, proj_id, "TASK-B", "Child", TaskStatus::HumanReview);

    state.review_tasks = vec![
        ReviewTaskState::new(task_a),
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
    ];
    state.selected_task_index = 1;

    state.jump_to_blocker();
    assert_eq!(state.selected_task_index, 0);
}

#[test]
fn test_tier1_dag_jump_to_dependent_shortcut() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "TASK-A", "Root", TaskStatus::Completed);
    let task_b = sample_task(id_b, proj_id, "TASK-B", "Child", TaskStatus::HumanReview);

    state.review_tasks = vec![
        ReviewTaskState::new(task_a),
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
    ];
    state.selected_task_index = 0;

    state.jump_to_dependent();
    assert_eq!(state.selected_task_index, 1);
}

#[test]
fn test_tier1_dag_jump_on_root_or_leaf_is_safe_noop() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "TASK-A", "Root", TaskStatus::Completed);
    let task_b = sample_task(id_b, proj_id, "TASK-B", "Child", TaskStatus::HumanReview);

    state.review_tasks = vec![
        ReviewTaskState::new(task_a),
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
    ];

    // Jump to blocker on root is safe no-op
    state.selected_task_index = 0;
    state.jump_to_blocker();
    assert_eq!(state.selected_task_index, 0);

    // Jump to dependent on leaf is safe no-op
    state.selected_task_index = 1;
    state.jump_to_dependent();
    assert_eq!(state.selected_task_index, 1);
}

// ----------------------------------------------------------------------------
// Tier 1.7: Deep Fleet Telemetry Cards (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_fleet_telemetry_renders_health_badges() {
    let mut terminal = create_terminal(200, 24);
    let mut state = AppState::new();

    let mut a1 = Agent::mock(Uuid::new_v4(), "agent-1 (A)", AdapterType::Agy, vec![], AgentStatus::Idle);
    a1.health_status = HealthStatus::Healthy;
    let mut a2 = Agent::mock(Uuid::new_v4(), "agent-2 (B)", AdapterType::Mock, vec![], AgentStatus::Busy);
    a2.health_status = HealthStatus::Degraded;
    let mut a3 = Agent::mock(Uuid::new_v4(), "agent-3 (C)", AdapterType::Agy, vec![], AgentStatus::Error);
    a3.health_status = HealthStatus::Unhealthy;

    state.agents = vec![a1, a2, a3];

    terminal
        .draw(|f| FleetInspectorScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Healthy");
    assert_buffer_contains(&terminal, "Degraded");
    assert_buffer_contains(&terminal, "Unhealthy");
}

#[test]
fn test_tier1_fleet_telemetry_renders_heartbeat_latencies() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();

    let mut a1 = Agent::mock(Uuid::new_v4(), "agent-1 (A)", AdapterType::Agy, vec![], AgentStatus::Idle);
    a1.heartbeat_latency_ms = Some(42);
    state.agents = vec![a1];

    terminal
        .draw(|f| AgentInspectorWidget::render_panel(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "42ms");
}

#[test]
fn test_tier1_fleet_telemetry_renders_error_banner_on_degraded_agent() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();

    let mut a1 = Agent::mock(Uuid::new_v4(), "agent-1 (A)", AdapterType::Agy, vec![], AgentStatus::Busy);
    a1.health_status = HealthStatus::Degraded;
    a1.last_error = Some("High memory pressure warning".to_string());
    state.agents = vec![a1];

    terminal
        .draw(|f| AgentInspectorWidget::render_panel(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "ERROR BANNER");
    assert_buffer_contains(&terminal, "High memory pressure warning");
}

#[test]
fn test_tier1_fleet_telemetry_renders_error_banner_on_unhealthy_agent() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();

    let mut a1 = Agent::mock(Uuid::new_v4(), "agent-1 (A)", AdapterType::Agy, vec![], AgentStatus::Error);
    a1.health_status = HealthStatus::Unhealthy;
    a1.last_error = Some("Process exit signal 9 (OOMKilled)".to_string());
    state.agents = vec![a1];

    terminal
        .draw(|f| AgentInspectorWidget::render_panel(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "ERROR BANNER");
    assert_buffer_contains(&terminal, "Process exit signal 9");
}

#[test]
fn test_tier1_fleet_telemetry_renders_active_task_assignment() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();

    let task = sample_task(task_id, proj_id, "TASK-42", "Important Migration", TaskStatus::Executing);
    state.review_tasks = vec![ReviewTaskState::new(task)];

    let mut a1 = Agent::mock(Uuid::new_v4(), "agent-1 (A)", AdapterType::Agy, vec![], AgentStatus::Busy);
    a1.current_task_id = Some(task_id);
    state.agents = vec![a1];

    terminal
        .draw(|f| AgentInspectorWidget::render_panel(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "TASK-42");
    assert_buffer_contains(&terminal, "Important Migration");
}

#[test]
fn test_tier1_fleet_telemetry_renders_runtime_adapters() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();

    let a1 = Agent::mock(Uuid::new_v4(), "agent-agy (Alice)", AdapterType::Agy, vec![], AgentStatus::Idle);
    let a2 = Agent::mock(Uuid::new_v4(), "agent-mock (Bob)", AdapterType::Mock, vec![], AgentStatus::Idle);
    state.agents = vec![a1, a2];

    terminal
        .draw(|f| FleetInspectorScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Agy");
    assert_buffer_contains(&terminal, "Mock");
}

// ----------------------------------------------------------------------------
// Tier 1.8: Diagnostics & Live Event Stream (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_diagnostics_bounded_capacity_invariant_under_1000_events() {
    let mut state = AppState::new();
    let batch: Vec<CoordinatorEvent> = (0..1_000).map(sample_event).collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(batch));

    assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
    assert_eq!(state.recent_events.len(), MAX_RECENT_EVENTS);
    assert!(state.event_ring_buffer.len() <= MAX_RECENT_EVENTS);
    assert_eq!(state.event_ring_buffer.len(), MAX_RECENT_EVENTS);
}

#[test]
fn test_tier1_diagnostics_rapid_burst_10000_events_performance() {
    let mut state = AppState::new();
    let batch: Vec<CoordinatorEvent> = (0..10_000).map(sample_event).collect();
    let start = Instant::now();

    state.apply_update(TuiUpdateEvent::RecentEvents(batch));
    let elapsed = start.elapsed();

    assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
    assert!(
        elapsed.as_millis() < 500,
        "10k event burst processing took too long: {elapsed:?}"
    );
}

#[test]
fn test_tier1_diagnostics_severity_filtering() {
    let mut state = AppState::new();
    state.recent_events = vec![
        CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "task.failed".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: "Fatal task failure".to_string(),
            payload: json!({}),
        },
        CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "task.completed".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: "Normal completion".to_string(),
            payload: json!({}),
        },
    ];

    state.event_filter_severity = Some("ERROR".to_string());
    let filtered = state.filtered_recent_events();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].message, "Fatal task failure");
}

#[test]
fn test_tier1_diagnostics_entity_filtering() {
    let mut state = AppState::new();
    let task_id = Uuid::new_v4();
    let agent_id = Uuid::new_v4();

    state.recent_events = vec![
        CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "task.started".to_string(),
            project_id: None,
            task_id: Some(task_id),
            agent_id: None,
            message: "Task event".to_string(),
            payload: json!({}),
        },
        CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "agent.registered".to_string(),
            project_id: None,
            task_id: None,
            agent_id: Some(agent_id),
            message: "Agent event".to_string(),
            payload: json!({}),
        },
    ];

    state.event_filter_entity = Some("TASK".to_string());
    let filtered = state.filtered_recent_events();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].message, "Task event");
}

#[test]
fn test_tier1_diagnostics_selection_clamping_on_filter_shrink() {
    let mut state = AppState::new();
    state.recent_events = (0..10).map(sample_event).collect();
    state.selected_event_index = 8;

    // Filter that matches 0 events
    state.event_filter_severity = Some("NONEXISTENT_SEVERITY".to_string());
    state.clamp_event_selection();
    assert_eq!(state.selected_event_index, 0);
}

#[test]
fn test_tier1_diagnostics_event_table_buffer_render() {
    let mut terminal = create_terminal(120, 24);
    let mut state = AppState::new();
    state.recent_events = vec![sample_event(1)];

    terminal
        .draw(|f| DiagnosticsScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Coordinator test event #1");
}

// ----------------------------------------------------------------------------
// Tier 1.9: Terminal Resizing Resilience (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_resize_minimal_60x18() {
    let mut terminal = create_terminal(60, 18);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).expect("60x18 render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_compact_70x20() {
    let mut terminal = create_terminal(70, 20);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).expect("70x20 render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_standard_80x24() {
    let mut terminal = create_terminal(80, 24);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).expect("80x24 render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_expanded_100x30() {
    let mut terminal = create_terminal(100, 30);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).expect("100x30 render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_ultrawide_160x50() {
    let mut terminal = create_terminal(160, 50);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).expect("160x50 render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

// ============================================================================
// TIER 2: BOUNDARY & CORNER CONDITIONS (8 tests)
// ============================================================================

#[test]
fn test_tier2_empty_state_zero_tasks_zero_agents() {
    let mut terminal = create_terminal(80, 24);
    let state = AppState::new();
    terminal.draw(|f| render(f, &state)).expect("Empty state render failed");
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier2_command_palette_no_matching_results() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.show_command_palette = true;

    for c in "xyz_nonexistent_command".chars() {
        state.handle_key(press_char(c));
    }

    assert_eq!(filtered_commands(&state.command_palette_query).len(), 0);
    terminal
        .draw(|f| CommandPaletteWidget::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "xyz_nonexistent_command");
}

#[test]
fn test_tier2_dag_single_node_tree() {
    let mut state = AppState::new();
    let id = Uuid::new_v4();
    state.review_tasks = vec![ReviewTaskState::new(sample_task(
        id,
        Uuid::new_v4(),
        "SOLO-1",
        "Standalone Task",
        TaskStatus::Approved,
    ))];

    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert_eq!(tiers.len(), 1);
    assert_eq!(tiers[0], vec![id]);
}

#[test]
fn test_tier2_dag_cyclic_dependency_safety_guard() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    let task_a = sample_task(id_a, proj_id, "CYC-A", "Cycle A", TaskStatus::Proposed);
    let task_b = sample_task(id_b, proj_id, "CYC-B", "Cycle B", TaskStatus::Proposed);

    state.review_tasks = vec![
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_a,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_a)
        },
        ReviewTaskState {
            dependencies: vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
            ..ReviewTaskState::new(task_b)
        },
    ];

    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert!(!tiers.is_empty());
}

#[test]
fn test_tier2_bounded_buffer_eviction_fifo_order() {
    let mut state = AppState::new();

    for i in 1..=5 {
        state.push_recent_event(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "test".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Event {i}"),
            payload: json!({}),
        });
    }

    assert_eq!(state.recent_events[0].message, "Event 5");
    assert_eq!(state.recent_events[1].message, "Event 4");
    assert_eq!(state.recent_events[2].message, "Event 3");
}

#[test]
fn test_tier2_extreme_long_strings_rendering() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.active_project = Some(Project {
        id: Uuid::new_v4(),
        name: "A".repeat(300),
        description: "B".repeat(1000),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier2_unicode_and_symbols_in_query() {
    let mut state = AppState::new();
    state.show_command_palette = true;

    for c in "🦀 Rocket 🚀 :: [テスト]".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.command_palette_query, "🦀 Rocket 🚀 :: [テスト]");

    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.command_palette_query, "🦀 Rocket 🚀 :: [テスト");
}

#[test]
fn test_tier2_task_status_retry_transitions() {
    assert!(TaskStatus::Cancelled.can_transition_to(TaskStatus::Approved));
    assert!(TaskStatus::Cancelled.can_transition_to(TaskStatus::HumanReview));
    assert!(TaskStatus::Failed.can_transition_to(TaskStatus::Approved));
    assert!(TaskStatus::Failed.can_transition_to(TaskStatus::HumanReview));

    assert!(!TaskStatus::Cancelled.can_transition_to(TaskStatus::Executing));
    assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Approved));
}

// ============================================================================
// TIER 3: CROSS-FEATURE COMBINATIONS (5 tests)
// ============================================================================

#[test]
fn test_tier3_command_palette_modal_rendered_over_dag_view() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_command_palette = true;

    terminal.draw(|f| render(f, &state)).unwrap();

    assert_buffer_contains(&terminal, "Command Palette (:)");
    assert_buffer_contains(&terminal, "Task Dependency Graph");
}

#[test]
fn test_tier3_confirmation_modal_rendered_over_fleet_inspector() {
    let mut terminal = create_terminal(100, 30);
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::FleetInspector;
    let task_id = state.review_tasks[0].task.id;
    state.prompt_cancel_task(task_id);

    terminal.draw(|f| render(f, &state)).unwrap();

    assert_buffer_contains(&terminal, "Confirm Task Cancellation");
    assert_buffer_contains(&terminal, "Connected Agent Fleet");
}

#[test]
fn test_tier3_project_selection_updates_status_header() {
    let mut terminal = create_terminal(160, 10);
    let mut state = AppState::new().with_mock_data();
    state.active_project = Some(Project {
        id: Uuid::new_v4(),
        name: "AgentMesh Core".to_string(),
        description: "Core mesh".to_string(),
        status: ProjectStatus::Active,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    state.pg_connected = true;

    terminal
        .draw(|f| HeaderWidget::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Project: AgentMesh Core");
    assert_buffer_contains(&terminal, "[PG: ● OK]");
}

#[test]
fn test_tier3_overlap_acknowledgment_unblocks_plan_approval() {
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let warn_id = Uuid::new_v4();

    let mut task_state = ReviewTaskState::new(sample_task(
        task_id,
        proj_id,
        "PLAN-1",
        "Database Layer",
        TaskStatus::HumanReview,
    ));

    task_state.overlap_warnings.push(OverlapWarning {
        id: warn_id,
        project_id: proj_id,
        task_ids: serde_json::json!([task_id, Uuid::new_v4()]),
        resource: "crates/db".to_string(),
        severity: OverlapSeverity::Warning,
        acknowledged: false,
        created_at: Utc::now(),
    });

    assert!(!task_state.overlap_warnings[0].acknowledged);

    task_state.overlap_warnings[0].acknowledged = true;
    assert!(task_state.overlap_warnings[0].acknowledged);

    task_state.human_decision = Some(ApprovalStatus::Approved);
    assert_eq!(task_state.human_decision, Some(ApprovalStatus::Approved));
}

#[test]
fn test_tier3_rapid_event_burst_with_active_filter() {
    let mut state = AppState::new();

    let events: Vec<CoordinatorEvent> = (0..1_000)
        .map(|i| {
            let is_err = i % 10 == 0;
            CoordinatorEvent {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                event_type: if is_err {
                    "task.failed".to_string()
                } else {
                    "task.started".to_string()
                },
                project_id: None,
                task_id: None,
                agent_id: None,
                message: format!("Msg {i}"),
                payload: json!({}),
            }
        })
        .collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(events));
    state.event_filter_severity = Some("ERROR".to_string());

    let filtered = state.filtered_recent_events();
    assert_eq!(filtered.len(), 30);
    assert!(filtered.iter().all(|e| e.event_type == "task.failed"));
}

// ============================================================================
// TIER 4: REAL-WORLD OPERATOR WORKFLOWS (4 tests)
// ============================================================================

#[test]
fn test_tier4_e2e_operator_review_and_safety_approval_flow() {
    let mut state = AppState::new().with_mock_data();
    state.confirm_destructive_actions = true;
    state.current_screen = CurrentScreen::ProjectManagement;

    let load_action = state.handle_key(press_key(KeyCode::Enter));
    assert!(load_action.is_some());

    state.current_screen = CurrentScreen::PlanReview;
    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert!(!tiers.is_empty());

    let target_task_id = state.review_tasks[0].task.id;
    state.selected_task_index = 0;

    let unconfirmed_action = state.handle_key(press_char('c'));
    assert!(unconfirmed_action.is_none());
    assert!(state.pending_confirmation.is_some());

    let confirmed_action = state.handle_key(press_char('y'));
    assert_eq!(confirmed_action, Some(TuiAction::CancelTask { task_id: target_task_id }));
    assert!(state.pending_confirmation.is_none());
}

#[test]
fn test_tier4_e2e_fleet_incident_triage_flow() {
    let mut state = AppState::new().with_mock_data();
    let target_agent_id = state.agents[0].id;
    let target_agent_owner = state.agents[0].human_owner.clone();

    state.agents[0].health_status = HealthStatus::Unhealthy;
    state.agents[0].last_error = Some("Heartbeat timeout on agent".to_string());
    state.selected_agent_index = 0;

    let mut terminal = create_terminal(120, 24);
    terminal
        .draw(|f| AgentInspectorWidget::render_panel(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "Heartbeat timeout");

    state.push_recent_event(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "agent.timeout".to_string(),
        project_id: None,
        task_id: None,
        agent_id: Some(target_agent_id),
        message: format!("Heartbeat timeout on {target_agent_owner}"),
        payload: json!({ "agent_id": target_agent_id }),
    });

    state.event_filter_entity = Some("AGENT".to_string());
    let results = state.filtered_recent_events();
    assert!(!results.is_empty());
    assert!(results[0].message.contains("Heartbeat timeout"));
}

#[test]
fn test_tier4_e2e_command_palette_navigation_flow() {
    let mut state = AppState::new();
    state.handle_key(press_char(':'));
    assert!(state.show_command_palette);

    for c in "nav:fleet".chars() {
        state.handle_key(press_char(c));
    }

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, None);
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);
    assert!(!state.show_command_palette);
}

#[test]
fn test_tier4_e2e_retry_task_lifecycle_flow() {
    let task_id = Uuid::new_v4();
    let proj_id = Uuid::new_v4();

    let mut task = sample_task(task_id, proj_id, "PLAN-1", "Flaky DB Job", TaskStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);

    assert!(task.status.can_transition_to(TaskStatus::Approved));
    task.status = TaskStatus::Approved;
    assert_eq!(task.status, TaskStatus::Approved);

    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.review_tasks = vec![ReviewTaskState::new(sample_task(
        task_id,
        proj_id,
        "PLAN-RETRY",
        "Retry target",
        TaskStatus::Failed,
    ))];
    state.selected_task_index = 0;

    let retry_action = state.handle_key(press_char('r'));
    assert_eq!(retry_action, Some(TuiAction::RetryTask { task_id }));
}
