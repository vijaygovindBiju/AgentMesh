use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use coordinator::domain::{
    ApprovalStatus, DependencyKind, Project, ProjectStatus, Task, TaskDependency, TaskStatus,
};
use coordinator::tui::state::{DashboardFocus, DiagnosticsFocus};
use coordinator::tui::{
    render, AppState, CurrentScreen, InputMode, ReviewTaskState, TuiAction,
};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to create TestBackend terminal")
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

fn make_task(id: Uuid, proj_id: Uuid, short_id: &str, title: &str, status: TaskStatus) -> Task {
    Task {
        id,
        project_id: proj_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["crates/coordinator"]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ============================================================================
// STRESS SUITE 1: RAPID KEY EVENT FLOODS & DEEP NAVIGATION CYCLING
// ============================================================================

#[test]
fn test_stress_rapid_key_flood_no_panics() {
    let mut state = AppState::new().with_mock_data();
    let mut terminal = create_terminal(100, 30);

    // Flood with 5,000 diverse rapid key events across all modes
    let keys = [
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Char('1'),
        KeyCode::Char('2'),
        KeyCode::Char('3'),
        KeyCode::Char('4'),
        KeyCode::Char('g'),
        KeyCode::Char('y'),
        KeyCode::Char('n'),
        KeyCode::Char('e'),
        KeyCode::Char('c'),
        KeyCode::Char('a'),
        KeyCode::Char('r'),
        KeyCode::Char('?'),
        KeyCode::Char('x'),
        KeyCode::Char('z'),
        KeyCode::Char(' '),
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::F(1),
        KeyCode::F(5),
    ];

    for i in 0..5000 {
        let key = keys[i % keys.len()];
        let _ = state.handle_key(press(key));

        // Intermittently render every 100 keystrokes to ensure view rendering doesn't crash
        if i % 100 == 0 {
            terminal.draw(|f| render(f, &state)).expect("Render failed during key flood");
        }
    }
}

#[test]
fn test_stress_deep_screen_cycling() {
    let mut state = AppState::new().with_mock_data();

    // 2,000 Tab screen switches
    for _ in 0..2000 {
        let prev = state.current_screen;
        state.handle_key(press(KeyCode::Tab));
        match prev {
            CurrentScreen::ProjectInput => assert_eq!(state.current_screen, CurrentScreen::PlanReview),
            CurrentScreen::PlanReview => assert_eq!(state.current_screen, CurrentScreen::Dashboard),
            CurrentScreen::Dashboard => assert_eq!(state.current_screen, CurrentScreen::Diagnostics),
            CurrentScreen::Diagnostics => assert_eq!(state.current_screen, CurrentScreen::ProjectInput),
        }
    }

    // 2,000 numeric screen switches
    let targets = [
        (KeyCode::Char('1'), CurrentScreen::ProjectInput),
        (KeyCode::Char('2'), CurrentScreen::PlanReview),
        (KeyCode::Char('3'), CurrentScreen::Dashboard),
        (KeyCode::Char('4'), CurrentScreen::Diagnostics),
    ];

    for i in 0..2000 {
        let (key, expected) = targets[i % 4];
        state.handle_key(press(key));
        assert_eq!(state.current_screen, expected);
    }
}

#[test]
fn test_stress_modal_thrashing() {
    let mut state = AppState::new().with_mock_data();
    let mut terminal = create_terminal(100, 30);

    // Rapidly toggle Help Modal (? then Esc) 500 times
    for _ in 0..500 {
        state.handle_key(press(KeyCode::Char('?')));
        assert!(state.show_help_modal);
        terminal.draw(|f| render(f, &state)).unwrap();

        state.handle_key(press(KeyCode::Esc));
        assert!(!state.show_help_modal);
    }

    // Switch to Dashboard and rapidly toggle Agent Inspector (c then Esc) 500 times
    state.current_screen = CurrentScreen::Dashboard;
    state.dashboard_focus = DashboardFocus::AgentsTable;
    for _ in 0..500 {
        state.handle_key(press(KeyCode::Char('c')));
        assert!(state.show_agent_inspector);
        terminal.draw(|f| render(f, &state)).unwrap();

        state.handle_key(press(KeyCode::Esc));
        assert!(!state.show_agent_inspector);
    }

    // Switch to Diagnostics and rapidly toggle Event Payload modal (Enter then Esc) 500 times
    state.current_screen = CurrentScreen::Diagnostics;
    state.diagnostics_focus = DiagnosticsFocus::EventsStream;
    for _ in 0..500 {
        state.handle_key(press(KeyCode::Enter));
        assert!(state.show_event_payload_modal);
        terminal.draw(|f| render(f, &state)).unwrap();

        state.handle_key(press(KeyCode::Esc));
        assert!(!state.show_event_payload_modal);
    }

    // Switch focus to Timeline and toggle Timeline modal (Enter then Esc) 500 times
    state.diagnostics_focus = DiagnosticsFocus::Timeline;
    for _ in 0..500 {
        state.handle_key(press(KeyCode::Enter));
        assert!(state.show_timeline_modal);
        terminal.draw(|f| render(f, &state)).unwrap();

        state.handle_key(press(KeyCode::Esc));
        assert!(!state.show_timeline_modal);
    }
}

#[test]
fn test_stress_plan_review_navigation_bounds() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;

    let task_count = state.review_tasks.len();
    assert!(task_count > 0);

    // Press Down 1,000 times
    for _ in 0..1000 {
        state.handle_key(press(KeyCode::Down));
    }
    assert_eq!(state.selected_task_index, task_count - 1);

    // Press Up 1,000 times
    for _ in 0..1000 {
        state.handle_key(press(KeyCode::Up));
    }
    assert_eq!(state.selected_task_index, 0);

    // Rapid toggle details pane and DAG view
    for _ in 0..500 {
        state.handle_key(press(KeyCode::Enter));
        state.handle_key(press(KeyCode::Char('g')));
    }
}

#[test]
fn test_stress_dashboard_pane_switching_and_acknowledgment() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::Dashboard;

    // Rapidly switch focus left/right 1,000 times
    for _ in 0..1000 {
        state.handle_key(press(KeyCode::Right));
        state.handle_key(press(KeyCode::Left));
    }

    // Focus Overlaps table and acknowledge all overlaps
    state.dashboard_focus = DashboardFocus::OverlapsTable;
    let initial_overlaps = state.active_overlaps.len();
    for _ in 0..initial_overlaps {
        let action = state.handle_key(press(KeyCode::Char('a')));
        assert!(matches!(action, Some(TuiAction::AcknowledgeOverlap { .. })));
    }
    assert!(state.active_overlaps.is_empty());

    // Pressing 'a' when empty must return None safely and not panic
    for _ in 0..50 {
        let action = state.handle_key(press(KeyCode::Char('a')));
        assert_eq!(action, None);
    }
}

