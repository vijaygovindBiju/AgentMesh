//! Phase 22 Plan Review Ergonomics, Responsive Layout & Usability Test Suite
//!
//! Comprehensive 4-tier verification of the Plan Review screen, task list,
//! content-adaptive task inspector, keyboard navigation, and responsive
//! terminal resizing across compact (<90 cols), standard (120 cols), and
//! wide (~180-200 cols) profiles using Ratatui's `TestBackend`.

use chrono::Utc;
use coordinator::domain::{
    ApprovalStatus, DependencyKind, OverlapSeverity, OverlapWarning, Task, TaskDependency,
    TaskStatus,
};
use coordinator::tui::screens::PlanReviewScreen;
use coordinator::tui::state::{
    AppState, CurrentScreen, DestructiveAction, InputMode, PlanReviewFocus, ReviewTaskState,
    TuiAction,
};
use coordinator::tui::ui;
use coordinator::tui::widgets::{DagViewWidget, DetailsPaneWidget, FooterWidget, TaskCardWidget};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

// ============================================================================
// Test Harness & Helper Architecture
// ============================================================================

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn assert_buffer_contains(terminal: &Terminal<TestBackend>, needle: &str) {
    let rendered = buffer_to_string(terminal);
    assert!(
        rendered.contains(needle),
        "Buffer did not contain expected text '{needle}'. Rendered buffer:\n{rendered}"
    );
}

fn assert_buffer_does_not_contain(terminal: &Terminal<TestBackend>, needle: &str) {
    let rendered = buffer_to_string(terminal);
    assert!(
        !rendered.contains(needle),
        "Buffer unexpectedly contained '{needle}'. Rendered buffer:\n{rendered}"
    );
}

fn sample_task(
    id: Uuid,
    project_id: Uuid,
    short_id: &str,
    title: &str,
    status: TaskStatus,
) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["crates/coordinator/src/main.rs", "crates/coordinator/src/tui"]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn sample_review_task(
    id: Uuid,
    project_id: Uuid,
    short_id: &str,
    title: &str,
    status: TaskStatus,
) -> ReviewTaskState {
    ReviewTaskState {
        task: sample_task(id, project_id, short_id, title, status),
        suggested_agent_name: Some("RealAgent-1-Backend".to_string()),
        overlap_warnings: Vec::new(),
        dependencies: Vec::new(),
        human_decision: None,
        progress_percent: None,
    }
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press_shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn press_ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn mouse_scroll(column: u16, row: u16, kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

// ============================================================================
// Tier 1: Feature Coverage (Primary Happy Paths)
// ============================================================================

#[test]
fn test_tier1_responsive_plan_layout_compact_80x24() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("PLAN-{i}"),
            &format!("Compact Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    let mut terminal = create_terminal(80, 24);
    terminal
        .draw(|frame| {
            ui::render(frame, &state);
        })
        .expect("Terminal draw failed at 80x24");

    // Header tabs and Plan indicator visible in compact mode
    assert_buffer_contains(&terminal, "3:Plan");

    // Left column: Proposed tasks card list
    assert_buffer_contains(&terminal, "[PLAN-1]");

    // Right column: Task Details Inspector
    assert_buffer_contains(&terminal, "Task Details");

    // Footer actions visible
    assert_buffer_contains(&terminal, "Select All");
}

#[test]
fn test_tier1_responsive_plan_layout_standard_120x30() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=4 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("PLAN-{i}"),
            &format!("Standard Screen Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            ui::render(frame, &state);
        })
        .expect("Terminal draw failed at 120x30");

    // Outer titles for both columns rendered cleanly
    assert_buffer_contains(&terminal, "Proposed Task Decomposition (Human Review)");
    assert_buffer_contains(&terminal, "Task Details & Inspector");

    // Task items visible
    assert_buffer_contains(&terminal, "[PLAN-1]");
    assert_buffer_contains(&terminal, "Standard Screen Task #1");

    // Metadata lines
    assert_buffer_contains(&terminal, "RealAgent-1-Backend");
}

#[test]
fn test_tier1_responsive_plan_layout_wide_180x50() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=5 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("PLAN-{i}"),
            &format!("Ultra-wide Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    let mut terminal = create_terminal(180, 50);
    terminal
        .draw(|frame| {
            ui::render(frame, &state);
        })
        .expect("Terminal draw failed at 180x50");

    // Unclipped outer headers and footers
    assert_buffer_contains(&terminal, "Proposed Task Decomposition (Human Review)");
    assert_buffer_contains(&terminal, "Task Details & Inspector");
    assert_buffer_contains(&terminal, "[PLAN-5]");
}

