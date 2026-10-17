use std::time::Instant;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use uuid::Uuid;

use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render, AppState, ConfirmationModalState, CurrentScreen, TuiUpdateEvent,
    MAX_RECENT_EVENTS,
};

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to create TestBackend terminal")
}

fn sample_event(i: usize) -> CoordinatorEvent {
    CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: match i % 4 {
            0 => "task.started".to_string(),
            1 => "task.progress".to_string(),
            2 => "task.failed".to_string(),
            _ => "agent.heartbeat".to_string(),
        },
        project_id: Some(Uuid::new_v4()),
        task_id: Some(Uuid::new_v4()),
        agent_id: Some(Uuid::new_v4()),
        message: format!("Test event message {i}"),
        payload: json!({ "counter": i }),
    }
}

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn make_ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

// ============================================================================
// CHALLENGE 1: Event Buffer Burst Stress & Capacity Invariant
// ============================================================================

#[test]
fn test_challenge1_push_recent_event_burst_1k() {
    let mut state = AppState::new();
    let start = Instant::now();

    for i in 0..1_000 {
        state.push_recent_event(sample_event(i));
        assert!(
            state.recent_events.len() <= MAX_RECENT_EVENTS,
            "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
            state.recent_events.len()
        );
        assert!(
            state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
            "state.event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
            state.event_ring_buffer.len()
        );
    }

    let elapsed = start.elapsed();
    assert_eq!(state.event_ring_buffer.len(), MAX_RECENT_EVENTS);
    assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
    println!("Challenge 1: push_recent_event 1,000 events took: {elapsed:?}");
}

#[test]
fn test_challenge1_push_recent_event_burst_5k() {
    let mut state = AppState::new();
    let start = Instant::now();

    for i in 0..5_000 {
        state.push_recent_event(sample_event(i));
        assert!(
            state.recent_events.len() <= MAX_RECENT_EVENTS,
            "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
            state.recent_events.len()
        );
        assert!(
            state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
            "state.event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
            state.event_ring_buffer.len()
        );
    }

    let elapsed = start.elapsed();
    assert_eq!(state.event_ring_buffer.len(), MAX_RECENT_EVENTS);
    assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
    println!("Challenge 1: push_recent_event 5,000 events took: {elapsed:?}");
}

#[test]
fn test_challenge1_push_recent_event_burst_10k() {
    let mut state = AppState::new();
    let start = Instant::now();

    for i in 0..10_000 {
        state.push_recent_event(sample_event(i));
        assert!(
            state.recent_events.len() <= MAX_RECENT_EVENTS,
            "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
            state.recent_events.len()
        );
        assert!(
            state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
            "state.event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
            state.event_ring_buffer.len()
        );
    }

    let elapsed = start.elapsed();
    assert_eq!(state.event_ring_buffer.len(), MAX_RECENT_EVENTS);
    assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
    println!("Challenge 1: push_recent_event 10,000 events took: {elapsed:?}");
    // Performance assertion: 10,000 events should complete in well under 500ms
    assert!(elapsed.as_millis() < 500, "10k burst exceeded performance limit: {elapsed:?}");
}

#[test]
fn test_challenge1_update_event_recent_events_batch_1k() {
    let mut state = AppState::new();
    let batch: Vec<CoordinatorEvent> = (0..1_000).map(sample_event).collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(batch));

    assert!(
        state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
        "event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
        state.event_ring_buffer.len()
    );
    assert!(
        state.recent_events.len() <= MAX_RECENT_EVENTS,
        "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
        state.recent_events.len()
    );
}

#[test]
fn test_challenge1_update_event_recent_events_batch_5k() {
    let mut state = AppState::new();
    let batch: Vec<CoordinatorEvent> = (0..5_000).map(sample_event).collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(batch));

    assert!(
        state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
        "event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
        state.event_ring_buffer.len()
    );
    assert!(
        state.recent_events.len() <= MAX_RECENT_EVENTS,
        "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
        state.recent_events.len()
    );
}

#[test]
fn test_challenge1_update_event_recent_events_batch_10k() {
    let mut state = AppState::new();
    let batch: Vec<CoordinatorEvent> = (0..10_000).map(sample_event).collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(batch));

    assert!(
        state.event_ring_buffer.len() <= MAX_RECENT_EVENTS,
        "event_ring_buffer exceeded MAX_RECENT_EVENTS: len={}",
        state.event_ring_buffer.len()
    );
    assert!(
        state.recent_events.len() <= MAX_RECENT_EVENTS,
        "state.recent_events exceeded MAX_RECENT_EVENTS: len={}",
        state.recent_events.len()
    );
}

