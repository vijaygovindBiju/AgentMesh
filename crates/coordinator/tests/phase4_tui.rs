use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use coordinator::domain::ApprovalStatus;
use coordinator::tui::{render, AppState, CurrentScreen, InputMode, TuiAction};

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_tui_human_review_keybindings_and_action_emission() {
    let mut state = AppState::new().with_mock_data();
    let mut emitted_actions = Vec::new();

    // Verify initial mock data setup
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

    let mut state = AppState::new().with_mock_data();

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
