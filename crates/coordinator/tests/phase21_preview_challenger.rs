//! Phase 21 TUI Empirical Adversarial Challenge Test Suite
//!
//! Evaluates the 3 mandatory adversarial challenges:
//! 1. Deep DAG Traversal & Complex Topology Challenge:
//!    - Deep linear chain (depth = 6: A -> B -> C -> D -> E -> F)
//!    - Diamond dependency graph (A -> B, A -> C, B -> D, C -> D)
//!    - Multi-root parallel graph (3 distinct roots with independent subtrees)
//!    - Cycle resistance (circular dependency graph A -> B -> A, cycle below root)
//!    - Blocker ('[') and dependent (']') keyboard navigation
//! 2. Destructive Action Confirmation Flow Challenge:
//!    - CancelTask confirmation flow (press 'c', check modal, abort with 'n'/'Esc', confirm with 'y'/'Enter')
//!    - RejectTask confirmation flow (press 'n', check modal, abort with 'n'/'Esc', confirm with 'y'/'Enter')
//! 3. Task Status State Machine Retry Invariants:
//!    - Invariants for TaskStatus::Cancelled transitions

use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use coordinator::domain::{DependencyKind, Task, TaskDependency, TaskStatus};
use coordinator::tui::{
    render, widgets::DagViewWidget, AppState, CurrentScreen, ReviewTaskState, TuiAction,
};

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

