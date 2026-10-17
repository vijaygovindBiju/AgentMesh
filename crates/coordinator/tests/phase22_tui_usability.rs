use chrono::Utc;
use coordinator::domain::{ApprovalStatus, Task, TaskStatus};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::screens::{DashboardScreen, PlanReviewScreen};
use coordinator::tui::state::{
    AppState, CurrentScreen, DashboardFocus, DestructiveAction, DiagnosticsFocus, TuiAction,
};
use coordinator::tui::widgets::FooterWidget;
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).unwrap()
}

fn assert_buffer_contains(terminal: &Terminal<TestBackend>, needle: &str) {
    let buffer = terminal.backend().buffer();
    let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        rendered.contains(needle),
        "Buffer did not contain '{needle}'. Rendered buffer:\n{buffer:?}"
    );
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
// 1. Mouse Wheel Scrolling Tests
// ============================================================================

#[test]
fn test_mouse_scroll_plan_review_tasks_list() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = false;

    let proj_id = Uuid::new_v4();
    for i in 1..=5 {
        let task = sample_task(Uuid::new_v4(), proj_id, &format!("TASK-{i}"), &format!("Task {i}"), TaskStatus::HumanReview);
        state.review_tasks.push(coordinator::tui::state::ReviewTaskState {
            task,
            suggested_agent_name: None,
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        });
    }

    assert_eq!(state.selected_task_index, 0);

    // Scroll Down -> selected_task_index increases
    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_down);
    assert_eq!(state.selected_task_index, 1);

    state.handle_mouse(scroll_down);
    assert_eq!(state.selected_task_index, 2);

    // Scroll Up -> selected_task_index decreases
    let scroll_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_up);
    assert_eq!(state.selected_task_index, 1);
}

#[test]
fn test_mouse_scroll_plan_review_dag_offset() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    assert_eq!(state.dag_scroll_offset, 0);

    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_down);
    assert_eq!(state.dag_scroll_offset, 1);

    state.handle_mouse(scroll_down);
    assert_eq!(state.dag_scroll_offset, 2);

    let scroll_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_up);
    assert_eq!(state.dag_scroll_offset, 1);

    state.handle_mouse(scroll_up);
    assert_eq!(state.dag_scroll_offset, 0);

    // Clamps at 0
    state.handle_mouse(scroll_up);
    assert_eq!(state.dag_scroll_offset, 0);
}

#[test]
fn test_mouse_scroll_dashboard_focused_tables() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    // 1. Tasks Table focus
    state.dashboard_focus = DashboardFocus::TasksTable;
    assert_eq!(state.selected_dashboard_task_index, 0);

    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 30,
        row: 15,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_down);
    assert_eq!(state.selected_dashboard_task_index, 1);

    let scroll_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 30,
        row: 15,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_up);
    assert_eq!(state.selected_dashboard_task_index, 0);

    // 2. Agents Table focus
    state.dashboard_focus = DashboardFocus::AgentsTable;
    assert_eq!(state.selected_agent_index, 0);
    state.handle_mouse(scroll_down);
    assert_eq!(state.selected_agent_index, 1);
    state.handle_mouse(scroll_up);
    assert_eq!(state.selected_agent_index, 0);
}

#[test]
fn test_mouse_scroll_diagnostics_events() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;
    state.diagnostics_focus = DiagnosticsFocus::EventsStream;

    for i in 1..=5 {
        state.push_recent_event(sample_event(i));
    }

    assert_eq!(state.selected_event_index, 0);

    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_down);
    assert_eq!(state.selected_event_index, 1);

    let scroll_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 20,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(scroll_up);
    assert_eq!(state.selected_event_index, 0);
}

// ============================================================================
// 2. Mouse Click Tab Jumping Tests
// ============================================================================

#[test]
fn test_mouse_click_header_tabs_switches_screen() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    // Click on Tab 2 (ProjectManagement) around col 83 (68 + 15)
    let click_tab2 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 83,
        row: 1,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(click_tab2);
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    // Click on Tab 3 (PlanReview) around col 93 (68 + 25)
    let click_tab3 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 93,
        row: 1,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(click_tab3);
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    // Click on Tab 4 (FleetInspector) around col 103 (68 + 35)
    let click_tab4 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 103,
        row: 1,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(click_tab4);
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    // Click on Tab 5 (Diagnostics) around col 116 (68 + 48)
    let click_tab5 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 116,
        row: 1,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(click_tab5);
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    // Click on Tab 1 (Dashboard) around col 73 (68 + 5)
    let click_tab1 = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 73,
        row: 1,
        modifiers: KeyModifiers::NONE,
    };
    state.handle_mouse(click_tab1);
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);
}

// ============================================================================
// 3. Fast Screen Cycling Keyboard Tests
// ============================================================================

#[test]
fn test_bracket_keys_cycle_screens_forward_and_backward() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    // ']' cycles forward
    state.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

    state.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    state.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

    state.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    state.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    // '[' cycles backward
    state.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    state.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);
}

#[test]
fn test_backtab_cycles_screen_backward() {
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

    state.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(state.current_screen, CurrentScreen::FleetInspector);
}

// ============================================================================
// 4. Bulk Task Approval Tests
// ============================================================================