#[test]
fn test_stress_shrink_tasks_list_triggers_direct_index_panic_on_approve() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;

    // Navigate to task index 2
    state.handle_key(press(KeyCode::Down));
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 2);

    // Background update replaces tasks with only 1 task
    let single_task = state.review_tasks[0].clone();
    state.apply_update(coordinator::tui::TuiUpdateEvent::Tasks(vec![single_task]));

    // selected_task_index is safely clamped to 0 because review_tasks.len() is 1
    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.review_tasks.len(), 1);

    // Approves cleanly without panicking due to bounds clamping and safe indexing
    let action = state.handle_key(press(KeyCode::Char('y')));
    assert!(action.is_some());
}

// ============================================================================
// STRESS SUITE 2: LARGE TASK COUNTS, CYCLIC & WIDE DEPENDENCY GRAPHS
// ============================================================================

#[test]
fn test_stress_large_task_count_100_tasks_navigation_and_review() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    state.active_project = Some(Project {
        id: proj_id,
        name: "Massive Task Plan".to_string(),
        description: "Stress test with 100 tasks".to_string(),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    // Populate 100 tasks
    let mut tasks = Vec::new();
    for i in 0..100 {
        let tid = Uuid::new_v4();
        let short_id = format!("PLAN-{}", i + 1);
        let title = format!("Task {} automated migration unit", i + 1);
        let mut r = ReviewTaskState::new(make_task(tid, proj_id, &short_id, &title, TaskStatus::HumanReview));
        r.suggested_agent_name = Some(format!("agent-{}", (i % 5) + 1));
        r.progress_percent = Some((i % 100) as u8);
        tasks.push(r);
    }
    state.review_tasks = tasks;
    state.current_screen = CurrentScreen::PlanReview;

    let mut terminal = create_terminal(120, 35);

    // Render initially
    terminal.draw(|f| render(f, &state)).expect("Render failed with 100 tasks");

    // Scroll through all 100 tasks from top to bottom
    for i in 0..99 {
        assert_eq!(state.selected_task_index, i);
        state.handle_key(press(KeyCode::Down));
    }
    assert_eq!(state.selected_task_index, 99);

    // Verify terminal renders at bottom without panic
    terminal.draw(|f| render(f, &state)).expect("Render failed at task 99");

    // Approve 50 tasks with 'y' and reject 49 tasks with 'n'
    state.selected_task_index = 0;
    for i in 0..50 {
        let action = state.handle_key(press(KeyCode::Char('y')));
        assert!(matches!(action, Some(TuiAction::ApproveTask { .. })));
        assert_eq!(state.review_tasks[i].human_decision, Some(ApprovalStatus::Approved));
    }

    for i in 50..99 {
        let action = state.handle_key(press(KeyCode::Char('n')));
        assert!(matches!(action, Some(TuiAction::RejectTask { .. })));
        assert_eq!(state.review_tasks[i].human_decision, Some(ApprovalStatus::Rejected));
    }

    // Scroll back up to 0
    for _ in 0..150 {
        state.handle_key(press(KeyCode::Up));
    }
    assert_eq!(state.selected_task_index, 0);
}

#[test]
fn test_stress_cyclic_dependency_graph_in_dag_view() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();

    // Create a 3-task circular dependency: A -> B -> C -> A
    let mut task_a = ReviewTaskState::new(make_task(id_a, proj_id, "CYC-A", "Component A", TaskStatus::HumanReview));
    task_a.dependencies.push(TaskDependency {
        dependent_id: id_a,
        depends_on_id: id_c,
        kind: DependencyKind::Blocks,
    });

    let mut task_b = ReviewTaskState::new(make_task(id_b, proj_id, "CYC-B", "Component B", TaskStatus::HumanReview));
    task_b.dependencies.push(TaskDependency {
        dependent_id: id_b,
        depends_on_id: id_a,
        kind: DependencyKind::Blocks,
    });

    let mut task_c = ReviewTaskState::new(make_task(id_c, proj_id, "CYC-C", "Component C", TaskStatus::HumanReview));
    task_c.dependencies.push(TaskDependency {
        dependent_id: id_c,
        depends_on_id: id_b,
        kind: DependencyKind::Blocks,
    });

    state.review_tasks = vec![task_a, task_b, task_c];
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;

    let mut terminal = create_terminal(120, 30);

    // Must render cleanly without infinite loop or panic
    terminal.draw(|f| render(f, &state)).expect("DAG render panicked on circular dependency!");

    let rendered = buffer_to_string(&terminal);
    assert!(rendered.contains("0 Root(s) Ready"), "Cyclic graph must report 0 ready roots");
    assert!(rendered.contains("3 Dependent"), "All 3 tasks in cycle must be classified as dependents");
    assert!(rendered.contains("blocks"), "Blocking edges must be rendered");

    // Also verify full-width DAG view (details pane toggled off)
    state.show_details_pane = false;
    terminal.draw(|f| render(f, &state)).expect("Full-width DAG render failed");
    let full_rendered = buffer_to_string(&terminal);
    assert!(full_rendered.contains("3 Dependent(s)"), "Full width DAG contains full label");
}