#[test]
fn test_tier1_inspector_sections_rendering() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();

    let mut item = sample_review_task(
        task_id,
        proj_id,
        "INSPECT-1",
        "Inspectable Architecture Task",
        TaskStatus::HumanReview,
    );
    item.task.description = "Primary task description outlining implementation steps.".to_string();
    item.dependencies.push(TaskDependency {
        dependent_id: task_id,
        depends_on_id: dep_id,
        kind: DependencyKind::Blocks,
    });
    item.overlap_warnings.push(OverlapWarning {
        id: Uuid::new_v4(),
        project_id: proj_id,
        task_ids: json!([task_id, dep_id]),
        resource: "crates/coordinator/src/tui/screens/plan_review.rs".to_string(),
        severity: OverlapSeverity::Warning,
        acknowledged: false,
        created_at: Utc::now(),
    });

    state.review_tasks.push(item);

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            DetailsPaneWidget::render(frame, frame.area(), &state);
        })
        .expect("DetailsPaneWidget draw failed");

    // Verify all 5 inspector sections are present
    assert_buffer_contains(&terminal, "Task ID: [INSPECT-1]");
    assert_buffer_contains(&terminal, "Suggested Agent:");
    assert_buffer_contains(&terminal, "Description");
    assert_buffer_contains(&terminal, "Dependencies (Prerequisites)");
    assert_buffer_contains(&terminal, "Affected Resources");
    assert_buffer_contains(&terminal, "Resource Overlap Analysis");
}

#[test]
fn test_tier1_card_metadata_and_progress_presentation() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = false; // Full width so metadata and progress are not clipped

    let proj_id = Uuid::new_v4();

    // Task 1: Human Review with 50% progress
    let mut t1 = sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "META-1",
        "Review Task With Progress",
        TaskStatus::HumanReview,
    );
    t1.progress_percent = Some(50);
    t1.suggested_agent_name = Some("Agent-Agy-Worker".to_string());
    state.review_tasks.push(t1);

    // Task 2: Approved task with 100% progress
    let mut t2 = sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "META-2",
        "Fully Completed Task",
        TaskStatus::Completed,
    );
    t2.human_decision = Some(ApprovalStatus::Approved);
    state.review_tasks.push(t2);

    // Task 3: Executing task
    let t3 = sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "META-3",
        "Currently Executing Task",
        TaskStatus::Executing,
    );
    state.review_tasks.push(t3);

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            PlanReviewScreen::render(frame, frame.area(), &state);
        })
        .expect("PlanReviewScreen draw failed");

    // Status badges visible
    assert_buffer_contains(&terminal, "? HUMAN REVIEW");
    assert_buffer_contains(&terminal, "✔ APPROVED");
    assert_buffer_contains(&terminal, "▶ EXECUTING");

    // Agent name visible
    assert_buffer_contains(&terminal, "Agent-Agy-Worker");

    // Progress bar indicators visible
    assert_buffer_contains(&terminal, "50%");
    assert_buffer_contains(&terminal, "100%");
}

#[test]
fn test_tier1_keyboard_navigation_cursor_movement() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=4 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("NAV-{i}"),
            &format!("Navigation Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    assert_eq!(state.selected_task_index, 0);

    // Down arrow moves cursor to 1
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 1);

    // 'j' moves cursor to 2
    state.handle_key(press(KeyCode::Char('j')));
    assert_eq!(state.selected_task_index, 2);

    // Up arrow moves cursor back to 1
    state.handle_key(press(KeyCode::Up));
    assert_eq!(state.selected_task_index, 1);

    // 'k' moves cursor back to 0
    state.handle_key(press(KeyCode::Char('k')));
    assert_eq!(state.selected_task_index, 0);

    // Enter toggles details pane
    assert!(state.show_details_pane);
    state.handle_key(press(KeyCode::Enter));
    assert!(!state.show_details_pane);
    state.handle_key(press(KeyCode::Enter));
    assert!(state.show_details_pane);
}

