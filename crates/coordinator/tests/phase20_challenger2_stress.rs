use std::sync::Arc;
use std::time::Instant;
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

use agent_protocol::{AgentMessage, AgentStatus as ProtocolAgentStatus};
use coordinator::domain::{
    AdapterType, Agent, AgentStatus, Task, TaskStatus,
};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render, AppState, CurrentScreen, ReviewTaskState, TuiUpdateEvent,
};

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sample_task(id: Uuid, project_id: Uuid, short_id: &str, title: &str) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["test/resource"]),
        status: TaskStatus::Approved,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ============================================================================
// 1. BURST EVENT INJECTION (500+ RAPID AGENT MESSAGES)
// ============================================================================

#[test]
fn test_stress_burst_agent_messages_1000_sequential() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();

    // Register 10 agents
    let mut agent_ids = Vec::new();
    for i in 0..10 {
        let aid = Uuid::new_v4();
        agent_ids.push(aid);
        state.agents.push(Agent::mock(
            aid,
            &format!("agent-{i}"),
            AdapterType::Mock,
            vec!["worker".to_string()],
            AgentStatus::Idle,
        ));
    }

    // Register 50 review tasks
    let mut task_ids = Vec::new();
    for i in 0..50 {
        let tid = Uuid::new_v4();
        task_ids.push(tid);
        let task = sample_task(tid, proj_id, &format!("TASK-{i}"), &format!("Task #{i}"));
        state.review_tasks.push(ReviewTaskState::new(task));
    }

    let start = Instant::now();

    // Burst inject 1,000 mixed AgentMessages:
    // 200 TaskStarted, 400 ProgressUpdate, 150 Completed, 100 Failed, 100 Blocked, 50 Heartbeat
    for i in 0..1000 {
        let agent_id = agent_ids[i % agent_ids.len()];
        let task_id = task_ids[i % task_ids.len()];

        let msg = match i % 6 {
            0 => AgentMessage::TaskStarted {
                agent_id,
                task_id,
                idempotency_key: format!("key-{i}"),
                timestamp: Utc::now(),
            },
            1 => AgentMessage::ProgressUpdate {
                agent_id,
                task_id,
                percent: ((i * 7) % 100) as u8,
                message: format!("Progress at step {i}"),
                timestamp: Utc::now(),
            },
            2 => AgentMessage::Blocked {
                agent_id,
                task_id,
                reason: format!("Dependency blocked at step {i}"),
                blocking_task_id: None,
                timestamp: Utc::now(),
            },
            3 => AgentMessage::Completed {
                agent_id,
                task_id,
                summary: format!("Completed at step {i}"),
                timestamp: Utc::now(),
            },
            4 => AgentMessage::Failed {
                agent_id,
                task_id,
                error: format!("Error at step {i}"),
                timestamp: Utc::now(),
            },
            _ => AgentMessage::Heartbeat {
                agent_id,
                status: ProtocolAgentStatus::Busy,
                current_task_id: Some(task_id),
                health: None,
                timestamp: Utc::now(),
            },
        };

        state.apply_update(TuiUpdateEvent::AgentMessage(msg));
    }

    let duration = start.elapsed();
    println!("Processed 1,000 rapid AgentMessages in {:?}", duration);

    // Assert high throughput (< 500ms for 1000 events in memory)
    assert!(duration.as_millis() < 500, "Burst processing took too long: {:?}", duration);

    // Verify recent_events capping (50 Heartbeats do not call push_recent_event,
    // 950 other messages do, so recent_events should be capped at 200)
    assert_eq!(state.recent_events.len(), 200);

    // Verify status message is populated with the last processed message
    assert!(state.status_message.is_some());
}

#[test]
fn test_stress_burst_unknown_entities() {
    let mut state = AppState::new();

    // Burst inject 600 events with completely unknown agent_ids and task_ids
    for i in 0..600 {
        let unknown_aid = Uuid::new_v4();
        let unknown_tid = Uuid::new_v4();

        let msg = match i % 4 {
            0 => AgentMessage::TaskStarted {
                agent_id: unknown_aid,
                task_id: unknown_tid,
                idempotency_key: format!("unknown-{i}"),
                timestamp: Utc::now(),
            },
            1 => AgentMessage::ProgressUpdate {
                agent_id: unknown_aid,
                task_id: unknown_tid,
                percent: 42,
                message: format!("Unknown task progress {i}"),
                timestamp: Utc::now(),
            },
            2 => AgentMessage::Completed {
                agent_id: unknown_aid,
                task_id: unknown_tid,
                summary: format!("Unknown task completion {i}"),
                timestamp: Utc::now(),
            },
            _ => AgentMessage::Failed {
                agent_id: unknown_aid,
                task_id: unknown_tid,
                error: format!("Unknown task failure {i}"),
                timestamp: Utc::now(),
            },
        };

        // None of these should panic even though agents and tasks do not exist in AppState
        state.apply_update(TuiUpdateEvent::AgentMessage(msg));
    }

    assert_eq!(state.recent_events.len(), 200);
}

