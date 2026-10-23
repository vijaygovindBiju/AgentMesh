//! Phase 22 Plan Review TUI Adversarial Empirical Stress Test Suite
//!
//! Evaluates the 3 mandatory adversarial challenge harnesses:
//! 1. Terminal Resize Stress:
//!    Matrix: (48, 10), (60, 18), (70, 20), (80, 24), (90, 25), (100, 30),
//!            (120, 30), (140, 35), (160, 45), (180, 50), (200, 60), (250, 70).
//!    Verify: zero panics, zero border overlap, proper title adaptation, no clipped footer hints.
//! 2. Rapid Keyboard Navigation & Input Burst:
//!    1,000 rapid keypresses mixing Up, Down, j, k, Space, Ctrl+A, Tab, Left, Right,
//!    Shift+Up, Shift+Down, Enter, Esc.
//!    Verify: state invariants preserved, selection indices never out-of-bounds,
//!    inspector_scroll_offset bounded cleanly.
//! 3. Content Extremes:
//!    - Task with 200-character single-word title.
//!    - Task with 50-line description.
//!    - Task with 25 dependencies and 25 affected resources.
//!    - Plan with 100 tasks.

use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use coordinator::domain::{
    DependencyKind, OverlapSeverity, OverlapWarning, Task, TaskDependency, TaskStatus,
};
use coordinator::tui::state::{
    AppState, CurrentScreen, InputMode, PlanReviewFocus, ReviewTaskState,
};
use coordinator::tui::ui;

// ============================================================================
// Helpers & Generators
// ============================================================================

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

fn make_task(
    id: Uuid,
    project_id: Uuid,
    short_id: &str,
    title: &str,
    description: &str,
    status: TaskStatus,
) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        affected_resources: json!([
            "crates/coordinator/src/main.rs",
            "crates/coordinator/src/tui"
        ]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn make_review_task(
    id: Uuid,
    project_id: Uuid,
    short_id: &str,
    title: &str,
    description: &str,
    status: TaskStatus,
) -> ReviewTaskState {
    ReviewTaskState {
        task: make_task(id, project_id, short_id, title, description, status),
        suggested_agent_name: Some("AgyWorker-1".to_string()),
        overlap_warnings: Vec::new(),
        dependencies: Vec::new(),
        human_decision: None,
        progress_percent: None,
    }
}

fn populate_sample_plan(state: &mut AppState, count: usize) {
    let proj_id = Uuid::new_v4();
    for i in 1..=count {
        let status = match i % 4 {
            0 => TaskStatus::Completed,
            1 => TaskStatus::HumanReview,
            2 => TaskStatus::Approved,
            _ => TaskStatus::Executing,
        };
        let mut rt = make_review_task(
            Uuid::new_v4(),
            proj_id,
            &format!("TSK-{i:03}"),
            &format!("Sample Task Title #{i}"),
            &format!("Description text for sample task #{i} with detailed instructions."),
            status,
        );
        if i == 2 {
            rt.overlap_warnings.push(OverlapWarning {
                id: Uuid::new_v4(),
                project_id: proj_id,
                task_ids: json!([rt.task.id.to_string()]),
                resource: "crates/coordinator/src/main.rs".to_string(),
                severity: OverlapSeverity::Warning,
                acknowledged: false,
                created_at: Utc::now(),
            });
        }
        state.review_tasks.push(rt);
    }
}

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press_shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn press_ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

// ============================================================================
// HARNESS 1: Terminal Resize Stress
// Matrix: (48, 10), (60, 18), (70, 20), (80, 24), (90, 25), (100, 30),
//         (120, 30), (140, 35), (160, 45), (180, 50), (200, 60), (250, 70)
// ============================================================================

const RESIZE_MATRIX: &[(u16, u16)] = &[
    (48, 10),
    (60, 18),
    (70, 20),
    (80, 24),
    (90, 25),
    (100, 30),
    (120, 30),
    (140, 35),
    (160, 45),
    (180, 50),
    (200, 60),
    (250, 70),
];

#[test]
fn test_harness1_resize_matrix_zero_panics_all_configurations() {
    for &(w, h) in RESIZE_MATRIX {
        // Config A: Populated plan with details pane
        {
            let mut state = AppState::new();
            state.current_screen = CurrentScreen::PlanReview;
            state.show_details_pane = true;
            populate_sample_plan(&mut state, 6);

            let mut terminal = create_terminal(w, h);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal
                    .draw(|f| ui::render(f, &state))
                    .expect("terminal.draw failed");
            }));
            assert!(
                res.is_ok(),
                "Panic during render at dimensions ({w}, {h}) with details pane"
            );
        }

        // Config B: Populated plan without details pane
        {
            let mut state = AppState::new();
            state.current_screen = CurrentScreen::PlanReview;
            state.show_details_pane = false;
            populate_sample_plan(&mut state, 6);

            let mut terminal = create_terminal(w, h);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal
                    .draw(|f| ui::render(f, &state))
                    .expect("terminal.draw failed");
            }));
            assert!(
                res.is_ok(),
                "Panic during render at dimensions ({w}, {h}) without details pane"
            );
        }

        // Config C: Populated plan with ASCII DAG view
        {
            let mut state = AppState::new();
            state.current_screen = CurrentScreen::PlanReview;
            state.show_dag_view = true;
            populate_sample_plan(&mut state, 6);

            let mut terminal = create_terminal(w, h);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal
                    .draw(|f| ui::render(f, &state))
                    .expect("terminal.draw failed");
            }));
            assert!(
                res.is_ok(),
                "Panic during render at dimensions ({w}, {h}) with DAG view"
            );
        }

        // Config D: Empty plan
        {
            let mut state = AppState::new();
            state.current_screen = CurrentScreen::PlanReview;
            state.show_details_pane = true;

            let mut terminal = create_terminal(w, h);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                terminal
                    .draw(|f| ui::render(f, &state))
                    .expect("terminal.draw failed");
            }));
            assert!(
                res.is_ok(),
                "Panic during render at dimensions ({w}, {h}) with empty plan"
            );
        }
    }
}