#[test]
fn test_tier1_footer_action_hints_non_clipped() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    state.review_tasks.push(sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "FOOT-1",
        "Footer Test Task",
        TaskStatus::HumanReview,
    ));

    // Test across compact (80), standard (120), and wide (180) terminals
    for &width in &[80, 120, 180] {
        let mut terminal = create_terminal(width, 3);
        terminal
            .draw(|frame| {
                FooterWidget::render(frame, frame.area(), &state);
            })
            .unwrap_or_else(|_| panic!("Footer render failed at width {width}"));

        let buf = buffer_to_string(&terminal);
        assert!(
            buf.contains("[Y]")
                || buf.contains("Approve")
                || buf.contains("Nav")
                || buf.contains("Toggle"),
            "Footer at width {width} lacked essential hints. Buffer:\n{buf}"
        );
    }
}

// ============================================================================
// Tier 2: Boundary & Corner Conditions
// ============================================================================

#[test]
fn test_tier2_empty_plan_renders_quick_start_guide() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.review_tasks.clear();

    for &(w, h) in &[(80, 24), (120, 30), (180, 50)] {
        let mut terminal = create_terminal(w, h);
        terminal
            .draw(|frame| {
                PlanReviewScreen::render(frame, frame.area(), &state);
            })
            .unwrap_or_else(|_| panic!("Failed to render empty plan at {w}x{h}"));

        assert_buffer_contains(
            &terminal,
            "No tasks proposed yet.",
        );
        assert_buffer_contains(&terminal, "Quick Start Guide:");
        assert_buffer_contains(&terminal, "[2]");
        assert_buffer_contains(&terminal, "[I]");
        assert_buffer_contains(&terminal, "[Enter]");

        // Invariant: no phantom task cards rendered
        assert_buffer_does_not_contain(&terminal, "[PLAN-");
    }
}

#[test]
fn test_tier2_task_long_description_word_wrapping() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let mut item = sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "WRAP-1",
        "Long Description Task",
        TaskStatus::HumanReview,
    );

    // Multi-line long description exceeding 10 lines
    item.task.description = [
        "Line 1: Refactor the central coordinator state machine to support distributed heartbeats.",
        "Line 2: Ensure all NATS JetStream consumer subscriptions remain resilient under network partitions.",
        "Line 3: Implement an exponential backoff retry policy for transient PostgreSQL connection drops.",
        "Line 4: Acknowledge resource overlaps across concurrent tasks using explicit mutex leases.",
        "Line 5: Maintain backwards compatibility with legacy headless CLI flags without regression.",
        "Line 6: Validate memory bounds on the recent event ring buffer during high frequency bursts.",
        "Line 7: Stream telemetry metrics to the observability dashboard every fifty milliseconds.",
        "Line 8: Handle graceful task cancellation with clean workspace git worktree reclamation.",
        "Line 9: Provide interactive terminal resizing without visual tearing or off-by-one errors.",
        "Line 10: Complete the human approval workflow before assigning tasks to available agents.",
        "Line 11: Document all interface contracts and architectural boundaries in PROJECT.md.",
        "Line 12: Ensure all integration test suites pass with zero warnings across the workspace.",
    ]
    .join("\n");

    state.review_tasks.push(item);

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            DetailsPaneWidget::render(frame, frame.area(), &state);
        })
        .expect("DetailsPaneWidget failed with long description");

    assert_buffer_contains(&terminal, "Description");
    assert_buffer_contains(&terminal, "central coordinator");
    assert_buffer_contains(&terminal, "NATS JetStream");
}

#[test]
fn test_tier2_task_many_dependencies_display() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let mut item = sample_review_task(
        task_id,
        proj_id,
        "DEPS-1",
        "Highly Dependent Task",
        TaskStatus::HumanReview,
    );

    // 8 distinct dependencies
    for d in 1..=8 {
        let dep_id = Uuid::new_v4();
        item.dependencies.push(TaskDependency {
            dependent_id: task_id,
            depends_on_id: dep_id,
            kind: DependencyKind::Blocks,
        });
        state.review_tasks.push(sample_review_task(
            dep_id,
            proj_id,
            &format!("PREREQ-{d}"),
            &format!("Prerequisite Task #{d}"),
            TaskStatus::Completed,
        ));
    }

    state.review_tasks.insert(0, item);
    state.selected_task_index = 0;

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            DetailsPaneWidget::render(frame, frame.area(), &state);
        })
        .expect("DetailsPaneWidget failed with many dependencies");

    assert_buffer_contains(&terminal, "Dependencies (Prerequisites)");
    assert_buffer_contains(&terminal, "PREREQ-1");
}

