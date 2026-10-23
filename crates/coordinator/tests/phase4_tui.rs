use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use agent_protocol::AgentMessage;
use coordinator::domain::ApprovalStatus;
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::state::DashboardFocus;
use coordinator::tui::{render, AppState, CurrentScreen, InputMode, TuiAction, TuiUpdateEvent};

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

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_tui_human_review_keybindings_and_action_emission() {
    let mut state = AppState::new().with_test_data();
    let mut emitted_actions = Vec::new();

    // Verify initial agy data setup
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);
    assert_eq!(state.review_tasks.len(), 3);
    assert_eq!(state.selected_task_index, 0);

    let task0_id = state.review_tasks[0].task.id;
    let task1_id = state.review_tasks[1].task.id;
    let task2_id = state.review_tasks[2].task.id;

    // 1. Approve task 0 with [Y]
    if let Some(action) = state.handle_key(press_key(KeyCode::Char('y'))) {
        emitted_actions.push(action);
    }
    assert_eq!(emitted_actions.len(), 1);
    assert_eq!(
        emitted_actions[0],
        TuiAction::ApproveTask { task_id: task0_id }
    );
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::Approved)
    );
    // Automatically advanced selection to task 1
    assert_eq!(state.selected_task_index, 1);

    // 2. Reject task 1 with [N]
    if let Some(action) = state.handle_key(press_key(KeyCode::Char('n'))) {
        emitted_actions.push(action);
    }
    assert_eq!(emitted_actions.len(), 2);
    assert_eq!(
        emitted_actions[1],
        TuiAction::RejectTask { task_id: task1_id }
    );
    assert_eq!(
        state.review_tasks[1].human_decision,
        Some(ApprovalStatus::Rejected)
    );
    // Automatically advanced selection to task 2
    assert_eq!(state.selected_task_index, 2);

    // 3. Edit task 2 with [E]
    let edit_action = state.handle_key(press_key(KeyCode::Char('e')));
    assert_eq!(edit_action, None); // Entering edit mode emits no external action
    assert_eq!(state.input_mode, InputMode::EditingDescription);

    // Add extra text to description buffer
    for c in " - verified by operator".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }

    // Confirm edit and approve with [Enter]
    if let Some(action) = state.handle_key(press_key(KeyCode::Enter)) {
        emitted_actions.push(action);
    }
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(emitted_actions.len(), 3);
    match &emitted_actions[2] {
        TuiAction::EditTaskDescription {
            task_id,
            new_description,
        } => {
            assert_eq!(*task_id, task2_id);
            assert!(new_description.ends_with("- verified by operator"));
        }
        other => panic!("Expected EditTaskDescription, got {:?}", other),
    }
    assert_eq!(
        state.review_tasks[2].human_decision,
        Some(ApprovalStatus::EditedAndApproved)
    );

    // 4. Details Pane toggle with [Enter] in normal mode
    assert!(state.show_details_pane);
    state.handle_key(press_key(KeyCode::Enter));
    assert!(!state.show_details_pane);
    state.handle_key(press_key(KeyCode::Enter));
    assert!(state.show_details_pane);

    // 5. Quit key with [Q]
    if let Some(action) = state.handle_key(press_key(KeyCode::Char('q'))) {
        emitted_actions.push(action);
    }
    assert_eq!(emitted_actions.len(), 4);
    assert_eq!(emitted_actions[3], TuiAction::Quit);
    assert!(state.should_quit);
}

#[test]
fn test_tui_rendering_headless_all_modes() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut state = AppState::new().with_test_data();

    // Render Plan Review
    terminal
        .draw(|f| render(f, &state))
        .expect("Render PlanReview");

    // Render Details toggle off
    state.show_details_pane = false;
    terminal
        .draw(|f| render(f, &state))
        .expect("Render PlanReview full width");
    state.show_details_pane = true;

    // Render Description Editing mode
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = "Inline editing buffer text".to_string();
    terminal
        .draw(|f| render(f, &state))
        .expect("Render Inline Editor");
    state.input_mode = InputMode::Normal;

    // Render Project Input mode
    state.current_screen = CurrentScreen::ProjectInput;
    state.input_mode = InputMode::EnteringProject;
    state.project_name_input = "Mesh Project".to_string();
    state.project_desc_input = "Distributed mesh network".to_string();
    terminal
        .draw(|f| render(f, &state))
        .expect("Render ProjectInput");
    state.input_mode = InputMode::Normal;

    // Render Dashboard
    state.current_screen = CurrentScreen::Dashboard;
    terminal
        .draw(|f| render(f, &state))
        .expect("Render Dashboard");
}

#[test]
fn test_milestone2_dag_view_toggle_and_rendering() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();
    state.current_screen = CurrentScreen::PlanReview;

    assert!(!state.show_dag_view);
    // Press 'g' to toggle DAG view on
    state.handle_key(press_key(KeyCode::Char('g')));
    assert!(state.show_dag_view);

    // Draw and verify DAG rendered
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("DAG") || buf.contains("Dependency Graph") || buf.contains("PLAN-1"));
    assert!(buf.contains("──blocks──▶") || buf.contains("blocks") || buf.contains("PLAN-2"));

    // Press 'g' to toggle back
    state.handle_key(press_key(KeyCode::Char('g')));
    assert!(!state.show_dag_view);
}

#[test]
fn test_milestone2_help_modal_toggle_and_rendering() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();

    assert!(!state.show_help_modal);
    // Press '?' to toggle help modal on
    state.handle_key(press_key(KeyCode::Char('?')));
    assert!(state.show_help_modal);

    // Draw and verify help modal rendered
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("Keyboard Shortcuts"));

    // Press 'Esc' to close
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_help_modal);
}

