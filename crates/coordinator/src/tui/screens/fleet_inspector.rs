use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Row, Table},
    Frame,
};

use crate::domain::{AgentStatus, HealthStatus};
use crate::tui::state::AppState;
use crate::tui::widgets::AgentInspectorWidget;

pub struct FleetInspectorScreen;

impl FleetInspectorScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Adaptive split: Horizontal on wide displays (>=100 cols), Vertical on compact displays
        let chunks = if area.width >= 100 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(area)
        };

        // 1. Top/Left Panel: Connected Agent Fleet table
        Self::render_fleet_table(frame, chunks[0], state);

        // 2. Bottom/Right Panel: Telemetry & capability details using AgentInspectorWidget
        AgentInspectorWidget::render_panel(frame, chunks[1], state);
    }

    fn render_fleet_table(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Connected Agent Fleet & Liveness [j/k: Select Agent] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let rows: Vec<Row> = if state.agents.is_empty() {
            vec![Row::new(vec![
                "No agents currently registered.".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ])]
        } else {
            state
                .agents
                .iter()
                .enumerate()
                .map(|(idx, a)| {
                    let (name, owner) = if let Some(first_idx) = a.human_owner.find(" (") {
                        let name = &a.human_owner[..first_idx];
                        let owner = a.human_owner[first_idx + 2..].trim_end_matches(')');
                        (name.to_string(), owner.to_string())
                    } else {
                        (
                            format!("agent-{}", &a.id.to_string()[..8]),
                            a.human_owner.clone(),
                        )
                    };

                    let is_selected = idx == state.selected_agent_index;
                    let mark = if is_selected { "▶ " } else { "  " };
                    let display_name = format!("{mark}{name}");

                    let runtime_str = format!("{:?}", a.adapter_type);

                    let (status_str, status_col) = match a.status {
                        AgentStatus::Idle => ("IDLE", Color::Green),
                        AgentStatus::Busy => ("BUSY", Color::Yellow),
                        AgentStatus::Blocked => ("BLOCKED", Color::Magenta),
                        AgentStatus::Error => ("ERROR", Color::Red),
                        AgentStatus::Offline => ("OFFLINE", Color::DarkGray),
                    };

                    let (health_badge, _health_col) = match a.health_status {
                        HealthStatus::Healthy => ("[Healthy]", Color::Green),
                        HealthStatus::Degraded => ("[Degraded]", Color::Yellow),
                        HealthStatus::Unhealthy => ("[Unhealthy]", Color::Red),
                        HealthStatus::Offline => ("[Offline]", Color::DarkGray),
                    };

                    let latency_str = a
                        .heartbeat_latency_ms
                        .map(|ms| format!("{ms}ms"))
                        .unwrap_or_else(|| "-".to_string());

                    let current_task_str = if let Some(tid) = a.current_task_id {
                        if let Some(t) = state.review_tasks.iter().find(|rt| rt.task.id == tid) {
                            format!("[{}] {}", t.task.short_id, t.task.title)
                        } else {
                            format!("[{}]", &tid.to_string()[..8])
                        }
                    } else {
                        "- (Idle)".to_string()
                    };

                    let mut row_style = Style::default().fg(status_col);
                    if is_selected {
                        row_style = row_style.add_modifier(Modifier::BOLD);
                    }

                    Row::new(vec![
                        display_name,
                        owner,
                        runtime_str,
                        status_str.to_string(),
                        health_badge.to_string(),
                        latency_str,
                        current_task_str,
                    ])
                    .style(row_style)
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(20), // Agent Name
                Constraint::Percentage(16), // Human Owner
                Constraint::Percentage(10), // Runtime
                Constraint::Percentage(10), // Status
                Constraint::Percentage(12), // Health
                Constraint::Percentage(10), // Latency
                Constraint::Percentage(22), // Current Task
            ],
        )
        .header(
            Row::new(vec![
                "Agent Name",
                "Human Owner",
                "Runtime",
                "Status",
                "Health",
                "Latency",
                "Current Task",
            ])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block);

        frame.render_widget(table, area);
    }
}