#[test]
fn test_stress_out_of_order_lifecycle() {
    let mut state = AppState::new();
    let proj_id = Uuid::new_v4();
    let aid = Uuid::new_v4();
    let tid = Uuid::new_v4();

    state.agents.push(Agent::mock(
        aid,
        "worker-alpha",
        AdapterType::Mock,
        vec!["compute".to_string()],
        AgentStatus::Idle,
    ));

    state.review_tasks.push(ReviewTaskState::new(sample_task(
        tid,
        proj_id,
        "TASK-O3",
        "Out of order task",
    )));

    // 1. Receive Completed before Started
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::Completed {
        agent_id: aid,
        task_id: tid,
        summary: "Premature complete".to_string(),
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Completed);
    assert_eq!(state.review_tasks[0].progress_percent, Some(100));

    // 2. Receive ProgressUpdate after Completed
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::ProgressUpdate {
        agent_id: aid,
        task_id: tid,
        percent: 25,
        message: "Late progress report".to_string(),
        timestamp: Utc::now(),
    }));
    // Note: progress_percent reflects the latest progress update received
    assert_eq!(state.review_tasks[0].progress_percent, Some(25));

    // 3. Receive Failed after Completed
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::Failed {
        agent_id: aid,
        task_id: tid,
        error: "Post-completion crash".to_string(),
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Failed);

    // 4. Receive TaskStarted after Failed
    state.apply_update(TuiUpdateEvent::AgentMessage(AgentMessage::TaskStarted {
        agent_id: aid,
        task_id: tid,
        idempotency_key: "retry-1".to_string(),
        timestamp: Utc::now(),
    }));
    assert_eq!(state.review_tasks[0].task.status, TaskStatus::Executing);
    assert_eq!(state.review_tasks[0].progress_percent, Some(0));
}

// ============================================================================
// 2. STATE CONCURRENCY & MULTI-PRODUCER BURST
// ============================================================================

#[tokio::test]
async fn test_stress_concurrent_producers_burst() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();
    let state = Arc::new(Mutex::new(AppState::new()));

    let num_producers = 10;
    let events_per_producer = 100; // Total 1,000 events

    let mut handles = Vec::new();
    for p in 0..num_producers {
        let producer_tx = tx.clone();
        handles.push(tokio::spawn(async move {
            for e in 0..events_per_producer {
                let msg = AgentMessage::ProgressUpdate {
                    agent_id: Uuid::new_v4(),
                    task_id: Uuid::new_v4(),
                    percent: ((p * 10 + e) % 100) as u8,
                    message: format!("Producer {p} Event {e}"),
                    timestamp: Utc::now(),
                };
                producer_tx.send(TuiUpdateEvent::AgentMessage(msg)).unwrap();
            }
        }));
    }
    drop(tx); // Close original tx so receiver terminates when producers finish

    let state_consumer = state.clone();
    let consumer_handle = tokio::spawn(async move {
        let mut total_received = 0;
        while let Some(evt) = rx.recv().await {
            let mut s = state_consumer.lock().await;
            s.apply_update(evt);
            total_received += 1;
        }
        total_received
    });

    for h in handles {
        h.await.unwrap();
    }
    let total = consumer_handle.await.unwrap();

    assert_eq!(total, 1000);
    let final_state = state.lock().await;
    assert_eq!(final_state.recent_events.len(), 200);
}

// ============================================================================
// 3. SLIDING WINDOW CAPACITY CAPPING (200 EVENTS) & ORDERING
// ============================================================================

#[test]
fn test_stress_sliding_window_exact_retention_and_ordering() {
    let mut state = AppState::new();

    // Push 1,000 sequential ObservabilityEvents
    for seq in 1..=1000 {
        let ev = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "benchmark.seq".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Event #{seq}"),
            payload: json!({ "seq": seq }),
        };
        state.apply_update(TuiUpdateEvent::ObservabilityEvent(ev));

        // Invariant: at any point, recent_events <= 200
        assert!(state.recent_events.len() <= 200);
    }

    assert_eq!(state.recent_events.len(), 200);

    // Index 0 must be newest (seq = 1000)
    assert_eq!(state.recent_events[0].message, "Event #1000");
    assert_eq!(state.recent_events[0].payload["seq"], 1000);

    // Index 199 must be 200th newest (seq = 801)
    assert_eq!(state.recent_events[199].message, "Event #801");
    assert_eq!(state.recent_events[199].payload["seq"], 801);

    // Verify all sequence numbers from 1000 down to 801 are present in order
    for idx in 0..200 {
        let expected_seq = 1000 - idx;
        assert_eq!(state.recent_events[idx].payload["seq"], expected_seq);
    }
}

// ============================================================================
// 4. DIAGNOSTICS NAVIGATION & RENDERING UNDER BURST
// ============================================================================