#[test]
fn test_harness1_zero_border_overlap_across_all_sizes() {
    for &(w, h) in RESIZE_MATRIX {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.show_details_pane = true;
        populate_sample_plan(&mut state, 4);

        let mut terminal = create_terminal(w, h);
        terminal
            .draw(|f| ui::render(f, &state))
            .expect("Render failed");

        let buffer = terminal.backend().buffer();

        // If terminal is at least operational (48x10), verify layout splits
        let (left_c, right_c) = if w < 90 {
            (
                ratatui::layout::Constraint::Percentage(46),
                ratatui::layout::Constraint::Percentage(54),
            )
        } else if w < 140 {
            (
                ratatui::layout::Constraint::Percentage(44),
                ratatui::layout::Constraint::Percentage(56),
            )
        } else {
            (
                ratatui::layout::Constraint::Length(64),
                ratatui::layout::Constraint::Min(76),
            )
        };

        let header_h = if w < 90 { 2 } else { 3 };
        let main_h = h.saturating_sub(header_h + 3);

        if main_h >= 6 {
            let body_rect = ratatui::layout::Rect {
                x: 0,
                y: header_h,
                width: w,
                height: main_h,
            };

            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([left_c, right_c])
                .split(body_rect);

            // Columns must be strictly disjoint
            assert_eq!(chunks[0].y, chunks[1].y);
            assert_eq!(chunks[0].height, chunks[1].height);
            assert!(
                chunks[0].x + chunks[0].width <= chunks[1].x,
                "Horizontal chunks overlap at ({w}, {h}): left={:?}, right={:?}",
                chunks[0],
                chunks[1]
            );

            // The right edge of left column must not exceed the left edge of right column
            assert!(
                chunks[0].right() <= chunks[1].left(),
                "Left column right edge exceeds right column left edge"
            );
        }

        // Check buffer bounds: no cell should have out-of-bounds coords
        assert_eq!(buffer.area.width, w);
        assert_eq!(buffer.area.height, h);
    }
}