fn press_char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn make_task(id: Uuid, project_id: Uuid, short_id: &str, title: &str, status: TaskStatus) -> Task {
    Task {
        id,
        project_id,
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

fn make_review_task(task: Task, dependencies: Vec<TaskDependency>) -> ReviewTaskState {
    let mut r = ReviewTaskState::new(task);
    r.dependencies = dependencies;
    r
}

// ============================================================================
// CHALLENGE 1: Deep DAG Traversal & Complex Topology Challenge
// ============================================================================

#[test]
fn test_challenge1_deep_linear_chain_depth_6_all_nodes_rendered() {
    let mut terminal = create_terminal(120, 60);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_details_pane = false;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();
    let id_d = Uuid::new_v4();
    let id_e = Uuid::new_v4();
    let id_f = Uuid::new_v4();

    // Deep linear chain: A -> B -> C -> D -> E -> F
    let task_a = make_task(
        id_a,
        proj_id,
        "TASK-A",
        "Init Schema",
        TaskStatus::Completed,
    );
    let task_b = make_task(
        id_b,
        proj_id,
        "TASK-B",
        "Setup Migrations",
        TaskStatus::Approved,
    );
    let task_c = make_task(
        id_c,
        proj_id,
        "TASK-C",
        "Build Repository",
        TaskStatus::HumanReview,
    );
    let task_d = make_task(
        id_d,
        proj_id,
        "TASK-D",
        "Implement Service",
        TaskStatus::Proposed,
    );
    let task_e = make_task(
        id_e,
        proj_id,
        "TASK-E",
        "Add Integration Tests",
        TaskStatus::Proposed,
    );
    let task_f = make_task(
        id_f,
        proj_id,
        "TASK-F",
        "Deployment Smoke",
        TaskStatus::Proposed,
    );

    state.review_tasks = vec![
        make_review_task(task_a, vec![]),
        make_review_task(
            task_b,
            vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_c,
            vec![TaskDependency {
                dependent_id: id_c,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_d,
            vec![TaskDependency {
                dependent_id: id_d,
                depends_on_id: id_c,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_e,
            vec![TaskDependency {
                dependent_id: id_e,
                depends_on_id: id_d,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_f,
            vec![TaskDependency {
                dependent_id: id_f,
                depends_on_id: id_e,
                kind: DependencyKind::Blocks,
            }],
        ),
    ];

    // Compute topological tiers
    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert_eq!(
        tiers.len(),
        6,
        "Expected 6 tiers in linear chain of depth 6"
    );
    assert_eq!(tiers[0], vec![id_a]);
    assert_eq!(tiers[1], vec![id_b]);
    assert_eq!(tiers[2], vec![id_c]);
    assert_eq!(tiers[3], vec![id_d]);
    assert_eq!(tiers[4], vec![id_e]);
    assert_eq!(tiers[5], vec![id_f]);

    // Render widget and verify all 6 nodes are visible in buffer
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);

    assert!(buf.contains("TASK-A"), "TASK-A must be rendered in buffer");
    assert!(buf.contains("TASK-B"), "TASK-B must be rendered in buffer");
    assert!(buf.contains("TASK-C"), "TASK-C must be rendered in buffer");
    assert!(buf.contains("TASK-D"), "TASK-D must be rendered in buffer");
    assert!(buf.contains("TASK-E"), "TASK-E must be rendered in buffer");
    assert!(buf.contains("TASK-F"), "TASK-F must be rendered in buffer");

    // Verify 1-hop bug is completely eliminated: depth 5 and 6 are visible
    assert!(buf.contains("Tier 0"), "Tier 0 header must be visible");
    assert!(buf.contains("Tier 5"), "Tier 5 header must be visible");
}

#[test]
fn test_challenge1_diamond_dependency_graph() {
    let mut terminal = create_terminal(120, 50);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_details_pane = false;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();
    let id_d = Uuid::new_v4();

    // Diamond: A -> B, A -> C, B -> D, C -> D
    let task_a = make_task(id_a, proj_id, "TASK-A", "Root Core", TaskStatus::Approved);
    let task_b = make_task(id_b, proj_id, "TASK-B", "Branch B", TaskStatus::HumanReview);
    let task_c = make_task(id_c, proj_id, "TASK-C", "Branch C", TaskStatus::HumanReview);
    let task_d = make_task(id_d, proj_id, "TASK-D", "Join Sink D", TaskStatus::Proposed);

    state.review_tasks = vec![
        make_review_task(task_a, vec![]),
        make_review_task(
            task_b,
            vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_c,
            vec![TaskDependency {
                dependent_id: id_c,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            task_d,
            vec![
                TaskDependency {
                    dependent_id: id_d,
                    depends_on_id: id_b,
                    kind: DependencyKind::Blocks,
                },
                TaskDependency {
                    dependent_id: id_d,
                    depends_on_id: id_c,
                    kind: DependencyKind::Blocks,
                },
            ],
        ),
    ];

    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert_eq!(tiers.len(), 3, "Diamond must have 3 execution tiers");
    assert_eq!(tiers[0], vec![id_a]); // Tier 0
    assert_eq!(tiers[1].len(), 2); // Tier 1: B and C
    assert!(tiers[1].contains(&id_b));
    assert!(tiers[1].contains(&id_c));
    assert_eq!(tiers[2], vec![id_d]); // Tier 2: D waits on both B and C

    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("TASK-A"));
    assert!(buf.contains("TASK-B"));
    assert!(buf.contains("TASK-C"));
    assert!(buf.contains("TASK-D"));
}

#[test]
fn test_challenge1_multi_root_parallel_graph() {
    let mut terminal = create_terminal(120, 50);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_details_pane = false;

    let proj_id = Uuid::new_v4();
    let id_r1 = Uuid::new_v4();
    let id_c1 = Uuid::new_v4();
    let id_r2 = Uuid::new_v4();
    let id_c2 = Uuid::new_v4();
    let id_r3 = Uuid::new_v4();
    let id_c3 = Uuid::new_v4();

    // 3 parallel root trees: R1 -> C1, R2 -> C2, R3 -> C3
    state.review_tasks = vec![
        make_review_task(
            make_task(
                id_r1,
                proj_id,
                "ROOT-1",
                "Backend Root",
                TaskStatus::Approved,
            ),
            vec![],
        ),
        make_review_task(
            make_task(
                id_c1,
                proj_id,
                "CHILD-1",
                "Backend Child",
                TaskStatus::Proposed,
            ),
            vec![TaskDependency {
                dependent_id: id_c1,
                depends_on_id: id_r1,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            make_task(
                id_r2,
                proj_id,
                "ROOT-2",
                "Frontend Root",
                TaskStatus::Approved,
            ),
            vec![],
        ),
        make_review_task(
            make_task(
                id_c2,
                proj_id,
                "CHILD-2",
                "Frontend Child",
                TaskStatus::Proposed,
            ),
            vec![TaskDependency {
                dependent_id: id_c2,
                depends_on_id: id_r2,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            make_task(
                id_r3,
                proj_id,
                "ROOT-3",
                "DevOps Root",
                TaskStatus::Approved,
            ),
            vec![],
        ),
        make_review_task(
            make_task(
                id_c3,
                proj_id,
                "CHILD-3",
                "DevOps Child",
                TaskStatus::Proposed,
            ),
            vec![TaskDependency {
                dependent_id: id_c3,
                depends_on_id: id_r3,
                kind: DependencyKind::Blocks,
            }],
        ),
    ];

    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert_eq!(tiers.len(), 2, "Multi-root parallel graph has 2 tiers");
    assert_eq!(tiers[0].len(), 3, "Tier 0 must contain 3 roots");
    assert!(tiers[0].contains(&id_r1));
    assert!(tiers[0].contains(&id_r2));
    assert!(tiers[0].contains(&id_r3));

    assert_eq!(tiers[1].len(), 3, "Tier 1 must contain 3 children");
    assert!(tiers[1].contains(&id_c1));
    assert!(tiers[1].contains(&id_c2));
    assert!(tiers[1].contains(&id_c3));

    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("3 Root(s) Ready"));
    assert!(buf.contains("ROOT-1"));
    assert!(buf.contains("ROOT-2"));
    assert!(buf.contains("ROOT-3"));
    assert!(buf.contains("CHILD-1"));
    assert!(buf.contains("CHILD-2"));
    assert!(buf.contains("CHILD-3"));
}

#[test]
fn test_challenge1_cycle_resistance_circular_dependency() {
    let mut terminal = create_terminal(120, 40);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_details_pane = false;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    // Circular dependency: A -> B and B -> A
    state.review_tasks = vec![
        make_review_task(
            make_task(
                id_a,
                proj_id,
                "CYCLE-A",
                "Cycle Node A",
                TaskStatus::HumanReview,
            ),
            vec![TaskDependency {
                dependent_id: id_a,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            make_task(
                id_b,
                proj_id,
                "CYCLE-B",
                "Cycle Node B",
                TaskStatus::HumanReview,
            ),
            vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
    ];

    // Must terminate safely without infinite loop or stack overflow
    let tiers = DagViewWidget::compute_topological_tiers(&state);
    assert!(
        !tiers.is_empty(),
        "Circular dependency must still produce partitioned tiers"
    );
    assert_eq!(tiers.iter().map(|t| t.len()).sum::<usize>(), 2);

    // Terminal render must not stack overflow
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("CYCLE-A"));
    assert!(buf.contains("CYCLE-B"));
}

#[test]
fn test_challenge1_cycle_below_root_resistance() {
    let mut terminal = create_terminal(120, 40);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;
    state.show_details_pane = false;

    let proj_id = Uuid::new_v4();
    let id_r = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    // Root R -> A -> B -> A (cycle below root)
    state.review_tasks = vec![
        make_review_task(
            make_task(id_r, proj_id, "ROOT-0", "Valid Root", TaskStatus::Approved),
            vec![],
        ),
        make_review_task(
            make_task(
                id_a,
                proj_id,
                "CYCLE-A",
                "Cycle Node A",
                TaskStatus::Proposed,
            ),
            vec![
                TaskDependency {
                    dependent_id: id_a,
                    depends_on_id: id_r,
                    kind: DependencyKind::Blocks,
                },
                TaskDependency {
                    dependent_id: id_a,
                    depends_on_id: id_b,
                    kind: DependencyKind::Blocks,
                },
            ],
        ),
        make_review_task(
            make_task(
                id_b,
                proj_id,
                "CYCLE-B",
                "Cycle Node B",
                TaskStatus::Proposed,
            ),
            vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
    ];

    // Render must detect cycle and terminate cleanly
    terminal.draw(|f| render(f, &state)).unwrap();
    let buf = buffer_to_string(&terminal);
    assert!(buf.contains("ROOT-0"));
    assert!(buf.contains("Cycle/Repeated Reference Detected") || buf.contains("CYCLE-A"));
}

#[test]
fn test_challenge1_blocker_and_dependent_navigation() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;
    state.show_dag_view = true;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_c = Uuid::new_v4();

    // A (idx 0) -> B (idx 1) -> C (idx 2)
    state.review_tasks = vec![
        make_review_task(
            make_task(
                id_a,
                proj_id,
                "PLAN-A",
                "Prerequisite A",
                TaskStatus::Approved,
            ),
            vec![],
        ),
        make_review_task(
            make_task(id_b, proj_id, "PLAN-B", "Middle B", TaskStatus::Approved),
            vec![TaskDependency {
                dependent_id: id_b,
                depends_on_id: id_a,
                kind: DependencyKind::Blocks,
            }],
        ),
        make_review_task(
            make_task(id_c, proj_id, "PLAN-C", "Leaf C", TaskStatus::Approved),
            vec![TaskDependency {
                dependent_id: id_c,
                depends_on_id: id_b,
                kind: DependencyKind::Blocks,
            }],
        ),
    ];

    // Start on C (index 2)
    state.selected_task_index = 2;

    // Press '[' to jump to prerequisite B (index 1)
    let action1 = state.handle_key(press_char('['));
    assert_eq!(action1, None);
    assert_eq!(
        state.selected_task_index, 1,
        "Expected jump from C (2) to prerequisite B (1)"
    );

    // Press '[' again to jump to prerequisite A (index 0)
    let action2 = state.handle_key(press_char('['));
    assert_eq!(action2, None);
    assert_eq!(
        state.selected_task_index, 0,
        "Expected jump from B (1) to prerequisite A (0)"
    );

    // Press '[' on root A -> no blocker, should be safe no-op
    let action3 = state.handle_key(press_char('['));
    assert_eq!(action3, None);
    assert_eq!(
        state.selected_task_index, 0,
        "Root task has no blockers, must remain at 0"
    );

    // Press ']' to jump to downstream dependent B (index 1)
    let action4 = state.handle_key(press_char(']'));
    assert_eq!(action4, None);
    assert_eq!(
        state.selected_task_index, 1,
        "Expected jump from A (0) to downstream dependent B (1)"
    );

    // Press ']' again to jump to downstream dependent C (index 2)
    let action5 = state.handle_key(press_char(']'));
    assert_eq!(action5, None);
    assert_eq!(
        state.selected_task_index, 2,
        "Expected jump from B (1) to downstream dependent C (2)"
    );

    // Press ']' on leaf C -> no downstream dependents, safe no-op
    let action6 = state.handle_key(press_char(']'));
    assert_eq!(action6, None);
    assert_eq!(
        state.selected_task_index, 2,
        "Leaf task has no downstream dependents, must remain at 2"
    );
}

// ============================================================================
// CHALLENGE 2: Destructive Action Confirmation Flow Challenge
// ============================================================================

/// Direct keystroke challenge for CancelTask:
/// Tests what happens when the user presses 'c' in PlanReview screen.
/// Mandate: "press 'c', assert confirmation modal is shown, press 'n' or 'Esc', assert NO CancelTask emitted.
/// Press 'c', assert confirmation modal is shown, press 'y' or 'Enter', assert CancelTask IS emitted."
#[test]
fn test_challenge2_keystroke_c_canceltask_modal_flow() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    state.review_tasks = vec![make_review_task(
        make_task(
            id_a,
            proj_id,
            "TASK-1",
            "Sample Task",
            TaskStatus::Executing,
        ),
        vec![],
    )];
    state.selected_task_index = 0;

    // STEP 1: Press 'c' to request task cancellation
    let action_on_c = state.handle_key(press_char('c'));

    // EMPIRICAL OBSERVATION:
    // If confirmation modal is wired:
    //   - pending_confirmation must be Some(...)
    //   - action_on_c must be None (no destructive emission before confirmation)
    // If confirmation modal is NOT wired:
    //   - pending_confirmation is None
    //   - action_on_c is Some(TuiAction::CancelTask) immediately!
    println!("[EMPIRICAL] Action returned on 'c': {:?}", action_on_c);
    println!(
        "[EMPIRICAL] pending_confirmation on 'c': {:?}",
        state.pending_confirmation
    );

    let modal_opened = state.pending_confirmation.is_some();
    let action_deferred = action_on_c.is_none();

    assert!(
        modal_opened && action_deferred,
        "CRITICAL BUG / CONTRACT VIOLATION: Pressing 'c' did NOT show confirmation modal! \
         Instead, pending_confirmation={:?} and action={:?}. \
         Expected modal to be open and action deferred.",
        state.pending_confirmation,
        action_on_c
    );
}

/// Direct keystroke challenge for RejectTask:
/// Tests what happens when the user presses 'n' in PlanReview screen.
/// Mandate: "Test RejectTask flow similarly with 'n'."
#[test]
fn test_challenge2_keystroke_n_rejecttask_modal_flow() {
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::PlanReview;

    let proj_id = Uuid::new_v4();
    let id_a = Uuid::new_v4();
    state.review_tasks = vec![make_review_task(
        make_task(
            id_a,
            proj_id,
            "TASK-1",
            "Sample Task",
            TaskStatus::HumanReview,
        ),
        vec![],
    )];
    state.selected_task_index = 0;

    // STEP 1: Press 'n' to reject task
    let action_on_n = state.handle_key(press_char('n'));

    println!("[EMPIRICAL] Action returned on 'n': {:?}", action_on_n);
    println!(
        "[EMPIRICAL] pending_confirmation on 'n': {:?}",
        state.pending_confirmation
    );

    let modal_opened = state.pending_confirmation.is_some();
    let action_deferred = action_on_n.is_none();

    assert!(
        modal_opened && action_deferred,
        "CRITICAL BUG / CONTRACT VIOLATION: Pressing 'n' did NOT show confirmation modal! \
         Instead, pending_confirmation={:?} and action={:?}. \
         Expected modal to be open and action deferred.",
        state.pending_confirmation,
        action_on_n
    );
}

/// Tests the confirmation modal state machine logic when explicitly invoked via prompt_cancel_task
#[test]
fn test_challenge2_explicit_prompt_cancel_flow_abort_with_n_and_esc() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    state.review_tasks = vec![make_review_task(
        make_task(
            task_id,
            proj_id,
            "TASK-1",
            "Sample Task",
            TaskStatus::Executing,
        ),
        vec![],
    )];
    state.selected_task_index = 0;

    // Trigger confirmation modal
    state.prompt_cancel_task(task_id);
    assert!(
        state.pending_confirmation.is_some(),
        "Confirmation modal must be open"
    );

    // Press 'n': aborts action
    let action = state.handle_key(press_char('n'));
    assert_eq!(action, None, "Aborting with 'n' must NOT emit action");
    assert!(
        state.pending_confirmation.is_none(),
        "Modal must be closed after 'n'"
    );

    // Trigger again and abort with 'Esc'
    state.prompt_cancel_task(task_id);
    assert!(state.pending_confirmation.is_some());
    let action_esc = state.handle_key(press_key(KeyCode::Esc));
    assert_eq!(action_esc, None, "Aborting with 'Esc' must NOT emit action");
    assert!(
        state.pending_confirmation.is_none(),
        "Modal must be closed after 'Esc'"
    );
}

#[test]
fn test_challenge2_explicit_prompt_cancel_flow_confirm_with_y_and_enter() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    state.review_tasks = vec![make_review_task(
        make_task(
            task_id,
            proj_id,
            "TASK-1",
            "Sample Task",
            TaskStatus::Executing,
        ),
        vec![],
    )];
    state.selected_task_index = 0;

    // Trigger and confirm with 'y'
    state.prompt_cancel_task(task_id);
    let action_y = state.handle_key(press_char('y'));
    assert_eq!(
        action_y,
        Some(TuiAction::CancelTask { task_id }),
        "Confirming with 'y' must emit CancelTask action"
    );
    assert!(
        state.pending_confirmation.is_none(),
        "Modal must close on confirmation"
    );

    // Reset task status and confirm with Enter
    state.review_tasks[0].task.status = TaskStatus::Executing;
    state.prompt_cancel_task(task_id);
    let action_enter = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(
        action_enter,
        Some(TuiAction::CancelTask { task_id }),
        "Confirming with Enter must emit CancelTask action"
    );
    assert!(
        state.pending_confirmation.is_none(),
        "Modal must close on confirmation"
    );
}

#[test]
fn test_challenge2_explicit_prompt_reject_flow_abort_and_confirm() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    state.review_tasks = vec![make_review_task(
        make_task(
            task_id,
            proj_id,
            "TASK-1",
            "Sample Task",
            TaskStatus::HumanReview,
        ),
        vec![],
    )];
    state.selected_task_index = 0;

    // Abort with 'n'
    state.prompt_reject_task(task_id);
    assert!(state.pending_confirmation.is_some());
    let action_abort = state.handle_key(press_char('n'));
    assert_eq!(action_abort, None, "Aborting reject must NOT emit action");
    assert!(state.pending_confirmation.is_none());

    // Confirm with 'y'
    state.prompt_reject_task(task_id);
    let action_confirm = state.handle_key(press_char('y'));
    assert_eq!(
        action_confirm,
        Some(TuiAction::RejectTask { task_id }),
        "Confirming reject with 'y' must emit RejectTask action"
    );
    assert!(state.pending_confirmation.is_none());
}

// ============================================================================
// CHALLENGE 3: Task Status State Machine Retry Invariants
// ============================================================================

#[test]
fn test_challenge3_task_status_retry_invariants_for_cancelled() {
    use TaskStatus::*;

    // REQUIRED INVARIANTS:
    // (Cancelled, Approved) -> true
    assert!(
        Cancelled.can_transition_to(Approved),
        "Cancelled -> Approved must be permitted for task retry"
    );

    // (Cancelled, HumanReview) -> true
    assert!(
        Cancelled.can_transition_to(HumanReview),
        "Cancelled -> HumanReview must be permitted for human re-review"
    );

    // (Cancelled, Executing) -> false
    assert!(
        !Cancelled.can_transition_to(Executing),
        "Cancelled -> Executing must NOT be permitted directly without approval"
    );

    // (Cancelled, Completed) -> false
    assert!(
        !Cancelled.can_transition_to(Completed),
        "Cancelled -> Completed must NOT be permitted"
    );

    // Exhaustive check of all other target states from Cancelled
    assert!(
        !Cancelled.can_transition_to(Proposed),
        "Cancelled -> Proposed must be false"
    );
    assert!(
        !Cancelled.can_transition_to(Rejected),
        "Cancelled -> Rejected must be false"
    );
    assert!(
        !Cancelled.can_transition_to(Assigned),
        "Cancelled -> Assigned must be false"
    );
    assert!(
        !Cancelled.can_transition_to(Blocked),
        "Cancelled -> Blocked must be false"
    );
    assert!(
        !Cancelled.can_transition_to(Failed),
        "Cancelled -> Failed must be false"
    );
    assert!(
        !Cancelled.can_transition_to(Cancelled),
        "Cancelled -> Cancelled must be false"
    );
}