#[test]
fn test_stress_self_loop_dependency_in_dag_view() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let id_self = Uuid::new_v4();

    // Task that depends on itself: Self -> Self
    let mut task_self = ReviewTaskState::new(make_task(id_self, proj_id, "SELF-1", "Self Referencing", TaskStatus::HumanReview));
    task_self.dependencies.push(TaskDependency {
        dependent_id: id_self,
        depends_on_id: id_self,
        kind: DependencyKind::Blocks,
    });

    state.review_tasks = vec![task_self];
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;

    let mut terminal = create_terminal(100, 25);
    terminal.draw(|f| render(f, &state)).expect("DAG render panicked on self-loop dependency");

    let rendered = buffer_to_string(&terminal);
    assert!(rendered.contains("1 Tasks"));
    assert!(rendered.contains("0 Root(s) Ready"));
}

#[test]
fn test_stress_wide_dependency_graph_fan_out_and_fan_in() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    let root_id = Uuid::new_v4();
    let root_task = ReviewTaskState::new(make_task(root_id, proj_id, "ROOT-0", "Master Root Task", TaskStatus::HumanReview));

    let sink_id = Uuid::new_v4();
    let mut sink_task = ReviewTaskState::new(make_task(sink_id, proj_id, "SINK-99", "Master Sink Task", TaskStatus::HumanReview));

    let mut tasks = vec![root_task];

    // Fan-out: 50 intermediate tasks all depend on root_id
    // Fan-in: sink_task depends on all 50 intermediate tasks
    for i in 1..=50 {
        let mid_id = Uuid::new_v4();
        let mut mid_task = ReviewTaskState::new(make_task(mid_id, proj_id, &format!("MID-{}", i), "Parallel Worker", TaskStatus::HumanReview));
        mid_task.dependencies.push(TaskDependency {
            dependent_id: mid_id,
            depends_on_id: root_id,
            kind: DependencyKind::Blocks,
        });
        sink_task.dependencies.push(TaskDependency {
            dependent_id: sink_id,
            depends_on_id: mid_id,
            kind: DependencyKind::Blocks,
        });
        tasks.push(mid_task);
    }
    tasks.push(sink_task);

    state.review_tasks = tasks;
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;

    let mut terminal = create_terminal(140, 50);
    terminal.draw(|f| render(f, &state)).expect("DAG render panicked on 50-wide fan-out/fan-in graph");

    let rendered = buffer_to_string(&terminal);
    assert!(rendered.contains("52 Tasks"));
    assert!(rendered.contains("1 Root(s) Ready"));
    assert!(rendered.contains("51 Dependent(s)"));
}