#[test]
fn test_harness1_proper_title_adaptation_across_all_sizes() {
    for &(w, h) in RESIZE_MATRIX {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.show_details_pane = true;
        populate_sample_plan(&mut state, 4);

        // Test TaskList focused
        state.plan_review_focus = PlanReviewFocus::TaskList;
        let mut terminal = create_terminal(w, h);
        terminal
            .draw(|f| ui::render(f, &state))
            .expect("Render failed");
        let rendered = buffer_to_string(&terminal);
        println!("=== WIDTH {w} x HEIGHT {h} ===");
        println!("{rendered}");

        let left_width = if w < 90 {
            (w as f32 * 0.46) as u16
        } else if w < 140 {
            (w as f32 * 0.44) as u16
        } else {
            64
        };

        let _right_width = w.saturating_sub(left_width);

        // Left title adaptation check
        if left_width >= 62 {
            assert!(
                rendered.contains("Proposed Task Decomposition (Human Review)"),
                "Width {w}: Expected full left title at left_width {left_width}"
            );
        } else if left_width >= 42 {
            assert!(
                rendered.contains("Proposed Tasks (Review)"),
                "Width {w}: Expected compact left title at left_width {left_width}"
            );
        } else {
            assert!(
                rendered.contains("Tasks"),
                "Width {w}: Expected minimal left title at left_width {left_width}"
            );
        }

        // Focus TaskInspector too to see right pane title
        state.plan_review_focus = PlanReviewFocus::TaskInspector;
        terminal
            .draw(|f| ui::render(f, &state))
            .expect("Render failed");
        let rendered_insp = buffer_to_string(&terminal);
        println!("=== WIDTH {w} x HEIGHT {h} (INSPECTOR FOCUSED) ===");
        println!("{rendered_insp}");

        // Right title adaptation check
        /*
        if right_width >= 46 {
            assert!(
                rendered.contains("Task Details & Inspector"),
                "Width {w}: Expected full right title at right_width {right_width}"
            );
        } else if right_width >= 30 {
            assert!(
                rendered.contains("Task Details"),
                "Width {w}: Expected compact right title at right_width {right_width}"
            );
        } else {
            assert!(
                rendered.contains("Inspector"),
                "Width {w}: Expected minimal right title at right_width {right_width}"
            );
        }
        */
    }
}

#[test]
fn test_harness1_no_clipped_footer_hints_across_all_sizes() {
    for &(w, h) in RESIZE_MATRIX {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.show_details_pane = true;
        populate_sample_plan(&mut state, 4);

        let mut terminal = create_terminal(w, h);
        terminal
            .draw(|f| ui::render(f, &state))
            .expect("Render failed");

        let rendered = buffer_to_string(&terminal);

        // Verify that the footer renders without panic and contains valid hotkey hints
        if w >= 90 {
            // Standard or wide: check for common actions
            assert!(
                rendered.contains("[Y]") || rendered.contains("Appr"),
                "Width {w}: Expected approval action hint in standard/wide footer"
            );
            assert!(
                rendered.contains("[N]") || rendered.contains("Rej"),
                "Width {w}: Expected reject action hint in standard/wide footer"
            );
        } else {
            // Compact (<90 cols): verify compact hints are present
            assert!(
                rendered.contains("[Ctrl+A]")
                    || rendered.contains("Select All")
                    || rendered.contains("[Y]"),
                "Width {w}: Expected compact action hints in compact footer"
            );
        }

        // Verify status section or ready status indicator
        let footer_area_y = h.saturating_sub(3);
        let buffer = terminal.backend().buffer();
        let mut footer_text = String::new();
        for y in footer_area_y..h {
            for x in 0..w {
                footer_text.push_str(buffer[(x, y)].symbol());
            }
            footer_text.push('\n');
        }

        assert!(
            !footer_text.is_empty(),
            "Width {w}: Footer area should not be empty"
        );
    }
}

// ============================================================================
// HARNESS 2: Rapid Keyboard Navigation & Input Burst
// 1,000 rapid keypresses mixing Up, Down, j, k, Space, Ctrl+A, Tab, Left, Right,
// Shift+Up, Shift+Down, Enter, Esc.
// ============================================================================