#[test]
fn test_tier2_long_title_truncation_or_wrapping() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let long_title = "Architectural Redesign and Complete Re-implementation of Distributed State Machine Synchronization Protocol with Consensus Validation";

    let item = sample_review_task(
        Uuid::new_v4(),
        proj_id,
        "LONG-TITLE-1",
        long_title,
        TaskStatus::HumanReview,
    );
    state.review_tasks.push(item);

    let mut terminal = create_terminal(80, 24);
    terminal
        .draw(|frame| {
            TaskCardWidget::render(
                frame,
                frame.area(),
                &state.review_tasks[0],
                true,
                false,
            );
        })
        .expect("TaskCardWidget failed on long title");

    // Short ID must remain intact
    assert_buffer_contains(&terminal, "[LONG-TITLE-1]");
}

#[test]
fn test_tier2_selection_bounds_ctrl_a_and_esc() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=5 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("SEL-{i}"),
            &format!("Selection Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    assert!(state.selected_task_indices.is_empty());

    // Ctrl+A selects all tasks
    state.handle_key(press_ctrl(KeyCode::Char('a')));
    assert_eq!(state.selected_task_indices.len(), 5);
    for idx in 0..5 {
        assert!(state.selected_task_indices.contains(&idx));
    }

    // Esc clears selection
    state.handle_key(press(KeyCode::Esc));
    assert!(state.selected_task_indices.is_empty());

    // Space toggles individual selection
    state.selected_task_index = 2;
    state.handle_key(press(KeyCode::Char(' ')));
    assert_eq!(state.selected_task_indices.len(), 1);
    assert!(state.selected_task_indices.contains(&2));

    // Space again toggles it off
    state.handle_key(press(KeyCode::Char(' ')));
    assert!(state.selected_task_indices.is_empty());
}

#[test]
fn test_tier2_navigation_clamping_at_boundaries() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("CLAMP-{i}"),
            &format!("Clamping Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    // Clamping at index 0 on Up
    assert_eq!(state.selected_task_index, 0);
    state.handle_key(press(KeyCode::Up));
    assert_eq!(state.selected_task_index, 0);
    state.handle_key(press(KeyCode::Char('k')));
    assert_eq!(state.selected_task_index, 0);

    // Clamping at len - 1 on Down
    for _ in 0..10 {
        state.handle_key(press(KeyCode::Down));
    }
    assert_eq!(state.selected_task_index, 2);
}

// ============================================================================
// Tier 3: Cross-Feature Combinations
// ============================================================================

#[test]
fn test_tier3_focused_and_multi_selected_card_styling() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("CARD-{i}"),
            &format!("Card Style #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    // Task 0 is focused AND selected
    state.selected_task_index = 0;
    state.selected_task_indices.insert(0);

    // Task 2 is selected but NOT focused
    state.selected_task_indices.insert(2);

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            PlanReviewScreen::render(frame, frame.area(), &state);
        })
        .expect("PlanReviewScreen failed during focused+selected rendering");

    let buf = buffer_to_string(&terminal);

    // Both cards must show selection indicator (✓)
    assert!(
        buf.contains('✓') || buf.contains("[✓]"),
        "Buffer lacked selection checkmark for selected tasks. Buffer:\n{buf}"
    );

    // Focused card must also be identifiable
    assert!(buf.contains("[CARD-1]"));
    assert!(buf.contains("[CARD-3]"));
}

#[test]
fn test_tier3_pane_focus_switching() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("FOC-{i}"),
            &format!("Focus Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    // Initial focus is TaskList
    assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskList);

    // Press Right (or Left/h/l) switches focus to TaskInspector
    state.handle_key(press(KeyCode::Right));
    assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskInspector);

    // When TaskInspector is focused, Down / j scrolls inspector_scroll_offset
    assert_eq!(state.inspector_scroll_offset, 0);
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.inspector_scroll_offset, 1);

    state.handle_key(press(KeyCode::Char('j')));
    assert_eq!(state.inspector_scroll_offset, 2);

    // Up / k scrolls inspector_scroll_offset back
    state.handle_key(press(KeyCode::Up));
    assert_eq!(state.inspector_scroll_offset, 1);

    state.handle_key(press(KeyCode::Char('k')));
    assert_eq!(state.inspector_scroll_offset, 0);

    // PageDown scrolls by 5
    state.handle_key(press(KeyCode::PageDown));
    assert_eq!(state.inspector_scroll_offset, 5);

    // PageUp scrolls back by 5
    state.handle_key(press(KeyCode::PageUp));
    assert_eq!(state.inspector_scroll_offset, 0);

    // Press Left / Right switches back to TaskList
    state.handle_key(press(KeyCode::Left));
    assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskList);

    // When TaskList is focused, Down advances selected_task_index
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 1);
}