#[test]
fn test_stress_orphaned_external_dependency_ids() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    let task_id = Uuid::new_v4();
    let external_dep_id = Uuid::new_v4(); // Does NOT exist in review_tasks

    let mut task = ReviewTaskState::new(make_task(task_id, proj_id, "ORPH-1", "Orphaned Dep Task", TaskStatus::HumanReview));
    task.dependencies.push(TaskDependency {
        dependent_id: task_id,
        depends_on_id: external_dep_id,
        kind: DependencyKind::Blocks,
    });

    state.review_tasks = vec![task];
    state.current_screen = CurrentScreen::PlanReview;

    let mut terminal = create_terminal(120, 30);

    // 1. Task cards + details pane inspector (must render 'External Prerequisite')
    terminal.draw(|f| render(f, &state)).expect("Render panicked on external dep in details pane");
    let content = buffer_to_string(&terminal);
    assert!(content.contains("External Prerequisite"), "Details pane must display 'External Prerequisite'");

    // 2. DAG view (must not panic when mapping non-existent dep UUID)
    state.show_dag_view = true;
    terminal.draw(|f| render(f, &state)).expect("DAG view panicked on external dep UUID");
}

// ============================================================================
// STRESS SUITE 3: EXTREME TERMINAL BUFFER SIZES
// ============================================================================