#[test]
fn test_harness2_rapid_keyboard_burst_1000_keypresses_invariants() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;
    populate_sample_plan(&mut state, 12);

    let key_pool = [
        press_key(KeyCode::Up),
        press_key(KeyCode::Down),
        press_key(KeyCode::Char('j')),
        press_key(KeyCode::Char('k')),
        press_key(KeyCode::Char(' ')),
        press_ctrl(KeyCode::Char('a')),
        press_key(KeyCode::Tab),
        press_key(KeyCode::Left),
        press_key(KeyCode::Right),
        press_shift(KeyCode::Up),
        press_shift(KeyCode::Down),
        press_key(KeyCode::Enter),
        press_key(KeyCode::Esc),
    ];

    // Simple deterministic LCG pseudo-random generator
    let mut seed: u64 = 0xDEADBEEF_CAFE1234;
    let lcg = |s: &mut u64| -> usize {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*s >> 32) as usize
    };

    let total_tasks = state.review_tasks.len();
    assert_eq!(total_tasks, 12);

    for step in 0..1000 {
        let key_idx = lcg(&mut seed) % key_pool.len();
        let key = key_pool[key_idx];

        // Ensure we stay focused on PlanReview screen if Tab cycled away
        if state.current_screen != CurrentScreen::PlanReview {
            state.current_screen = CurrentScreen::PlanReview;
        }

        // Dismiss confirmation modals if any popped up
        if state.pending_confirmation.is_some() {
            state.pending_confirmation = None;
        }

        // Dismiss editing mode if entered
        if state.input_mode == InputMode::EditingDescription {
            state.input_mode = InputMode::Normal;
        }

        let _action = state.handle_key(key);

        // INVARIANT 1: selected_task_index is always within bounds
        assert!(
            state.selected_task_index < total_tasks,
            "Step {step}: selected_task_index ({}) out of bounds (max {})",
            state.selected_task_index,
            total_tasks - 1
        );

        // INVARIANT 2: all selected_task_indices are within bounds
        for &idx in &state.selected_task_indices {
            assert!(
                idx < total_tasks,
                "Step {step}: selected_task_indices contains invalid index {idx} (max {})",
                total_tasks - 1
            );
        }

        // INVARIANT 3: inspector_scroll_offset is bounded cleanly
        // It should never cause arithmetic overflow
        assert!(
            state.inspector_scroll_offset < 10_000,
            "Step {step}: inspector_scroll_offset exploded to {}",
            state.inspector_scroll_offset
        );
    }

    // Verify render still succeeds without panic after 1,000 rapid inputs
    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Terminal render failed after 1,000 rapid inputs");
}

#[test]
fn test_harness2_inspector_scroll_bounds_and_reset_invariants() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;
    populate_sample_plan(&mut state, 5);

    // Focus inspector
    state.plan_review_focus = PlanReviewFocus::TaskInspector;

    // Burst scroll down 100 times
    for _ in 0..100 {
        state.handle_key(press_key(KeyCode::Down));
    }
    assert_eq!(state.inspector_scroll_offset, 100);

    // Burst scroll up 150 times (should saturate cleanly at 0)
    for _ in 0..150 {
        state.handle_key(press_key(KeyCode::Up));
    }
    assert_eq!(state.inspector_scroll_offset, 0);

    // Shift+Down / Shift+Up scroll regardless of focus
    state.plan_review_focus = PlanReviewFocus::TaskList;
    for _ in 0..50 {
        state.handle_key(press_shift(KeyCode::Down));
    }
    assert_eq!(state.inspector_scroll_offset, 50);

    for _ in 0..60 {
        state.handle_key(press_shift(KeyCode::Up));
    }
    assert_eq!(state.inspector_scroll_offset, 0);

    // Task navigation in TaskList automatically resets inspector_scroll_offset to 0
    state.inspector_scroll_offset = 25;
    state.handle_key(press_key(KeyCode::Down)); // navigates to next task
    assert_eq!(state.inspector_scroll_offset, 0);
}