#[test]
fn test_tier3_inspector_scroll_offset_reset_on_task_navigation() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let t1_id = Uuid::new_v4();
    let t2_id = Uuid::new_v4();

    let t1 = sample_review_task(t1_id, proj_id, "RESET-1", "First Task", TaskStatus::HumanReview);
    let mut t2 = sample_review_task(t2_id, proj_id, "RESET-2", "Second Task", TaskStatus::HumanReview);

    // t2 depends on t1
    t2.dependencies.push(TaskDependency {
        dependent_id: t2_id,
        depends_on_id: t1_id,
        kind: DependencyKind::Blocks,
    });

    state.review_tasks.push(t1);
    state.review_tasks.push(t2);

    // Set non-zero inspector scroll offset
    state.inspector_scroll_offset = 6;

    // Moving cursor via Down arrow resets offset to 0
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.inspector_scroll_offset, 0);

    // Set non-zero offset again
    state.inspector_scroll_offset = 4;

    // jump_to_blocker resets offset to 0
    let jumped = state.jump_to_blocker();
    assert!(jumped);
    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0);

    // Set non-zero offset again
    state.inspector_scroll_offset = 9;

    // jump_to_dependent resets offset to 0
    let jumped_dep = state.jump_to_dependent();
    assert!(jumped_dep);
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.inspector_scroll_offset, 0);

    // select_task helper also resets offset to 0
    state.inspector_scroll_offset = 8;
    state.select_task(0);
    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0);
}

#[test]
fn test_tier3_mouse_scroll_column_disambiguation() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=4 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("DISAMBIG-{i}"),
            &format!("Disambiguation Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0);

    // 1. Mouse wheel over left column (col 15 < list_width)
    // Scrolls task cards
    state.handle_mouse(mouse_scroll(15, 10, MouseEventKind::ScrollDown));
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.inspector_scroll_offset, 0);

    // 2. Mouse wheel over right column (col 75 >= list_width)
    // Scrolls inspector pane
    state.handle_mouse(mouse_scroll(75, 10, MouseEventKind::ScrollDown));
    assert_eq!(state.selected_task_index, 1); // card selection unchanged
    assert_eq!(state.inspector_scroll_offset, 1);

    state.handle_mouse(mouse_scroll(75, 10, MouseEventKind::ScrollDown));
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.inspector_scroll_offset, 2);

    // Scroll up over right column
    state.handle_mouse(mouse_scroll(75, 10, MouseEventKind::ScrollUp));
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.inspector_scroll_offset, 1);

    // 3. Mouse wheel back over left column scrolls task cards
    state.handle_mouse(mouse_scroll(15, 10, MouseEventKind::ScrollUp));
    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0); // reset on card change
}

#[test]
fn test_tier3_selective_shift_a_bulk_approval_with_multi_selection() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let t1_id = Uuid::new_v4();
    let t2_id = Uuid::new_v4();
    let t3_id = Uuid::new_v4();
    let t4_id = Uuid::new_v4();

    state.review_tasks.push(sample_review_task(t1_id, proj_id, "SEL-1", "Task 1", TaskStatus::HumanReview));
    state.review_tasks.push(sample_review_task(t2_id, proj_id, "SEL-2", "Task 2", TaskStatus::HumanReview));
    state.review_tasks.push(sample_review_task(t3_id, proj_id, "SEL-3", "Task 3", TaskStatus::HumanReview));
    state.review_tasks.push(sample_review_task(t4_id, proj_id, "SEL-4", "Task 4", TaskStatus::HumanReview));

    // Multi-select tasks 0 and 2 only
    state.selected_task_indices.insert(0);
    state.selected_task_indices.insert(2);

    // Press Shift+A
    state.handle_key(press_shift(KeyCode::Char('A')));
    assert!(state.pending_confirmation.is_some());

    let modal = state.pending_confirmation.as_ref().unwrap();
    assert_eq!(modal.action, DestructiveAction::ApproveAllTasks);
    assert!(modal.title.contains('2') || modal.message.contains('2'));

    // Confirm bulk approval via 'y'
    let action = state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(action, Some(TuiAction::ApproveAllTasks));

    // ONLY tasks 0 and 2 must be approved
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(state.review_tasks[0].human_decision, Some(ApprovalStatus::Approved));

    assert_eq!(state.review_tasks[1].task.status, TaskStatus::HumanReview);
    assert_eq!(state.review_tasks[1].human_decision, None);

    assert_eq!(state.review_tasks[2].task.status, TaskStatus::Approved);
    assert_eq!(state.review_tasks[2].human_decision, Some(ApprovalStatus::Approved));

    assert_eq!(state.review_tasks[3].task.status, TaskStatus::HumanReview);
    assert_eq!(state.review_tasks[3].human_decision, None);

    // Selection must be cleared after approval
    assert!(state.selected_task_indices.is_empty());
}