#[test]
fn test_stress_terminal_buffer_small_40x15() {
    let mut terminal = create_terminal(40, 15);
    let mut state = AppState::new().with_mock_data();

    let screens = [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ];

    // Render every screen at 40x15
    for screen in screens {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).unwrap_or_else(|e| panic!("Failed rendering {:?} at 40x15: {:?}", screen, e));
    }

    // Render PlanReview with details pane toggled off and on
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = false;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_details_pane = true;
    terminal.draw(|f| render(f, &state)).unwrap();

    // Render PlanReview in DAG view at 40x15
    state.show_dag_view = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_dag_view = false;

    // Render inline edit mode at 40x15
    state.input_mode = InputMode::EditingDescription;
    state.edit_buffer = "Small terminal edit test".to_string();
    terminal.draw(|f| render(f, &state)).unwrap();
    state.input_mode = InputMode::Normal;

    // Render all 4 modals at 40x15
    state.show_help_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_help_modal = false;

    state.show_agent_inspector = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_agent_inspector = false;

    state.show_event_payload_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_event_payload_modal = false;

    state.show_timeline_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_timeline_modal = false;
}

#[test]
fn test_stress_terminal_buffer_ultra_wide_300x100() {
    let mut terminal = create_terminal(300, 100);
    let mut state = AppState::new().with_mock_data();

    let screens = [
        CurrentScreen::ProjectInput,
        CurrentScreen::PlanReview,
        CurrentScreen::Dashboard,
        CurrentScreen::Diagnostics,
    ];

    for screen in screens {
        state.current_screen = screen;
        terminal.draw(|f| render(f, &state)).unwrap_or_else(|e| panic!("Failed rendering {:?} at 300x100: {:?}", screen, e));
    }

    // Render DAG view at 300x100
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_dag_view = false;

    // Render modals at 300x100
    state.show_help_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_help_modal = false;

    state.show_agent_inspector = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_agent_inspector = false;

    state.show_event_payload_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_event_payload_modal = false;

    state.show_timeline_modal = true;
    terminal.draw(|f| render(f, &state)).unwrap();
    state.show_timeline_modal = false;
}

#[test]
fn test_stress_terminal_buffer_degenerate_dimensions() {
    let state = AppState::new().with_mock_data();

    // Test highly degenerate sizes to ensure no arithmetic overflow/underflow or division by zero
    let extreme_sizes = [
        (10, 5),
        (20, 8),
        (30, 10),
        (250, 15),
        (40, 200),
    ];

    for (w, h) in extreme_sizes {
        let mut terminal = create_terminal(w, h);
        terminal.draw(|f| render(f, &state)).unwrap_or_else(|e| panic!("Panicked on degenerate size {w}x{h}: {:?}", e));
    }
}

// ============================================================================
// STRESS SUITE 4: MULTI-BYTE UTF-8 CHARACTER INSERTION AND DELETION
// ============================================================================

#[test]
fn test_stress_utf8_inline_editor_japanese_cjk() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;

    // Enter edit mode
    state.handle_key(press(KeyCode::Char('e')));
    assert_eq!(state.input_mode, InputMode::EditingDescription);

    // Clear buffer with backspace
    while !state.edit_buffer.is_empty() {
        state.handle_key(press(KeyCode::Backspace));
    }
    assert_eq!(state.edit_buffer, "");
    assert_eq!(state.edit_cursor, 0);

    // Insert Japanese 3-byte characters: こんにちは世界 (Hello World)
    let japanese_text = "こんにちは世界";
    for c in japanese_text.chars() {
        state.handle_key(press(KeyCode::Char(c)));
        assert!(state.edit_buffer.is_char_boundary(state.edit_cursor), "Cursor must remain on UTF-8 char boundary!");
    }
    assert_eq!(state.edit_buffer, japanese_text);
    assert_eq!(state.edit_cursor, japanese_text.len());

    // Navigate Left across 3-byte characters
    let chars_count = japanese_text.chars().count();
    for _ in 0..chars_count {
        state.handle_key(press(KeyCode::Left));
        assert!(state.edit_buffer.is_char_boundary(state.edit_cursor), "Left navigation must stay on UTF-8 char boundary");
    }
    assert_eq!(state.edit_cursor, 0);

    // Navigate Right across 3-byte characters
    for _ in 0..chars_count {
        state.handle_key(press(KeyCode::Right));
        assert!(state.edit_buffer.is_char_boundary(state.edit_cursor), "Right navigation must stay on UTF-8 char boundary");
    }
    assert_eq!(state.edit_cursor, japanese_text.len());

    // Backspace 2 characters from end ('界', then '世')
    state.handle_key(press(KeyCode::Backspace));
    state.handle_key(press(KeyCode::Backspace));
    assert_eq!(state.edit_buffer, "こんにちは");
    assert_eq!(state.edit_cursor, "こんにちは".len());

    // Confirm edit with Enter
    let action = state.handle_key(press(KeyCode::Enter));
    assert_eq!(state.input_mode, InputMode::Normal);
    match action {
        Some(TuiAction::EditTaskDescription { new_description, .. }) => {
            assert_eq!(new_description, "こんにちは");
        }
        other => panic!("Expected EditTaskDescription, got {:?}", other),
    }
}