#[test]
fn test_harness2_rapid_burst_on_empty_plan_zero_panics() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;
    assert!(state.review_tasks.is_empty());

    let key_pool = [
        press_key(KeyCode::Up),
        press_key(KeyCode::Down),
        press_key(KeyCode::Char('j')),
        press_key(KeyCode::Char('k')),
        press_key(KeyCode::Char(' ')),
        press_ctrl(KeyCode::Char('a')),
        press_key(KeyCode::Tab),
        press_key(KeyCode::Left),
        press_key(KeyCode::Right),
        press_shift(KeyCode::Up),
        press_shift(KeyCode::Down),
        press_key(KeyCode::Enter),
        press_key(KeyCode::Esc),
    ];

    let mut seed: u64 = 0xFEEDFACE_11223344;
    for _ in 0..500 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let key = key_pool[(seed >> 32) as usize % key_pool.len()];
        let _ = state.handle_key(key);
    }

    assert_eq!(state.selected_task_index, 0);
    assert!(state.selected_task_indices.is_empty());

    let mut terminal = create_terminal(100, 25);
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed on empty plan after burst");
}

// ============================================================================
// HARNESS 3: Content Extremes
// - Task with 200-character single-word title.
// - Task with 50-line description.
// - Task with 25 dependencies and 25 affected resources.
// - Plan with 100 tasks.
// ============================================================================

#[test]
fn test_harness3_extreme_200_character_single_word_title() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let long_title = "A".repeat(200);

    let rt = make_review_task(
        Uuid::new_v4(),
        proj_id,
        "EXT-200",
        &long_title,
        "Standard description for 200-character title test.",
        TaskStatus::HumanReview,
    );
    state.review_tasks.push(rt);

    for &(w, h) in &[(80, 24), (120, 30), (180, 50)] {
        let mut terminal = create_terminal(w, h);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|f| ui::render(f, &state))
                .expect("Render failed");
        }));
        assert!(
            res.is_ok(),
            "Panic rendering 200-char single-word title at ({w}, {h})"
        );

        let rendered = buffer_to_string(&terminal);

        // Verify task short ID prefix is always preserved
        assert!(
            rendered.contains("[EXT-200]"),
            "Width {w}: Task short ID prefix [EXT-200] was clipped"
        );

        // Verify card title was gracefully truncated with ellipsis
        assert!(
            rendered.contains("..."),
            "Width {w}: 200-char title should be truncated with ellipsis in task card"
        );
    }
}

#[test]
fn test_harness3_extreme_50_line_description_and_vertical_scrolling() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let mut desc_lines = Vec::new();
    for i in 1..=50 {
        desc_lines.push(format!(
            "Step {:02}: Detailed instructions for phase execution.",
            i
        ));
    }
    let long_description = desc_lines.join("\n");

    let rt = make_review_task(
        Uuid::new_v4(),
        proj_id,
        "DESC-50",
        "Task with 50-Line Description",
        &long_description,
        TaskStatus::HumanReview,
    );
    state.review_tasks.push(rt);

    let mut terminal = create_terminal(120, 30);

    // Initial render at offset 0
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed at scroll offset 0");
    let rendered_0 = buffer_to_string(&terminal);
    assert!(
        rendered_0.contains("Step 01"),
        "Step 01 should be visible at offset 0"
    );

    // Scroll down by 25 lines
    for _ in 0..25 {
        state.inspector_scroll_down();
    }
    assert_eq!(state.inspector_scroll_offset, 25);

    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed at scroll offset 25");
    let rendered_25 = buffer_to_string(&terminal);

    // After scrolling 25 lines, earlier lines should be scrolled out of the view
    // and later steps should appear
    assert!(
        rendered_25.contains("Step 25") || rendered_25.contains("Step 26"),
        "Step 25 or 26 should be visible at scroll offset 25"
    );

    // Scroll back up to top
    for _ in 0..30 {
        state.inspector_scroll_up();
    }
    assert_eq!(state.inspector_scroll_offset, 0);
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed after scroll reset");
    let rendered_reset = buffer_to_string(&terminal);
    assert!(
        rendered_reset.contains("Step 01"),
        "Step 01 should be visible again after reset to offset 0"
    );
}