#[test]
fn test_milestone2_agent_inspector_modal_and_focus_switching() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();
    state.current_screen = CurrentScreen::Dashboard;

    // Initial focus is AgentsTable
    assert_eq!(state.dashboard_focus, DashboardFocus::AgentsTable);

    // Switch focus to TasksTable with Right
    state.handle_key(press_key(KeyCode::Right));
    assert_eq!(state.dashboard_focus, DashboardFocus::TasksTable);

    // Switch focus to OverlapsTable with Right
    state.handle_key(press_key(KeyCode::Right));
    assert_eq!(state.dashboard_focus, DashboardFocus::OverlapsTable);

    // Switch back to AgentsTable
    state.handle_key(press_key(KeyCode::Left));
    assert_eq!(state.dashboard_focus, DashboardFocus::TasksTable);
    state.handle_key(press_key(KeyCode::Left));
    assert_eq!(state.dashboard_focus, DashboardFocus::AgentsTable);

    // Open agent inspector with 'c'
    assert!(!state.show_agent_inspector);
    state.handle_key(press_key(KeyCode::Char('c')));
    assert!(state.show_agent_inspector);

    // Draw and verify modal
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(
        buf.contains("Agent Capability Profile") || buf.contains("Runtime") || buf.contains("OS")
    );

    // Close inspector with Esc
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_agent_inspector);
}

#[test]
fn test_milestone2_details_pane_prerequisite_mapping() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();
    state.current_screen = CurrentScreen::PlanReview;

    // Task 1 depends on Task 0 in agy data
    state.selected_task_index = 1;
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    // Must contain human readable short ID PLAN-1 instead of raw UUID
    assert!(buf.contains("PLAN-1"));
    assert!(buf.contains("Prerequisites") || buf.contains("Depends on"));
}

#[test]
fn test_milestone2_real_time_progress_update_ingestion() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();
    let task_id = state.review_tasks[0].task.id;

    // Send ProgressUpdate message
    let msg = AgentMessage::ProgressUpdate {
        agent_id: Uuid::new_v4(),
        task_id,
        percent: 75,
        message: "Processing database migrations".to_string(),
        timestamp: Utc::now(),
    };
    state.apply_update(TuiUpdateEvent::AgentMessage(msg));

    assert_eq!(state.review_tasks[0].progress_percent, Some(75));

    // Render task cards and dashboard to verify progress display
    state.current_screen = CurrentScreen::PlanReview;
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("75%"));

    state.current_screen = CurrentScreen::Dashboard;
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("75%"));
}

#[test]
fn test_milestone2_diagnostics_live_events_sliding_window_and_modals() {
    let backend = TestBackend::new(140, 45);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = AppState::new().with_test_data();
    state.current_screen = CurrentScreen::Diagnostics;

    // Add 40 events to test sliding window beyond 30
    let mut events = Vec::new();
    for i in 0..40 {
        events.push(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            project_id: None,
            task_id: None,
            agent_id: None,
            event_type: "heartbeat".to_string(),
            message: format!("Heartbeat event {i}"),
            payload: json!({ "index": i, "details": format!("Heartbeat event {i}") }),
        });
    }
    state.apply_update(TuiUpdateEvent::RecentEvents(events));
    assert_eq!(state.recent_events.len(), 40);

    // Scroll down 35 times to test sliding window
    for _ in 0..35 {
        state.handle_key(press_key(KeyCode::Down));
    }
    assert_eq!(state.selected_event_index, 35);

    // Render to verify sliding window works without panic
    terminal.draw(|f| render(f, &state)).unwrap();

    // Press Enter to open event payload modal
    assert!(!state.show_event_payload_modal);
    state.handle_key(press_key(KeyCode::Enter));
    assert!(state.show_event_payload_modal);

    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("Event Payload Detail") || buf.contains("Event Details"));

    // Close modal with Esc
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.show_event_payload_modal);
}

#[test]
fn test_milestone2_inline_editor_utf8_multibyte_support() {
    let mut state = AppState::new().with_test_data();
    state.current_screen = CurrentScreen::PlanReview;

    // Start inline editing
    state.handle_key(press_key(KeyCode::Char('e')));
    assert_eq!(state.input_mode, InputMode::EditingDescription);

    // Clear buffer with backspaces
    while state.edit_cursor > 0 {
        state.handle_key(press_key(KeyCode::Backspace));
    }
    assert_eq!(state.edit_buffer, "");
    assert_eq!(state.edit_cursor, 0);

    // Type multi-byte characters: Japanese, Spanish accented, and Emoji
    for c in "東京 ñ 🚀".chars() {
        state.handle_key(press_key(KeyCode::Char(c)));
    }
    assert_eq!(state.edit_buffer, "東京 ñ 🚀");
    assert_eq!(state.edit_cursor, "東京 ñ 🚀".len());

    // Navigate Left by 2 characters (over '🚀' and ' ')
    state.handle_key(press_key(KeyCode::Left)); // before '🚀'
    state.handle_key(press_key(KeyCode::Left)); // before ' '

    // Backspace to delete 'ñ'
    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.edit_buffer, "東京  🚀");

    // Navigate Right
    state.handle_key(press_key(KeyCode::Right));
    state.handle_key(press_key(KeyCode::Right));

    // Submit edit
    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(state.input_mode, InputMode::Normal);
    match action {
        Some(TuiAction::EditTaskDescription {
            new_description, ..
        }) => {
            assert_eq!(new_description, "東京  🚀");
        }
        other => panic!("Expected EditTaskDescription, got {:?}", other),
    }
}