#[test]
fn test_tier3_empty_shift_a_bulk_approval_prompts_all_pending() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    for i in 1..=3 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("ALL-{i}"),
            &format!("All Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    // No tasks multi-selected
    assert!(state.selected_task_indices.is_empty());

    // Press Shift+A
    state.handle_key(press_shift(KeyCode::Char('A')));
    assert!(state.pending_confirmation.is_some());

    let modal = state.pending_confirmation.as_ref().unwrap();
    assert_eq!(modal.action, DestructiveAction::ApproveAllTasks);

    // Confirm via 'y'
    let action = state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(action, Some(TuiAction::ApproveAllTasks));

    // ALL tasks transitioned to Approved
    for item in &state.review_tasks {
        assert_eq!(item.task.status, TaskStatus::Approved);
        assert_eq!(item.human_decision, Some(ApprovalStatus::Approved));
    }
}

#[test]
fn test_tier3_overlap_warning_acknowledgement_and_approval_flow() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let t1_id = Uuid::new_v4();
    let t2_id = Uuid::new_v4();
    let warning_id = Uuid::new_v4();

    let mut t1 = sample_review_task(
        t1_id,
        proj_id,
        "OVERLAP-1",
        "Task With Overlap",
        TaskStatus::HumanReview,
    );
    t1.overlap_warnings.push(OverlapWarning {
        id: warning_id,
        project_id: proj_id,
        task_ids: json!([t1_id, t2_id]),
        resource: "crates/coordinator/src/tui/screens/plan_review.rs".to_string(),
        severity: OverlapSeverity::Warning,
        acknowledged: false,
        created_at: Utc::now(),
    });
    state.active_overlaps.push(t1.overlap_warnings[0].clone());
    state.review_tasks.push(t1);

    // Verify unacknowledged banner renders on card
    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            TaskCardWidget::render(frame, frame.area(), &state.review_tasks[0], true, false);
        })
        .expect("Failed to render overlap card");
    assert_buffer_contains(&terminal, "⚠ 1 Resource Overlap Warning(s) [A to ack]");

    // Press 'a' to acknowledge overlap
    let action = state.handle_key(press(KeyCode::Char('a')));
    assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id }));
    assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);
    assert!(state.active_overlaps.is_empty());

    // Banner now shows acknowledged status
    terminal
        .draw(|frame| {
            TaskCardWidget::render(frame, frame.area(), &state.review_tasks[0], true, false);
        })
        .expect("Failed to render acknowledged card");
    assert_buffer_contains(&terminal, "✔ 1 Overlap Warning(s) Acknowledged");

    // Press 'y' to approve task
    let approve_action = state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(approve_action, Some(TuiAction::ApproveTask { task_id: t1_id }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::Approved)
    );
}

#[test]
fn test_tier3_inline_description_edit_flow() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let mut item = sample_review_task(
        task_id,
        proj_id,
        "EDIT-1",
        "Editable Task",
        TaskStatus::HumanReview,
    );
    item.task.description = "Original specification.".to_string();
    state.review_tasks.push(item);

    assert_eq!(state.input_mode, InputMode::Normal);

    // Press 'e' to enter description editing mode
    state.handle_key(press(KeyCode::Char('e')));
    assert_eq!(state.input_mode, InputMode::EditingDescription);
    assert_eq!(state.edit_buffer, "Original specification.");

    // Append edit text
    for c in " Augmented with security constraints.".chars() {
        state.handle_key(press(KeyCode::Char(c)));
    }

    // Press Enter to confirm and approve
    let action = state.handle_key(press(KeyCode::Enter));
    assert_eq!(state.input_mode, InputMode::Normal);
    assert_eq!(
        action,
        Some(TuiAction::EditTaskDescription {
            task_id,
            new_description: "Original specification. Augmented with security constraints.".to_string(),
        })
    );

    // Task is approved and decision reflects edited approval
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(
        state.review_tasks[0].human_decision,
        Some(ApprovalStatus::EditedAndApproved)
    );
    assert_eq!(
        state.review_tasks[0].task.description,
        "Original specification. Augmented with security constraints."
    );
}