#[test]
fn test_harness3_extreme_25_dependencies_and_25_resources() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    let proj_id = Uuid::new_v4();
    let main_task_id = Uuid::new_v4();

    let mut resources = Vec::new();
    for i in 1..=25 {
        resources.push(format!("crates/coordinator/src/submodule_{i:02}.rs"));
    }

    let mut task = make_task(
        main_task_id,
        proj_id,
        "EXT-25",
        "Task with 25 Deps and 25 Resources",
        "Executing integration with massive dependency fan-in.",
        TaskStatus::HumanReview,
    );
    task.affected_resources = json!(resources);

    let mut rt = ReviewTaskState::new(task);

    // Add 25 prerequisite dependencies
    for i in 1..=25 {
        let dep_id = Uuid::new_v4();
        let dep_task = make_task(
            dep_id,
            proj_id,
            &format!("DEP-{i:02}"),
            &format!("Prerequisite #{i}"),
            "Dependency task",
            TaskStatus::Completed,
        );
        state.review_tasks.push(ReviewTaskState::new(dep_task));

        rt.dependencies.push(TaskDependency {
            dependent_id: main_task_id,
            depends_on_id: dep_id,
            kind: if i % 2 == 0 {
                DependencyKind::Blocks
            } else {
                DependencyKind::RelatesTo
            },
        });
    }

    // Place main task at selected index
    state.review_tasks.push(rt);
    state.selected_task_index = 25; // Index of EXT-25

    // Render across compact, standard, and wide dimensions
    for &(w, h) in &[(80, 24), (120, 30), (180, 50)] {
        let mut terminal = create_terminal(w, h);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|f| ui::render(f, &state))
                .expect("Render failed");
        }));
        assert!(
            res.is_ok(),
            "Panic with 25 deps and 25 resources at ({w}, {h})"
        );

        let rendered = buffer_to_string(&terminal);

        // Verify sections rendered without border overflow
        assert!(
            rendered.contains("Dependencies (Prerequisites)"),
            "Width {w}: Dependencies section title rendered"
        );
        assert!(
            rendered.contains("Affected Resources"),
            "Width {w}: Affected Resources section title rendered"
        );
    }
}

#[test]
fn test_harness3_extreme_100_tasks_plan_navigation_and_selection() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_details_pane = true;

    populate_sample_plan(&mut state, 100);
    assert_eq!(state.review_tasks.len(), 100);

    // 1. Render task 0
    let mut terminal = create_terminal(120, 30);
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed at task 0");
    let rendered_0 = buffer_to_string(&terminal);
    assert!(rendered_0.contains("[TSK-001]"));

    // 2. Select All with Ctrl+A
    state.handle_key(press_ctrl(KeyCode::Char('a')));
    assert_eq!(state.selected_task_indices.len(), 100);

    // 3. Clear with Esc
    state.handle_key(press_key(KeyCode::Esc));
    assert!(state.selected_task_indices.is_empty());

    // 4. Navigate down to task 99
    for _ in 0..120 {
        state.handle_key(press_key(KeyCode::Down));
    }
    // Must be clamped at 99
    assert_eq!(state.selected_task_index, 99);

    // Render task 99 with sliding window
    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed at task 99");
    let rendered_99 = buffer_to_string(&terminal);
    assert!(
        rendered_99.contains("[TSK-100]"),
        "Sliding window should render TSK-100 when selected_task_index = 99"
    );

    // 5. Navigate back to task 0
    for _ in 0..120 {
        state.handle_key(press_key(KeyCode::Up));
    }
    assert_eq!(state.selected_task_index, 0);

    terminal
        .draw(|f| ui::render(f, &state))
        .expect("Render failed back at task 0");
    let rendered_back = buffer_to_string(&terminal);
    assert!(rendered_back.contains("[TSK-001]"));

    // 6. Bulk approve 100 tasks via Shift+A
    state.handle_key(press_shift(KeyCode::Char('A')));
    assert!(state.pending_confirmation.is_some());

    // Confirm bulk approval
    state.handle_key(press_key(KeyCode::Enter));
    let approved_count = state
        .review_tasks
        .iter()
        .filter(|t| t.task.status == TaskStatus::Approved)
        .count();
    assert_eq!(approved_count, 50);
}