#[test]
fn test_challenge1_fifo_eviction_and_newest_ordering() {
    let mut state = AppState::new();
    for i in 0..500 {
        state.push_recent_event(sample_event(i));
    }

    assert_eq!(state.event_ring_buffer.len(), MAX_RECENT_EVENTS);
    // Newest event (index 499) at front
    assert_eq!(
        state.event_ring_buffer.front().unwrap().message,
        "Test event message 499"
    );
    // Oldest retained event (index 200) at back (500 - 300 = 200)
    assert_eq!(
        state.event_ring_buffer.back().unwrap().message,
        "Test event message 200"
    );
}

// ============================================================================
// CHALLENGE 2: Terminal Resize Matrix Stress (7 Dimensions x 5 Screens + Modals)
// ============================================================================

const RESIZE_DIMENSIONS: [(u16, u16, &str); 7] = [
    (47, 9, "extreme_boundary_too_small"),
    (60, 18, "compact"),
    (70, 20, "small"),
    (80, 24, "standard"),
    (100, 30, "medium"),
    (160, 50, "large"),
    (250, 80, "ultra_wide"),
];

const SCREENS: [CurrentScreen; 5] = [
    CurrentScreen::Dashboard,
    CurrentScreen::ProjectManagement,
    CurrentScreen::PlanReview,
    CurrentScreen::FleetInspector,
    CurrentScreen::Diagnostics,
];

#[test]
fn test_challenge2_extreme_size_boundary_47x9_renders_clean_message_zero_panics() {
    let mut terminal = create_terminal(47, 9);
    let state = AppState::new().with_mock_data();

    // Must render cleanly without panic
    terminal.draw(|f| render(f, &state)).expect("Render failed at 47x9");

    let text = buffer_to_string(&terminal);
    assert!(
        text.contains("Terminal too small"),
        "Expected 'Terminal too small' message at 47x9, got:\n{text}"
    );
    assert!(
        text.contains("47x9"),
        "Expected dimension text '47x9' at 47x9, got:\n{text}"
    );
    assert!(
        text.contains("min 60x18"),
        "Expected 'min 60x18' hint at 47x9, got:\n{text}"
    );
}

#[test]
fn test_challenge2_matrix_across_all_dimensions_and_screens() {
    for (width, height, label) in RESIZE_DIMENSIONS {
        for screen in SCREENS {
            let mut terminal = create_terminal(width, height);
            let mut state = AppState::new().with_mock_data();
            state.current_screen = screen;

            terminal.draw(|f| render(f, &state))
                .unwrap_or_else(|e| panic!("Draw failed on {width}x{height} ({label}) for screen {screen:?}: {e}"));

            let text = buffer_to_string(&terminal);
            if width < 48 || height < 10 {
                assert!(
                    text.contains("Terminal too small"),
                    "Dimension {width}x{height} below bounds must show guard message"
                );
            } else {
                // Must render screen content without error
                assert!(
                    !text.trim().is_empty(),
                    "Rendered buffer is empty on {width}x{height} ({label}) for screen {screen:?}"
                );
            }
        }
    }
}

#[test]
fn test_challenge2_matrix_with_active_modals() {
    for (width, height, label) in RESIZE_DIMENSIONS {
        // Modal 1: Help Modal
        {
            let mut terminal = create_terminal(width, height);
            let mut state = AppState::new().with_mock_data();
            state.show_help_modal = true;

            terminal.draw(|f| render(f, &state))
                .unwrap_or_else(|e| panic!("Panic with Help Modal open on {width}x{height} ({label}): {e}"));
        }

        // Modal 2: Command Palette Modal
        {
            let mut terminal = create_terminal(width, height);
            let mut state = AppState::new().with_mock_data();
            state.show_command_palette = true;
            state.command_palette_query = "task".to_string();

            terminal.draw(|f| render(f, &state))
                .unwrap_or_else(|e| panic!("Panic with Command Palette open on {width}x{height} ({label}): {e}"));
        }

        // Modal 3: Confirmation Dialog Modal
        {
            let mut terminal = create_terminal(width, height);
            let mut state = AppState::new().with_mock_data();
            state.pending_confirmation = Some(ConfirmationModalState::cancel(
                Uuid::new_v4(),
                "TASK-101",
            ));

            terminal.draw(|f| render(f, &state))
                .unwrap_or_else(|e| panic!("Panic with Confirmation Modal open on {width}x{height} ({label}): {e}"));
        }
    }
}

// ============================================================================
// CHALLENGE 3: Command Palette Modal Fuzzy / Symbol Stress
// ============================================================================

#[test]
fn test_challenge3_empty_query_filtering_and_rendering() {
    let mut state = AppState::new().with_mock_data();
    state.show_command_palette = true;
    state.command_palette_query = "".to_string();
    state.command_palette_selected_index = 0;

    let mut terminal = create_terminal(80, 24);
    terminal.draw(|f| render(f, &state)).expect("Render failed on empty query");

    let text = buffer_to_string(&terminal);
    assert!(text.contains("Command Palette"), "Palette header missing");
    assert!(text.contains("Go to Dashboard"), "Default commands missing");
}