#[test]
fn test_stress_utf8_inline_editor_emojis_4bytes() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;

    state.handle_key(press(KeyCode::Char('e')));
    while !state.edit_buffer.is_empty() {
        state.handle_key(press(KeyCode::Backspace));
    }

    // 4-byte UTF-8 emojis
    let emojis = ['🦀', '🚀', '🔥', '💡', '💻'];
    for emoji in emojis {
        state.handle_key(press(KeyCode::Char(emoji)));
        assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));
    }
    assert_eq!(state.edit_buffer, "🦀🚀🔥💡💻");

    // Move left 3 times (over 💻, 💡, 🔥)
    state.handle_key(press(KeyCode::Left));
    state.handle_key(press(KeyCode::Left));
    state.handle_key(press(KeyCode::Left));
    assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));

    // Insert an ASCII character '!' and another emoji '⚡' in between '🚀' and '🔥'
    state.handle_key(press(KeyCode::Char('!')));
    state.handle_key(press(KeyCode::Char('⚡')));
    assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));

    assert_eq!(state.edit_buffer, "🦀🚀!⚡🔥💡💻");

    // Backspace the '⚡' and '!'
    state.handle_key(press(KeyCode::Backspace));
    assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));
    state.handle_key(press(KeyCode::Backspace));
    assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));

    assert_eq!(state.edit_buffer, "🦀🚀🔥💡💻");
}

#[test]
fn test_stress_utf8_inline_editor_mixed_alphabets_and_rendering() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;

    state.handle_key(press(KeyCode::Char('e')));
    while !state.edit_buffer.is_empty() {
        state.handle_key(press(KeyCode::Backspace));
    }

    // Mixed: Latin accented + Cyrillic + Arabic + Chinese + Math symbols + Emoji
    let complex_utf8 = "Café Привет مرحبا 世界 ∑(x) ≥ 0 🚀";
    for c in complex_utf8.chars() {
        state.handle_key(press(KeyCode::Char(c)));
        assert!(state.edit_buffer.is_char_boundary(state.edit_cursor));
    }
    assert_eq!(state.edit_buffer, complex_utf8);

    // Verify it renders in DetailsPaneWidget on screen without panics
    let mut terminal = create_terminal(120, 30);
    terminal.draw(|f| render(f, &state)).expect("Rendering failed with complex UTF-8 string");

    let rendered = buffer_to_string(&terminal);
    assert!(rendered.contains("Café"));
    assert!(rendered.contains("Привет"));
    assert!(rendered.contains("世") && rendered.contains("界"));
}