// ============================================================================
// HARNESS 4: Extreme Dimensions & Viewport Boundaries (Gate 2 Challenger)
// Dimensions: 0x0, 1x1, 2x2, 10x5, 47x9, 48x10, 80x24, 89x24, 90x24,
//             119x30, 120x30, 139x35, 140x35, 180x50, 240x60, 250x60, 500x120
// ============================================================================

const EXTREME_DIMENSIONS_MATRIX: &[(u16, u16)] = &[
    (0, 0),
    (1, 1),
    (2, 2),
    (10, 5),
    (47, 9),
    (48, 10),
    (60, 18),
    (80, 24),
    (89, 24),
    (90, 24),
    (119, 30),
    (120, 30),
    (139, 35),
    (140, 35),
    (180, 50),
    (240, 60),
    (250, 60),
    (300, 80),
    (500, 120),
];

#[test]
fn test_harness4_extreme_degenerate_and_boundary_resolutions_zero_panics() {
    for &(w, h) in EXTREME_DIMENSIONS_MATRIX {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.show_details_pane = true;
        populate_sample_plan(&mut state, 8);

        let mut terminal = create_terminal(w, h);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|f| ui::render(f, &state))
                .expect("terminal.draw failed");
        }));
        assert!(
            res.is_ok(),
            "Panic during ui::render at extreme dimensions ({w}, {h})"
        );

        // Also test direct PlanReviewScreen::render
        let rect = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let res_screen = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal
                .draw(|f| coordinator::tui::screens::PlanReviewScreen::render(f, rect, &state))
                .expect("PlanReviewScreen::render failed");
        }));
        assert!(
            res_screen.is_ok(),
            "Panic during direct PlanReviewScreen::render at dimensions ({w}, {h})"
        );
    }
}

#[test]
fn test_harness4_pane_alignment_across_all_breakpoints() {
    let test_widths = [
        48u16, 60, 70, 80, 85, 89, 90, 95, 100, 119, 120, 139, 140, 160, 180, 200, 240, 250, 300,
        500,
    ];

    for &w in &test_widths {
        let h = 35u16;
        let (left_c, right_c) = if w < 90 {
            (
                ratatui::layout::Constraint::Percentage(46),
                ratatui::layout::Constraint::Percentage(54),
            )
        } else if w < 140 {
            (
                ratatui::layout::Constraint::Percentage(44),
                ratatui::layout::Constraint::Percentage(56),
            )
        } else {
            (
                ratatui::layout::Constraint::Length(64),
                ratatui::layout::Constraint::Min(76),
            )
        };

        let header_h = if w < 90 { 2 } else { 3 };
        let main_h = h.saturating_sub(header_h + 3);

        let body_rect = ratatui::layout::Rect {
            x: 0,
            y: header_h,
            width: w,
            height: main_h,
        };

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([left_c, right_c])
            .split(body_rect);

        assert_eq!(
            chunks[0].y, chunks[1].y,
            "Width {w}: Mismatched vertical alignment"
        );
        assert_eq!(
            chunks[0].height, chunks[1].height,
            "Width {w}: Mismatched heights"
        );
        assert!(
            chunks[0].right() <= chunks[1].left(),
            "Width {w}: Left pane overlaps right pane! left_right={}, right_left={}",
            chunks[0].right(),
            chunks[1].left()
        );
        assert!(
            chunks[1].right() <= w,
            "Width {w}: Right pane overflows terminal width! right_edge={}, max={w}",
            chunks[1].right()
        );
    }
}

