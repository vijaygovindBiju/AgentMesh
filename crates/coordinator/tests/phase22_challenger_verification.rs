//! Empirical Challenger Verification Suite for Phase 22 Plan Review Screen
//!
//! Evaluates:
//! 1. Mouse Interaction Matrix (clicking & scrolling across left/right columns at varying coordinates)
//! 2. Bulk vs Selective Approval Integrity (0 tasks vs {1, 3} of 5 tasks)
//! 3. Word Wrapping Verification (no split words, no ragged leading whitespace, Wrap { trim: true })
//! 4. High-Contrast Indicator (▶[✓] and dual-highlight when focused and selected simultaneously)

use chrono::Utc;
use coordinator::domain::{ApprovalStatus, Task, TaskStatus};
use coordinator::tui::screens::PlanReviewScreen;
use coordinator::tui::state::{
    AppState, CurrentScreen, DestructiveAction, ReviewTaskState, TuiAction,
};
use coordinator::tui::widgets::DetailsPaneWidget;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

// ============================================================================
// Helpers
// ============================================================================

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn buffer_to_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    lines
}

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    buffer_to_lines(terminal).join("\n")
}

fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn make_mouse_scroll(column: u16, row: u16, kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn make_mouse_click(column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers,
    }
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
        affected_resources: json!(["crates/coordinator/src/main.rs"]),
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
        suggested_agent_name: Some("WorkerAgent".to_string()),
        overlap_warnings: Vec::new(),
        dependencies: Vec::new(),
        human_decision: None,
        progress_percent: None,
    }
}

fn setup_plan_state(task_count: usize) -> AppState {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    for i in 0..task_count {
        state.review_tasks.push(sample_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("T-{i}"),
            &format!("Task Title #{i}"),
            TaskStatus::HumanReview,
        ));
    }
    state
}

// ============================================================================
// 1. Mouse Interaction Matrix Tests
// ============================================================================

#[test]
fn test_mouse_scroll_matrix_left_vs_right_columns() {
    let mut state = setup_plan_state(5);
    // At default 80x24 fallback, list_width = 80 * 45 / 100 = 36.
    // Columns 0..35 = Task List (Left). Columns 36..79 = Inspector (Right).

    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0);

    // Left column scroll down across varying row/col coordinates
    let left_coords = [(0, 5), (10, 10), (20, 15), (35, 8)];
    for (col, row) in left_coords {
        let prev_task = state.selected_task_index;
        let _prev_inspector = state.inspector_scroll_offset;

        state.handle_mouse(make_mouse_scroll(col, row, MouseEventKind::ScrollDown));

        // Must scroll task card selection down by 1 (unless at boundary)
        let expected_task = (prev_task + 1).min(state.review_tasks.len() - 1);
        assert_eq!(
            state.selected_task_index, expected_task,
            "Left column scroll at col {col}, row {row} failed to increment task selection"
        );
        // Inspector offset must remain 0 (reset on task selection change)
        assert_eq!(
            state.inspector_scroll_offset, 0,
            "Left column scroll at col {col}, row {row} improperly altered inspector offset"
        );
    }

    // Now test Right column scroll down across varying row/col coordinates
    let right_coords = [(36, 5), (45, 10), (60, 15), (75, 20), (79, 8)];
    let prev_task = state.selected_task_index;
    for (i, (col, row)) in right_coords.iter().enumerate() {
        state.handle_mouse(make_mouse_scroll(*col, *row, MouseEventKind::ScrollDown));

        // Task selection must be strictly untouched
        assert_eq!(
            state.selected_task_index, prev_task,
            "Right column scroll at col {col}, row {row} changed task selection"
        );
        // Inspector offset must increase
        assert_eq!(
            state.inspector_scroll_offset,
            i + 1,
            "Right column scroll at col {col}, row {row} failed to increment inspector offset"
        );
    }

    // Test Right column scroll up
    for (i, (col, row)) in right_coords.iter().enumerate().rev() {
        state.handle_mouse(make_mouse_scroll(*col, *row, MouseEventKind::ScrollUp));

        assert_eq!(
            state.selected_task_index, prev_task,
            "Right column scroll up at col {col}, row {row} changed task selection"
        );
        assert_eq!(
            state.inspector_scroll_offset, i,
            "Right column scroll up at col {col}, row {row} failed to decrement inspector offset"
        );
    }

    // Test Left column scroll up
    while state.selected_task_index > 0 {
        let prev = state.selected_task_index;
        state.handle_mouse(make_mouse_scroll(15, 10, MouseEventKind::ScrollUp));
        assert_eq!(state.selected_task_index, prev - 1);
        assert_eq!(state.inspector_scroll_offset, 0);
    }
}