#[test]
fn test_stress_utf8_project_input_name_and_description() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::ProjectInput;

    // Enter project edit mode
    state.handle_key(press(KeyCode::Char('i')));
    assert_eq!(state.input_mode, InputMode::EnteringProject);
    assert_eq!(state.project_focus_field, 0);

    // Type multi-byte UTF-8 project name
    let name = "プロジェクト-Project-🚀";
    for c in name.chars() {
        state.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(state.project_name_input, name);

    // Backspace 2 chars ('🚀' then '-')
    state.handle_key(press(KeyCode::Backspace));
    state.handle_key(press(KeyCode::Backspace));
    assert_eq!(state.project_name_input, "プロジェクト-Project");

    // Switch to description field
    state.handle_key(press(KeyCode::Tab));
    assert_eq!(state.project_focus_field, 1);

    // Type multi-byte description
    let desc = "AgentMesh分散型オーケストレーション基盤 🛡️";
    for c in desc.chars() {
        state.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(state.project_desc_input, desc);

    // Submit project
    let action = state.handle_key(press(KeyCode::Enter));
    match action {
        Some(TuiAction::SubmitProject { name, description }) => {
            assert_eq!(name, "プロジェクト-Project");
            assert_eq!(description, desc);
        }
        other => panic!("Expected SubmitProject action, got {:?}", other),
    }
}

#[test]
fn test_stress_dag_view_vertical_scrolling_large_graph() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    // 52-task graph: 1 root -> 50 intermediate -> 1 sink
    let root_id = Uuid::new_v4();
    let sink_id = Uuid::new_v4();
    let mut tasks = Vec::new();

    tasks.push(ReviewTaskState::new(make_task(root_id, proj_id, "ROOT-0", "Root Task", TaskStatus::Approved)));
    let mut sink_task = ReviewTaskState::new(make_task(sink_id, proj_id, "SINK-99", "Final Sink Task", TaskStatus::HumanReview));

    for i in 1..=50 {
        let mid_id = Uuid::new_v4();
        let mut mid_task = ReviewTaskState::new(make_task(mid_id, proj_id, &format!("MID-{i}"), "Parallel Worker", TaskStatus::HumanReview));
        mid_task.dependencies.push(TaskDependency {
            dependent_id: mid_id,
            depends_on_id: root_id,
            kind: DependencyKind::Blocks,
        });
        sink_task.dependencies.push(TaskDependency {
            dependent_id: sink_id,
            depends_on_id: mid_id,
            kind: DependencyKind::Blocks,
        });
        tasks.push(mid_task);
    }
    tasks.push(sink_task);

    state.review_tasks = tasks;
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;

    // Terminal with height 20 (smaller than the total DAG lines which is > 100)
    let mut terminal = create_terminal(120, 20);

    // When root is selected (index 0), root task should be visible in buffer
    state.selected_task_index = 0;
    terminal.draw(|f| render(f, &state)).expect("Render failed at index 0");
    let content_top = buffer_to_string(&terminal);
    assert!(content_top.contains("ROOT-0"), "Root task must be visible when selected at top");

    // When the final sink task is selected (index 51), DAG should scroll vertically to show SINK-99
    state.selected_task_index = 51;
    terminal.draw(|f| render(f, &state)).expect("Render failed at index 51");
    let content_bottom = buffer_to_string(&terminal);
    assert!(content_bottom.contains("SINK-99"), "Final sink task must be scrolled into view when selected");
}

#[test]
fn test_stress_inline_editor_cursor_indicator_position() {
    let mut state = AppState::new().with_mock_data();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    // Press 'e' to enter editing
    state.handle_key(press(KeyCode::Char('e')));
    assert_eq!(state.input_mode, InputMode::EditingDescription);

    // Clear buffer
    while !state.edit_buffer.is_empty() {
        state.handle_key(press(KeyCode::Backspace));
    }

    // Type "Hello World"
    for c in "Hello World".chars() {
        state.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(state.edit_buffer, "Hello World");
    assert_eq!(state.edit_cursor, 11);

    // Terminal render: when at end, cursor '_' is at the end
    let mut terminal = create_terminal(120, 30);
    terminal.draw(|f| render(f, &state)).expect("Render failed at end of buffer");
    let rendered_end = buffer_to_string(&terminal);
    assert!(rendered_end.contains("Hello World_"));

    // Move Left 6 times (so cursor is between "Hello" and " World")
    for _ in 0..6 {
        state.handle_key(press(KeyCode::Left));
    }
    assert_eq!(state.edit_cursor, 5);

    // Terminal render: visual cursor '_' must be inserted at index 5: "Hello_ World"
    terminal.draw(|f| render(f, &state)).expect("Render failed with cursor in middle");
    let rendered_middle = buffer_to_string(&terminal);
    assert!(
        rendered_middle.contains("Hello_ World"),
        "Cursor '_' must be displayed at edit_cursor position, got: {rendered_middle}"
    );
}