#[test]
fn test_bulk_approval_shift_a_opens_confirmation_modal() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        let task = sample_task(Uuid::new_v4(), proj_id, &format!("TASK-{i}"), &format!("Task {i}"), TaskStatus::HumanReview);
        state.review_tasks.push(coordinator::tui::state::ReviewTaskState {
            task,
            suggested_agent_name: None,
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        });
    }

    assert!(state.pending_confirmation.is_none());

    // Press Shift+A
    let action = state.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
    assert_eq!(action, None);

    // Confirmation modal should be active for 3 tasks
    assert!(state.pending_confirmation.is_some());
    let modal = state.pending_confirmation.as_ref().unwrap();
    assert_eq!(modal.title, "Confirm Bulk Task Approval");
    assert!(modal.message.contains("3 pending tasks"));
    assert_eq!(modal.action, DestructiveAction::ApproveAllTasks);

    // Confirm with 'y' -> dispatches Some(ApproveAllTasks)
    let action = state.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(action, Some(TuiAction::ApproveAllTasks));
    assert!(state.pending_confirmation.is_none());

    // All review tasks should now have human_decision = Some(Approved)
    for item in &state.review_tasks {
        assert_eq!(item.human_decision, Some(ApprovalStatus::Approved));
        assert_eq!(item.task.status, TaskStatus::Approved);
    }
}

#[test]
fn test_bulk_approval_via_command_palette() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=2 {
        let task = sample_task(Uuid::new_v4(), proj_id, &format!("TASK-{i}"), &format!("Task {i}"), TaskStatus::HumanReview);
        state.review_tasks.push(coordinator::tui::state::ReviewTaskState {
            task,
            suggested_agent_name: None,
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        });
    }

    // Open palette ':'
    state.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
    assert!(state.show_command_palette);

    // Type "approve_all"
    state.command_palette_query = "approve_all".to_string();

    // Press Enter to execute command
    let action = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(action, None);
    assert!(!state.show_command_palette);

    // Safety modal is prompted
    assert!(state.pending_confirmation.is_some());
}

#[test]
fn test_bulk_approval_empty_tasks_shows_toast_message() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.review_tasks.clear();

    // Press Shift+A with 0 tasks
    state.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
    assert!(state.pending_confirmation.is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("No pending tasks to approve.")
    );
}

// ============================================================================
// 5. Visual Hierarchy & Toast Severity Rendering Tests
// ============================================================================

#[test]
fn test_footer_renders_severity_icons_and_toast() {
    let mut terminal = create_terminal(120, 3);
    let mut state = AppState::new();

    // 1. Success toast
    state.status_message = Some("Approved all 5 tasks.".to_string());
    terminal
        .draw(|f| FooterWidget::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "✔");
    assert_buffer_contains(&terminal, "Approved all 5 tasks.");

    // 2. Error toast
    state.status_message = Some("Task PLAN-1 FAILED: segmentation fault".to_string());
    terminal
        .draw(|f| FooterWidget::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "✖");
    assert_buffer_contains(&terminal, "FAILED");

    // 3. Warning toast
    state.status_message = Some("Task PLAN-2 is BLOCKED: waiting on dep".to_string());
    terminal
        .draw(|f| FooterWidget::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "⚠");
    assert_buffer_contains(&terminal, "BLOCKED");

    // 4. Info toast
    state.status_message = Some("Ready. One-human operator mode active.".to_string());
    terminal
        .draw(|f| FooterWidget::render(f, f.area(), &state))
        .unwrap();
    assert_buffer_contains(&terminal, "ℹ");
    assert_buffer_contains(&terminal, "Ready.");
}

#[test]
fn test_dashboard_renders_unicode_progress_bar_and_status_icons() {
    let mut terminal = create_terminal(120, 35);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Dashboard;

    let proj_id = Uuid::new_v4();
    let task1 = sample_task(Uuid::new_v4(), proj_id, "PLAN-1", "Setup Database", TaskStatus::Completed);
    let task2 = sample_task(Uuid::new_v4(), proj_id, "PLAN-2", "Worker Engine", TaskStatus::Executing);

    state.review_tasks.push(coordinator::tui::state::ReviewTaskState {
        task: task1,
        suggested_agent_name: Some("RustSpecialist".to_string()),
        overlap_warnings: Vec::new(),
        dependencies: Vec::new(),
        human_decision: Some(ApprovalStatus::Approved),
        progress_percent: Some(100),
    });

    state.review_tasks.push(coordinator::tui::state::ReviewTaskState {
        task: task2,
        suggested_agent_name: Some("GoSpecialist".to_string()),
        overlap_warnings: Vec::new(),
        dependencies: Vec::new(),
        human_decision: Some(ApprovalStatus::Approved),
        progress_percent: Some(60),
    });

    terminal
        .draw(|f| DashboardScreen::render(f, f.area(), &state))
        .unwrap();

    // Unicode block progress bars
    assert_buffer_contains(&terminal, "[█████] 100%");
    assert_buffer_contains(&terminal, "[███░░] 60%");

    // Status badges with icons
    assert_buffer_contains(&terminal, "✔ Completed");
    assert_buffer_contains(&terminal, "▶ Executing");
}

#[test]
fn test_plan_review_empty_state_renders_onboarding_guide() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.review_tasks.clear();

    terminal
        .draw(|f| PlanReviewScreen::render(f, f.area(), &state))
        .unwrap();

    assert_buffer_contains(&terminal, "No tasks proposed yet. Go to [2: Proj]");
    assert_buffer_contains(&terminal, "Quick Start Guide:");
    assert_buffer_contains(&terminal, "1. Press [2]");
    assert_buffer_contains(&terminal, "2. Press [I]");
}