#[test]
fn test_mouse_click_task_cards_left_column() {
    let mut state = setup_plan_state(5);
    // list_top = 2 (header) + 1 = 3.
    // Card height = 5 lines.
    // Row 3..7 => Card 0
    // Row 8..12 => Card 1
    // Row 13..17 => Card 2

    // Click Card 1 at row 9, col 10
    state.handle_mouse(make_mouse_click(10, 9, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 1,
        "Clicking row 9 should select card 1"
    );
    assert!(
        state.selected_task_indices.is_empty(),
        "Normal click clears multi-selection"
    );

    // Click Card 0 at row 4, col 5 (while window is at offset 0)
    state.handle_mouse(make_mouse_click(5, 4, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 0,
        "Clicking row 4 should select card 0"
    );

    // Click Card 2 at row 14, col 20 (while window is at offset 0)
    state.handle_mouse(make_mouse_click(20, 14, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 2,
        "Clicking row 14 should select card 2"
    );

    // When card 2 is selected, sliding window shifts by 1, placing Card 1 at row 3..7
    // Clicking row 4 now selects Card 1:
    state.handle_mouse(make_mouse_click(5, 4, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 1,
        "Clicking row 4 selects Card 1 due to sliding window"
    );

    // Clicking row 4 again selects Card 0 once window has returned to offset 0:
    state.handle_mouse(make_mouse_click(5, 4, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 0,
        "Clicking row 4 selects Card 0 once window is at top"
    );

    // Ctrl+Click Card 1 to toggle multi-selection
    state.handle_mouse(make_mouse_click(10, 9, KeyModifiers::CONTROL));
    assert_eq!(state.selected_task_index, 1);
    assert!(state.selected_task_indices.contains(&1));

    // Ctrl+Click Card 2 to add to multi-selection (since window is at offset 0, Card 2 is at row 14)
    state.handle_mouse(make_mouse_click(10, 14, KeyModifiers::CONTROL));
    assert_eq!(state.selected_task_index, 2);
    assert!(state.selected_task_indices.contains(&1));
    assert!(state.selected_task_indices.contains(&2));

    // Clicking right column (Inspector, e.g. col 50, row 10) must NOT change task selection
    state.handle_mouse(make_mouse_click(50, 10, KeyModifiers::NONE));
    assert_eq!(
        state.selected_task_index, 2,
        "Clicking right column should not change selection"
    );
}

#[test]
fn test_mouse_click_coordinate_mapping_when_scrolled() {
    // Stress test: when task list is scrolled down, does clicking a card
    // accurately select the card visually displayed at that row?
    let mut state = setup_plan_state(10);
    // At 80x24: inner_height = 17, visible_cards = 3.
    // If selected_task_index = 5:
    // render_task_list formula:
    //   half_visible = 3 / 2 = 1
    //   max_offset = 10 - 3 = 7
    //   render_scroll_offset = 5.saturating_sub(1).min(7) = 4.
    // So render displays:
    //   Row 3..7: Task 4
    //   Row 8..12: Task 5
    //   Row 13..17: Task 6
    state.selected_task_index = 5;

    // Let's render using full top-level ui::render and inspect what card is rendered at row 3..7
    let mut terminal = create_terminal(80, 24);
    terminal
        .draw(|frame| {
            coordinator::tui::ui::render(frame, &state);
        })
        .unwrap();

    let lines = buffer_to_lines(&terminal);
    println!("Rendered line at row 3: {}", lines[3]);
    println!("Rendered line at row 4: {}", lines[4]);
    println!("Rendered line at row 8: {}", lines[8]);

    // Check whether row 4 has [T-4]
    let row4_shows_t4 = lines[4].contains("[T-4]");
    assert!(row4_shows_t4, "Expected row 4 to render task card T-4");

    // Now click row 4
    state.handle_mouse(make_mouse_click(10, 4, KeyModifiers::NONE));
    println!(
        "After clicking row 4, selected_task_index is: {}",
        state.selected_task_index
    );

    // Verified fix:
    // When the user clicks the card visually rendered at row 4 (Task 4),
    // the system MUST select Task 4.
    assert_eq!(
        state.selected_task_index, 4,
        "User clicked card [T-4] at row 4, but handle_mouse selected task {} instead of task 4!",
        state.selected_task_index
    );
}

#[test]
fn test_mouse_scroll_wide_terminal_desynchronization() {
    let mut state = setup_plan_state(5);
    assert_eq!(state.selected_task_index, 0);
    assert_eq!(state.inspector_scroll_offset, 0);

    // On a 180-col terminal, PlanReviewScreen allocates Constraint::Length(64) to the left column (Task List).
    // The Task List occupies columns 0..63.
    // The Inspector occupies columns 64..179.
    // Therefore, column 50 is visually inside the Task List!
    let mut terminal = create_terminal(180, 50);
    terminal
        .draw(|frame| {
            coordinator::tui::ui::render(frame, &state);
        })
        .unwrap();

    // Hover mouse over column 50 (inside Task List) and scroll down
    state.handle_mouse(make_mouse_scroll(50, 10, MouseEventKind::ScrollDown));

    // Verified fix:
    // With synchronized responsive breakpoints and recorded viewport size,
    // column 50 is correctly routed to Task List scroll:
    assert_eq!(
        state.selected_task_index, 1,
        "Column 50 is inside Task List (0..63) on 180-col screen, but handle_mouse scrolled inspector instead! task_index={}, inspector_offset={}",
        state.selected_task_index, state.inspector_scroll_offset
    );
}

// ============================================================================
// 2. Bulk vs Selective Approval Integrity Tests
// ============================================================================

#[test]
fn test_bulk_vs_selective_approval_zero_tasks_selected() {
    let mut state = setup_plan_state(5);
    assert!(state.selected_task_indices.is_empty());

    // Press Shift+A
    state.handle_key(make_key(KeyCode::Char('A'), KeyModifiers::SHIFT));

    // Must prompt confirmation modal for ALL 5 pending tasks
    assert!(
        state.pending_confirmation.is_some(),
        "Shift+A with 0 tasks selected must open confirmation modal"
    );
    let modal = state.pending_confirmation.as_ref().unwrap();
    assert_eq!(modal.action, DestructiveAction::ApproveAllTasks);
    assert_eq!(modal.title, "Confirm Bulk Task Approval");
    assert!(
        modal.message.contains("5 pending tasks"),
        "Modal message must mention all 5 pending tasks. Message: {}",
        modal.message
    );

    // Confirm with 'y'
    let action = state.handle_key(make_key(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(action, Some(TuiAction::ApproveAllTasks));

    // Verify all 5 tasks are approved
    for (i, task_item) in state.review_tasks.iter().enumerate() {
        assert_eq!(
            task_item.task.status,
            TaskStatus::Approved,
            "Task {i} was not approved by bulk approval"
        );
        assert_eq!(
            task_item.human_decision,
            Some(ApprovalStatus::Approved),
            "Task {i} human_decision was not updated"
        );
    }
}

#[test]
fn test_bulk_vs_selective_approval_tasks_1_and_3_selected() {
    let mut state = setup_plan_state(5);

    // Select tasks 1 and 3 out of {0, 1, 2, 3, 4}
    state.selected_task_indices.insert(1);
    state.selected_task_indices.insert(3);
    assert_eq!(state.selected_task_indices.len(), 2);

    // Press Shift+A
    state.handle_key(make_key(KeyCode::Char('A'), KeyModifiers::SHIFT));

    // Must prompt confirmation modal for SELECTED 2 tasks
    assert!(
        state.pending_confirmation.is_some(),
        "Shift+A with tasks {{1, 3}} selected must open confirmation modal"
    );
    let modal = state.pending_confirmation.as_ref().unwrap();
    assert_eq!(modal.action, DestructiveAction::ApproveAllTasks);
    assert_eq!(modal.title, "Confirm Bulk Task Approval");
    assert!(
        modal.message.contains("selected 2 task(s)"),
        "Modal message must indicate 2 selected tasks. Message: {}",
        modal.message
    );

    // Confirm with 'y'
    let action = state.handle_key(make_key(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(action, Some(TuiAction::ApproveAllTasks));

    // Verify ONLY tasks 1 and 3 are Approved
    assert_eq!(
        state.review_tasks[0].task.status,
        TaskStatus::HumanReview,
        "Task 0 should remain HumanReview"
    );
    assert_eq!(state.review_tasks[0].human_decision, None);

    assert_eq!(
        state.review_tasks[1].task.status,
        TaskStatus::Approved,
        "Task 1 MUST be Approved"
    );
    assert_eq!(
        state.review_tasks[1].human_decision,
        Some(ApprovalStatus::Approved)
    );

    assert_eq!(
        state.review_tasks[2].task.status,
        TaskStatus::HumanReview,
        "Task 2 should remain HumanReview"
    );
    assert_eq!(state.review_tasks[2].human_decision, None);

    assert_eq!(
        state.review_tasks[3].task.status,
        TaskStatus::Approved,
        "Task 3 MUST be Approved"
    );
    assert_eq!(
        state.review_tasks[3].human_decision,
        Some(ApprovalStatus::Approved)
    );

    assert_eq!(
        state.review_tasks[4].task.status,
        TaskStatus::HumanReview,
        "Task 4 should remain HumanReview"
    );
    assert_eq!(state.review_tasks[4].human_decision, None);

    // Verify selection is cleared after approval
    assert!(
        state.selected_task_indices.is_empty(),
        "Selection indices must be cleared after approval"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Approved selected 2 task(s).")
    );
}

// ============================================================================
// 3. Word Wrapping Verification Tests
// ============================================================================

#[test]
fn test_word_wrapping_no_split_words_and_no_ragged_leading_whitespace() {
    let mut state = setup_plan_state(1);

    // A realistic multi-sentence paragraph with diverse word lengths and NO manual newlines
    let raw_paragraph = "The distributed consensus coordinator orchestrates autonomous agents \
across heterogenous worker nodes. Every task assignment is rigorously evaluated for overlap \
conflicts and dependency constraints before execution dispatch. High-performance streaming \
telemetry guarantees real-time observability across all connected daemon processes.";

    state.review_tasks[0].task.description = raw_paragraph.to_string();

    // Test across inspector widths: compact (44 cols), standard (60 cols), wide (100 cols)
    for (width, height) in [(44, 20), (60, 25), (100, 30)] {
        let mut terminal = create_terminal(width, height);
        terminal
            .draw(|frame| {
                DetailsPaneWidget::render(frame, frame.area(), &state);
            })
            .expect("Render failed in word wrapping test");

        let lines = buffer_to_lines(&terminal);
        let orig_words: Vec<&str> = raw_paragraph.split_whitespace().collect();

        // Find the lines belonging to the Description block (between "Description" title and next block)
        let mut in_desc = false;
        let mut desc_lines = Vec::new();
        for line in &lines {
            if line.contains("Description") {
                in_desc = true;
                continue;
            }
            if in_desc {
                // If we reach the next block (Dependencies / Prerequisites / Affected Resources)
                if line.contains("Dependencies")
                    || line.contains("Prerequisites")
                    || line.contains("Affected Resources")
                {
                    break;
                }
                // Strip the vertical borders ('│' or '║' or ' ')
                let trimmed = line.trim();
                let inner = trimmed.trim_matches(|c: char| {
                    c == '│'
                        || c == '║'
                        || c == '┌'
                        || c == '┐'
                        || c == '└'
                        || c == '┘'
                        || c == '─'
                        || c == '━'
                        || c == '┃'
                });
                if !inner.trim().is_empty() {
                    desc_lines.push(inner.to_string());
                }
            }
        }

        assert!(
            !desc_lines.is_empty(),
            "Could not extract description lines at width {width}"
        );

        // Check 1: No ragged leading whitespace on wrapped lines
        for (idx, d_line) in desc_lines.iter().enumerate() {
            // Trim { trim: true } must strip leading spaces from wrapped lines
            let leading_spaces = d_line.len() - d_line.trim_start().len();
            assert_eq!(
                leading_spaces, 0,
                "Ragged leading whitespace detected on description line {idx} at width {width}: '{d_line}'"
            );
        }

        // Check 2: No words split across lines
        // For every word on every line, it must match an original word (or punctuation attached)
        for (line_idx, d_line) in desc_lines.iter().enumerate() {
            let line_words: Vec<&str> = d_line.split_whitespace().collect();
            for word in line_words {
                let clean_word = word.trim_matches(|c: char| c.is_ascii_punctuation());
                if clean_word.is_empty() {
                    continue;
                }
                let found = orig_words.iter().any(|&ow| {
                    let clean_ow = ow.trim_matches(|c: char| c.is_ascii_punctuation());
                    clean_ow == clean_word
                });
                assert!(
                    found,
                    "Word '{word}' on line {line_idx} at width {width} appears to be split mid-word! Line: '{d_line}'"
                );
            }
        }
    }
}

// ============================================================================
// 4. High-Contrast Indicator Tests
// ============================================================================

#[test]
fn test_high_contrast_indicator_focused_and_selected() {
    let mut state = setup_plan_state(4);

    // Setup:
    // Task 0: neither focused nor selected
    // Task 1: focused only
    // Task 2: selected only
    // Task 3: BOTH focused and selected

    state.selected_task_index = 3; // Task 3 is focused
    state.selected_task_indices.insert(2); // Task 2 is selected
    state.selected_task_indices.insert(3); // Task 3 is selected

    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|frame| {
            PlanReviewScreen::render(frame, frame.area(), &state);
        })
        .expect("Render failed in high-contrast test");

    let buffer_str = buffer_to_string(&terminal);

    // Check Task 3: must contain "▶[✓]" and "[SELECTED]" title
    assert!(
        buffer_str.contains("▶[✓]"),
        "Buffer lacked high-contrast prefix '▶[✓]' for task 3 (focused and selected). Rendered:\n{buffer_str}"
    );
    assert!(
        buffer_str.contains("[SELECTED]"),
        "Buffer lacked dual-highlight title '[SELECTED]' for task 3. Rendered:\n{buffer_str}"
    );

    // Check Task 2: selected only -> must contain "  [✓] " and NOT "▶[✓]" or "▶ [T-2]"
    assert!(
        buffer_str.contains("[✓] [T-2]"),
        "Buffer lacked selection prefix '[✓]' for task 2. Rendered:\n{buffer_str}"
    );

    // Now test Task 1: focused only
    state.selected_task_index = 1;
    terminal
        .draw(|frame| {
            PlanReviewScreen::render(frame, frame.area(), &state);
        })
        .expect("Render failed in focused-only test");

    let buffer_str2 = buffer_to_string(&terminal);
    // Task 1 should have "▶ [T-1]"
    assert!(
        buffer_str2.contains("▶ [T-1]") || buffer_str2.contains("▶  [T-1]"),
        "Buffer lacked focus arrow '▶' for task 1 (focused only). Rendered:\n{buffer_str2}"
    );
    // Task 1 must NOT have "[✓]"
    assert!(
        !buffer_str2.contains("▶[✓] [T-1]"),
        "Task 1 should not have '[✓]' since it is not selected"
    );
}

#[test]
fn test_exhaustive_mouse_click_matrix_across_all_scroll_positions_and_screen_sizes() {
    let screen_sizes = [(80, 24), (100, 30), (120, 30), (160, 45), (180, 50)];
    let task_count = 10;

    for (width, height) in screen_sizes {
        for initial_selected in 0..task_count {
            let mut state = setup_plan_state(task_count);
            state.selected_task_index = initial_selected;

            let mut terminal = create_terminal(width, height);
            terminal
                .draw(|frame| {
                    coordinator::tui::ui::render(frame, &state);
                })
                .expect("Failed to draw frame");

            let lines = buffer_to_lines(&terminal);

            // Scan lines for rendered tasks in the left column (Task List)
            let list_width = if width < 90 {
                width * 46 / 100
            } else if width < 140 {
                width * 44 / 100
            } else {
                64
            } as usize;

            for (row_idx, line) in lines.iter().enumerate() {
                let left_slice: String = line.chars().take(list_width).collect();

                if let Some(pos) = left_slice.find("[T-") {
                    let after = &left_slice[pos + 3..];
                    if let Some(end) = after.find(']') {
                        let id_str = &after[..end];
                        if let Ok(task_num) = id_str.parse::<usize>() {
                            let mut click_state = setup_plan_state(task_count);
                            click_state.set_viewport_size(width, height);
                            click_state.selected_task_index = initial_selected;
                            click_state.handle_mouse(make_mouse_click(
                                10,
                                row_idx as u16,
                                KeyModifiers::NONE,
                            ));
                            assert_eq!(
                                click_state.selected_task_index, task_num,
                                "Failed at ({width}x{height}), initial_selected={initial_selected}: clicked row {row_idx} rendering [T-{task_num}], but selected {}",
                                click_state.selected_task_index
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_exhaustive_column_boundary_matrix_across_all_breakpoints() {
    let widths = [70, 80, 89, 90, 100, 120, 139, 140, 160, 180, 200];
    let height = 30;

    for width in widths {
        let expected_list_width = if width < 90 {
            width * 46 / 100
        } else if width < 140 {
            width * 44 / 100
        } else {
            64
        };

        let mut state = setup_plan_state(5);
        state.selected_task_index = 0;
        state.inspector_scroll_offset = 0;

        let mut terminal = create_terminal(width, height);
        terminal
            .draw(|frame| {
                coordinator::tui::ui::render(frame, &state);
            })
            .expect("Failed to draw frame");

        // 1. Column list_width - 1 MUST be inside Task List
        let col_left = expected_list_width - 1;
        state.handle_mouse(make_mouse_scroll(col_left, 10, MouseEventKind::ScrollDown));
        assert_eq!(
            state.selected_task_index, 1,
            "At width {width}: col {col_left} (expected Task List) failed to scroll task selection"
        );
        assert_eq!(
            state.inspector_scroll_offset, 0,
            "At width {width}: col {col_left} scroll modified inspector offset"
        );

        // 2. Column list_width MUST be inside Inspector
        let col_right = expected_list_width;
        state.handle_mouse(make_mouse_scroll(col_right, 10, MouseEventKind::ScrollDown));
        assert_eq!(
            state.selected_task_index, 1,
            "At width {width}: col {col_right} (expected Inspector) modified task selection"
        );
        assert_eq!(
            state.inspector_scroll_offset, 1,
            "At width {width}: col {col_right} failed to scroll inspector offset"
        );
    }
}

#[test]
fn test_mouse_click_edge_cases_and_adversarial_inputs() {
    // 1. Empty plan: clicking anywhere in task list must not panic or select anything
    let mut empty_state = setup_plan_state(0);
    empty_state.set_viewport_size(120, 30);
    assert!(empty_state.review_tasks.is_empty());
    for row in [0, 4, 10, 15, 25, 29] {
        for col in [0, 10, 30, 50, 119] {
            empty_state.handle_mouse(make_mouse_click(col, row, KeyModifiers::NONE));
            assert_eq!(empty_state.selected_task_index, 0);
            assert!(empty_state.selected_task_indices.is_empty());
        }
    }

    // 2. Short plan (2 tasks) on large terminal (120x30):
    // Cards only exist at rows 4..8 (task 0) and rows 9..13 (task 1).
    // Clicking rows 14..25 (empty space below cards) must be a safe no-op.
    let mut state = setup_plan_state(2);
    state.set_viewport_size(120, 30);
    state.selected_task_index = 0;

    for empty_row in 14..26 {
        state.handle_mouse(make_mouse_click(10, empty_row, KeyModifiers::NONE));
        assert_eq!(
            state.selected_task_index, 0,
            "Clicking empty row {empty_row} below cards altered selected_task_index"
        );
    }

    // 3. Top and bottom border row safety of Plan Review container:
    // With header_height = 3, row 3 is the top border (list_top - 1).
    // Row 26 is the bottom border (header_height + body_height - 1).
    for border_row in [3, 26] {
        state.handle_mouse(make_mouse_click(10, border_row, KeyModifiers::NONE));
        assert_eq!(
            state.selected_task_index, 0,
            "Clicking border row {border_row} altered selected_task_index"
        );
    }

    // 4. Ctrl+Click multi-selection toggle mechanics
    state.current_screen = CurrentScreen::PlanReview;
    state.handle_mouse(make_mouse_click(10, 9, KeyModifiers::CONTROL)); // Click card 1
    assert_eq!(state.selected_task_index, 1);
    assert_eq!(state.selected_task_indices.len(), 1);
    assert!(state.selected_task_indices.contains(&1));

    state.handle_mouse(make_mouse_click(10, 9, KeyModifiers::CONTROL)); // Click card 1 again (toggle off)
    assert_eq!(state.selected_task_index, 1);
    assert!(state.selected_task_indices.is_empty());

    state.handle_mouse(make_mouse_click(10, 4, KeyModifiers::CONTROL)); // Click card 0
    state.handle_mouse(make_mouse_click(10, 9, KeyModifiers::CONTROL)); // Click card 1
    assert_eq!(state.selected_task_indices.len(), 2);
    assert!(state.selected_task_indices.contains(&0));
    assert!(state.selected_task_indices.contains(&1));

    // Normal click clears multi-selection
    state.handle_mouse(make_mouse_click(10, 4, KeyModifiers::NONE));
    assert_eq!(state.selected_task_index, 0);
    assert!(state.selected_task_indices.is_empty());
}