#[test]
fn test_tier3_single_task_approval_vs_rejection_actions() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.confirm_destructive_actions = false; // direct execution mode for unit verification

    let proj_id = Uuid::new_v4();
    let t1_id = Uuid::new_v4();
    let t2_id = Uuid::new_v4();

    state.review_tasks.push(sample_review_task(
        t1_id,
        proj_id,
        "ACT-1",
        "Approve Me",
        TaskStatus::HumanReview,
    ));
    state.review_tasks.push(sample_review_task(
        t2_id,
        proj_id,
        "ACT-2",
        "Reject Me",
        TaskStatus::HumanReview,
    ));

    // Approve task 0 via 'y'
    assert_eq!(state.selected_task_index, 0);
    let act1 = state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(act1, Some(TuiAction::ApproveTask { task_id: t1_id }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);

    // Auto-advance moved cursor to index 1
    assert_eq!(state.selected_task_index, 1);

    // Reject task 1 via 'n'
    let act2 = state.handle_key(press(KeyCode::Char('n')));
    assert_eq!(act2, Some(TuiAction::RejectTask { task_id: t2_id }));
    assert_eq!(
        state.review_tasks[1].human_decision,
        Some(ApprovalStatus::Rejected)
    );
}

// ============================================================================
// Tier 4: Real-World Scenarios
// ============================================================================

#[test]
fn test_tier4_complete_operator_review_workflow() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let t1_id = Uuid::new_v4();
    let t2_id = Uuid::new_v4();
    let t3_id = Uuid::new_v4();
    let t4_id = Uuid::new_v4();
    let t5_id = Uuid::new_v4();

    state.review_tasks.push(sample_review_task(
        t1_id,
        proj_id,
        "FLOW-1",
        "First Task",
        TaskStatus::HumanReview,
    ));
    state.review_tasks.push(sample_review_task(
        t2_id,
        proj_id,
        "FLOW-2",
        "Second Task with Overlap",
        TaskStatus::HumanReview,
    ));
    state.review_tasks.push(sample_review_task(
        t3_id,
        proj_id,
        "FLOW-3",
        "Third Task for Edit",
        TaskStatus::HumanReview,
    ));
    state.review_tasks.push(sample_review_task(
        t4_id,
        proj_id,
        "FLOW-4",
        "Fourth Task for Rejection",
        TaskStatus::HumanReview,
    ));
    state.review_tasks.push(sample_review_task(
        t5_id,
        proj_id,
        "FLOW-5",
        "Fifth Task for Cancellation",
        TaskStatus::HumanReview,
    ));

    // Add overlap warning on Task 1 (FLOW-2)
    let w_id = Uuid::new_v4();
    state.review_tasks[1].overlap_warnings.push(OverlapWarning {
        id: w_id,
        project_id: proj_id,
        task_ids: json!([t2_id, t1_id]),
        resource: "crates/coordinator/Cargo.toml".to_string(),
        severity: OverlapSeverity::Warning,
        acknowledged: false,
        created_at: Utc::now(),
    });
    state.active_overlaps.push(state.review_tasks[1].overlap_warnings[0].clone());

    // 1. Initial render at compact 80x24
    let mut terminal = create_terminal(80, 24);
    terminal
        .draw(|frame| ui::render(frame, &state))
        .expect("Render 80x24 failed");

    // 2. Approve Task 0
    state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
    assert_eq!(state.selected_task_index, 1);

    // 3. Acknowledge overlap on Task 1 and approve
    state.handle_key(press(KeyCode::Char('a')));
    assert!(state.review_tasks[1].overlap_warnings[0].acknowledged);
    state.handle_key(press(KeyCode::Char('y')));
    assert_eq!(state.review_tasks[1].task.status, TaskStatus::Approved);
    assert_eq!(state.selected_task_index, 2);

    // 4. Edit description on Task 2
    state.handle_key(press(KeyCode::Char('e')));
    for c in " (Reviewed)".chars() {
        state.handle_key(press(KeyCode::Char(c)));
    }
    state.handle_key(press(KeyCode::Enter));
    assert_eq!(state.review_tasks[2].task.status, TaskStatus::Approved);
    assert!(state.review_tasks[2].task.description.contains("(Reviewed)"));

    // 5. Navigate to Task 3 and Reject (with safety confirmation dialog)
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 3);
    state.handle_key(press(KeyCode::Char('n')));
    assert!(state.pending_confirmation.is_some());
    state.handle_key(press(KeyCode::Char('y'))); // Confirm rejection
    assert_eq!(state.review_tasks[3].task.status, TaskStatus::Rejected);

    // 6. Navigate to Task 4 and Cancel (with safety confirmation dialog)
    state.handle_key(press(KeyCode::Down));
    assert_eq!(state.selected_task_index, 4);
    state.handle_key(press(KeyCode::Char('c')));
    assert!(state.pending_confirmation.is_some());
    state.handle_key(press(KeyCode::Char('y'))); // Confirm cancellation
    assert_eq!(state.review_tasks[4].task.status, TaskStatus::Cancelled);

    // 7. Verify final state renders without errors at standard 120x30
    let mut standard_terminal = create_terminal(120, 30);
    standard_terminal
        .draw(|frame| ui::render(frame, &state))
        .expect("Render 120x30 failed");

    assert_buffer_contains(&standard_terminal, "✔ APPROVED");
    assert_buffer_contains(&standard_terminal, "✖ REJECTED");
}

