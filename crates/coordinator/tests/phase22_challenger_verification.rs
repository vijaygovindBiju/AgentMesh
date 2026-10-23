//! Empirical Challenger Verification Suite for Phase 22 Plan Review Screen
//!
//! Evaluates:
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
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
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
// ============================================================================

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