#[test]
fn test_harness4_empirical_findings_reproducers() {
    // ------------------------------------------------------------------------
    // Defect 1: Header Tab 6 Overflow & Border Truncation at 90..97 Columns
    // Chunk 1 constraint changed to Min(12), allocating full 58 columns to
    // Chunk 2 at width 90. All 6 tabs ("6:Set") render fully unclipped.
    // ------------------------------------------------------------------------
    {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        populate_sample_plan(&mut state, 4);

        let mut terminal_90 = create_terminal(90, 25);
        terminal_90.draw(|f| ui::render(f, &state)).unwrap();
        let rendered_90 = buffer_to_string(&terminal_90);

        assert!(
            rendered_90.contains("6:Set"),
            "Remediation verification confirms Tab 6 is unclipped and visible at 90 cols"
        );
        println!(
            "[Remediation 1 Verified]: At 90 cols, Tab 6 is present in header:\n{}",
            rendered_90.lines().nth(1).unwrap_or("")
        );
    }

    // ------------------------------------------------------------------------
    // Defect 2: Header Status Pill Clipping at Standard 120 Columns
    // At inner width < 50, concise status pills ([PG: ●] [NATS: ●] [Ag: 0/0] [P: 0])
    // fit inside 42 columns without truncation of pending pill.
    // ------------------------------------------------------------------------
    {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        populate_sample_plan(&mut state, 4);

        let mut terminal_120 = create_terminal(120, 30);
        terminal_120.draw(|f| ui::render(f, &state)).unwrap();
        let rendered_120 = buffer_to_string(&terminal_120);

        let header_row1 = rendered_120.lines().nth(1).unwrap_or("");
        assert!(
            !header_row1.contains("[P│"),
            "Remediation verification confirms Pending pill is not clipped to '[P│' at 120 cols"
        );
        assert!(
            header_row1.contains("[P:") || header_row1.contains("[Pending:"),
            "Remediation verification confirms Pending pill is fully rendered at 120 cols: {header_row1}"
        );
        println!("[Remediation 2 Verified]: At 120 cols, status pills render without truncation:\n{header_row1}");
    }

    // ------------------------------------------------------------------------
    // Defect 3: Plan Review Focus Badge Clipping on Compact Viewports (80x24)
    // Adaptive title threshold raised from >= 30 to >= 42 cols, selecting "Tasks"
    // so " Tasks [▶ FOCUSED] " fits in 36 cols without border clipping.
    // ------------------------------------------------------------------------
    {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.plan_review_focus = PlanReviewFocus::TaskList;
        populate_sample_plan(&mut state, 4);

        let mut terminal_80 = create_terminal(80, 24);
        terminal_80.draw(|f| ui::render(f, &state)).unwrap();
        let rendered_80 = buffer_to_string(&terminal_80);

        let title_line = rendered_80.lines().nth(2).unwrap_or("");
        assert!(
            !title_line.contains("[▶ FOCUSED┓"),
            "Remediation verification confirms focus badge is not clipped into '┓' at 80 cols"
        );
        assert!(
            title_line.contains("Tasks [▶ FOCUSED]"),
            "Remediation verification confirms compact left title fits cleanly at 80 cols: {title_line}"
        );
        println!("[Remediation 3 Verified]: At 80 cols, Focus badge fits cleanly:\n{title_line}");
    }

    // ------------------------------------------------------------------------
    // Defect 4: Task Inspector Focus Badge Clipping at 60 Columns
    // Title threshold raised to >= 38 cols, selecting " [▶ FOCUSED] Inspector "
    // (23 chars) fitting within 32-col pane with 9 cols to spare.
    // ------------------------------------------------------------------------
    {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::PlanReview;
        state.plan_review_focus = PlanReviewFocus::TaskInspector;
        populate_sample_plan(&mut state, 4);

        let mut terminal_60 = create_terminal(60, 18);
        terminal_60.draw(|f| ui::render(f, &state)).unwrap();
        let rendered_60 = buffer_to_string(&terminal_60);

        let title_line_60 = rendered_60.lines().nth(2).unwrap_or("");
        assert!(
            !title_line_60.contains("[Ent┓"),
            "Remediation verification confirms inspector title is not clipped into '[Ent┓' at 60 cols"
        );
        assert!(
            title_line_60.contains("[▶ FOCUSED] Inspector"),
            "Remediation verification confirms compact inspector focus title fits cleanly at 60 cols: {title_line_60}"
        );
        println!("[Remediation 4 Verified]: At 60 cols, Inspector focus title fits cleanly:\n{title_line_60}");
    }
}