#[test]
fn test_tier4_dynamic_resizing_during_review() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 1..=4 {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("DYN-{i}"),
            &format!("Dynamic Resize Task #{i}"),
            TaskStatus::HumanReview,
        ));
    }

    // Set cursor to task 1 and select tasks 1 and 3
    state.selected_task_index = 1;
    state.selected_task_indices.insert(1);
    state.selected_task_indices.insert(3);

    let sizes = [(80, 24), (120, 30), (180, 50), (100, 25), (80, 24)];

    for &(w, h) in &sizes {
        let mut term = create_terminal(w, h);
        term.draw(|frame| {
            ui::render(frame, &state);
        })
        .unwrap_or_else(|_| panic!("Failed to render during resize at {w}x{h}"));

        // Invariant: active task and selections remain intact
        assert_eq!(state.selected_task_index, 1);
        assert_eq!(state.selected_task_indices.len(), 2);

        // Task content visible in rendered buffer
        assert_buffer_contains(&term, "[DYN-2]");
    }
}

#[test]
fn test_tier4_dag_traversal_and_jump_review() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let t_a_id = Uuid::new_v4();
    let t_b_id = Uuid::new_v4();
    let t_c_id = Uuid::new_v4();

    let task_a = sample_review_task(
        t_a_id,
        proj_id,
        "DAG-A",
        "Root Architecture",
        TaskStatus::HumanReview,
    );
    let mut task_b = sample_review_task(
        t_b_id,
        proj_id,
        "DAG-B",
        "Middle Layer Implementation",
        TaskStatus::HumanReview,
    );
    let task_c = sample_review_task(
        t_c_id,
        proj_id,
        "DAG-C",
        "Frontend Integration",
        TaskStatus::HumanReview,
    );

    // B depends on A; C depends on B
    task_b.dependencies.push(TaskDependency {
        dependent_id: t_b_id,
        depends_on_id: t_a_id,
        kind: DependencyKind::Blocks,
    });

    state.review_tasks.push(task_a);
    state.review_tasks.push(task_b);
    state.review_tasks.push(task_c);

    // Operator starts on Task B (index 1)
    state.selected_task_index = 1;

    // Jump to blocker moves to Task A (index 0)
    let jumped = state.jump_to_blocker();
    assert!(jumped);
    assert_eq!(state.selected_task_index, 0);

    // Jump to dependent moves to Task B (index 1)
    let jumped_dep = state.jump_to_dependent();
    assert!(jumped_dep);
    assert_eq!(state.selected_task_index, 1);

    // Toggle DAG view mode with 'g'
    assert!(!state.show_dag_view);
    state.handle_key(press(KeyCode::Char('g')));
    assert!(state.show_dag_view);

    // Render DAG view in terminal
    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            DagViewWidget::render(frame, frame.area(), &state);
        })
        .expect("DagViewWidget rendering failed");

    assert_buffer_contains(&terminal, "Dependency Graph (DAG)");
    assert_buffer_contains(&terminal, "DAG-A");
    assert_buffer_contains(&terminal, "DAG-B");
    assert_buffer_contains(&terminal, "DAG-C");
}