#[test]
fn test_stress_diagnostics_scroll_clamping_and_modal() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;

    // Push 500 events
    for i in 1..=500 {
        state.apply_update(TuiUpdateEvent::ObservabilityEvent(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: format!("event.type.{i}"),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Log message {i}"),
            payload: json!({ "index": i }),
        }));
    }

    assert_eq!(state.recent_events.len(), 200);

    // 1. Initial render at top
    terminal.draw(|f| render(f, &state)).expect("Render top of diagnostics");

    // 2. Scroll down 300 times (exceeding 200 capacity)
    for _ in 0..300 {
        state.handle_key(make_key(KeyCode::Down));
    }
    // Must clamp at index 199
    assert_eq!(state.selected_event_index, 199);

    // Render at bottom of buffer
    terminal.draw(|f| render(f, &state)).expect("Render bottom of diagnostics");

    // 3. Open payload inspection modal on item 199
    state.handle_key(make_key(KeyCode::Enter));
    assert!(state.show_event_payload_modal);

    // Render with payload modal active
    terminal.draw(|f| render(f, &state)).expect("Render payload modal at bottom");

    // 4. Close modal with Esc
    state.handle_key(make_key(KeyCode::Esc));
    assert!(!state.show_event_payload_modal);

    // 5. Scroll up 300 times
    for _ in 0..300 {
        state.handle_key(make_key(KeyCode::Up));
    }
    // Must clamp at index 0
    assert_eq!(state.selected_event_index, 0);

    terminal.draw(|f| render(f, &state)).expect("Render top after scroll back");
}

// ============================================================================
// 5. EDGE CASE: RECENT EVENTS TRUNCATION / RESIZE WITH STALE SELECTED INDEX
// ============================================================================

#[test]
fn test_stress_stale_selected_index_after_events_shrink() {
    let mut terminal = create_terminal(120, 30);
    let mut state = AppState::new();
    state.current_screen = CurrentScreen::Diagnostics;

    // Push 200 events
    for i in 1..=200 {
        state.push_recent_event(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "ev".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("ev {i}"),
            payload: json!({}),
        });
    }

    // Scroll down to index 180
    state.selected_event_index = 180;

    // Simulate external query update via TuiUpdateEvent::RecentEvents with only 10 events
    let shrunk_events: Vec<CoordinatorEvent> = (1..=10)
        .map(|i| CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "shrunk".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("shrunk {i}"),
            payload: json!({}),
        })
        .collect();

    state.apply_update(TuiUpdateEvent::RecentEvents(shrunk_events));
    assert_eq!(state.recent_events.len(), 10);
    // Observe: selected_event_index is still 180 while recent_events.len() is 10
    assert_eq!(state.selected_event_index, 180);

    // Empirical test: Does rendering panic when selected_event_index > recent_events.len()?
    let render_res = terminal.draw(|f| render(f, &state));
    assert!(render_res.is_ok(), "Rendering panicked on out-of-bounds selected_event_index!");

    // Enter modal on out-of-bounds selected_event_index
    state.handle_key(make_key(KeyCode::Enter));
    // render_event_payload_modal should return gracefully without crashing
    let modal_render_res = terminal.draw(|f| render(f, &state));
    assert!(modal_render_res.is_ok(), "Modal rendering panicked on out-of-bounds selected_event_index!");
}

// ============================================================================
// 6. CAPACITY BYPASS IN RECENT_EVENTS UPDATE EVENT
// ============================================================================

#[test]
fn test_stress_recent_events_update_capacity_bypass() {
    let mut state = AppState::new();

    // Generate 350 events
    let large_batch: Vec<CoordinatorEvent> = (1..=350)
        .map(|i| CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "batch.event".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Batch #{i}"),
            payload: json!({ "i": i }),
        })
        .collect();

    // When applied via RecentEvents, it directly assigns without truncation
    state.apply_update(TuiUpdateEvent::RecentEvents(large_batch));

    // Note: recent_events holds 350 events because apply_update does not truncate RecentEvents
    assert_eq!(state.recent_events.len(), 350);

    // However, the very next single push_recent_event will truncate it down to 200
    state.push_recent_event(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "single.event".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Trigger truncation".to_string(),
        payload: json!({}),
    });

    assert_eq!(state.recent_events.len(), 200);
}

// ============================================================================
// 7. HEADLESS UNDRAINED CHANNEL BUFFER ACCUMULATION
// ============================================================================

#[test]
fn test_stress_headless_undrained_channel_behavior() {
    // In headless mode in main.rs, tui_tx is kept alive and sent to by metrics and
    // event subscriber loops, while tui_rx is never polled or drained.
    let (tui_tx, tui_rx) = tokio::sync::mpsc::unbounded_channel::<TuiUpdateEvent>();

    // Send 1,000 events without draining tui_rx
    for i in 0..1000 {
        let ev = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "undrained.test".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Message {i}"),
            payload: json!({}),
        };
        assert!(tui_tx.send(TuiUpdateEvent::ObservabilityEvent(ev)).is_ok());
    }

    // Since tui_rx is still alive, send succeeds and messages accumulate in memory.
    // Explicit drop of tui_rx:
    drop(tui_rx);

    // Subsequent sends fail immediately
    let fail_ev = CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "dropped".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Should fail".to_string(),
        payload: json!({}),
    };
    assert!(tui_tx.send(TuiUpdateEvent::ObservabilityEvent(fail_ev)).is_err());
}