#[test]
fn test_challenge3_special_characters_fuzzing() {
    let mut terminal = create_terminal(80, 24);
    let special_queries = [
        "!@#$%^&*()",
        "~`_-+={[}]|\\:;\"'<,>.?/",
        "🦀🚀⚡️🔥🎯",
        "'\"; DROP TABLE tasks; --",
        "<script>alert('xss')</script>",
        "\\x00\\xFF\\n\\r\\t",
        "[](){}^$*+?.|",
        "%s%d%x%n",
    ];

    for query in special_queries {
        let mut state = AppState::new().with_mock_data();
        state.show_command_palette = true;

        // Type each character into state via KeyCode::Char
        for c in query.chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }

        assert_eq!(state.command_palette_query, query);
        assert_eq!(
            state.command_palette_selected_index, 0,
            "Selection index not clamped to 0 after typing in query: {query}"
        );

        // Render buffer check
        terminal.draw(|f| render(f, &state))
            .unwrap_or_else(|e| panic!("Render panicked on special character query {query}: {e}"));

        let text = buffer_to_string(&terminal);
        // If no commands match, it should display "No matching commands found"
        let matches = coordinator::tui::widgets::command_palette::filtered_commands(&state.command_palette_query);
        if matches.is_empty() {
            assert!(
                text.contains("No matching commands"),
                "Expected empty match indicator for query: {query}"
            );
        }
    }
}

#[test]
fn test_challenge3_extreme_length_queries_over_100_chars() {
    let mut terminal = create_terminal(80, 24);
    let long_queries = [
        "a".repeat(101),
        "nav:".repeat(30), // 120 chars
        "task:approve_extremely_long_command_name_that_exceeds_normal_terminal_widths_repeated_multiple_times".to_string(),
        "x".repeat(300),
        "🔥".repeat(150),
    ];

    for query in &long_queries {
        let mut state = AppState::new().with_mock_data();
        state.show_command_palette = true;
        state.command_palette_query = query.clone();
        state.command_palette_selected_index = 0;

        terminal.draw(|f| render(f, &state))
            .unwrap_or_else(|e| panic!("Render panicked on query length {}: {e}", query.chars().count()));

        // Safe Down and Up navigation
        state.handle_key(make_key(KeyCode::Down));
        state.handle_key(make_key(KeyCode::Up));
        state.handle_key(make_ctrl_key('j'));
        state.handle_key(make_ctrl_key('k'));

        // Enter on non-matching query should not panic and should close or no-op safely
        let action = state.handle_key(make_key(KeyCode::Enter));
        assert!(
            action.is_none(),
            "Expected None action on unmatched query of length {}",
            query.len()
        );
        assert!(!state.show_command_palette);
    }
}

#[test]
fn test_challenge3_rapid_backspaces_underflow_fuzzing() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new().with_mock_data();
    state.show_command_palette = true;

    // Type 20 chars
    for c in "nav:dashboard".chars() {
        state.handle_key(make_key(KeyCode::Char(c)));
    }
    assert_eq!(state.command_palette_query, "nav:dashboard");

    // Rapidly press Backspace 50 times (far more than length)
    for _ in 0..50 {
        state.handle_key(make_key(KeyCode::Backspace));
        assert_eq!(state.command_palette_selected_index, 0);
    }
    assert_eq!(state.command_palette_query, "");

    // Must render cleanly
    terminal.draw(|f| render(f, &state)).expect("Render failed after backspace underflow");
    let text = buffer_to_string(&terminal);
    assert!(text.contains("Go to Dashboard"));
}

#[test]
fn test_challenge3_selection_clamping_navigation_and_dispatch() {
    let mut state = AppState::new().with_mock_data();
    state.show_command_palette = true;
    state.command_palette_query = "nav".to_string();

    let matches = coordinator::tui::widgets::command_palette::filtered_commands("nav");
    assert!(!matches.is_empty());
    let total = matches.len();

    // Spam Down key 100 times
    for _ in 0..100 {
        state.handle_key(make_key(KeyCode::Down));
    }
    assert_eq!(
        state.command_palette_selected_index,
        total - 1,
        "Selection index did not clamp at bottom"
    );

    // Spam Up key 100 times
    for _ in 0..100 {
        state.handle_key(make_key(KeyCode::Up));
    }
    assert_eq!(
        state.command_palette_selected_index, 0,
        "Selection index did not clamp at top"
    );

    // Navigate to index 1 (nav:projects) and press Enter
    state.handle_key(make_key(KeyCode::Down));
    assert_eq!(state.command_palette_selected_index, 1);
    let action = state.handle_key(make_key(KeyCode::Enter));
    assert_eq!(action, None);
    assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);
    assert!(!state.show_command_palette);
}
