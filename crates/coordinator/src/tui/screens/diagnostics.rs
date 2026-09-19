use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::domain::{AgentStatus, HealthStatus};
use crate::tui::state::AppState;

pub struct DiagnosticsScreen;

impl DiagnosticsScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Magenta))
            .title(Span::styled(
                " System Observability & Diagnostics ",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),      // 1. Throughput & Execution Metrics KPI bar
                Constraint::Percentage(45), // 2. Fleet Health & Task Timeline Drill-Down split
                Constraint::Percentage(50), // 3. Recent Coordinator Events log stream
            ])
            .split(inner_area);

        // 1. Metrics KPI Bar
        Self::render_metrics_bar(frame, chunks[0], state);

        // 2. Middle Row: Fleet Health (Left) + Task Timeline Drill-Down (Right)
        let middle_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // Agent Health & Error Rates
                Constraint::Percentage(50), // Task Timeline Drill-Down
            ])
            .split(chunks[1]);

        Self::render_fleet_health(frame, middle_chunks[0], state);
        Self::render_timeline_drilldown(frame, middle_chunks[1], state);

        // 3. Coordinator Events Log Stream
        Self::render_events_stream(frame, chunks[2], state);
    }

    /// Renders real-time system throughput and execution KPI cards.
    fn render_metrics_bar(frame: &mut Frame, area: Rect, state: &AppState) {
        let metrics = state.metrics.clone().unwrap_or_default();

        let tasks_total = metrics.total_tasks;
        let tasks_done = metrics.tasks_completed;
        let tasks_failed = metrics.tasks_failed;
        let tasks_executing = metrics.tasks_executing;
        let delivery_rate = metrics.delivery_success_rate_percent;
        let audit_alerts = metrics.total_audit_alerts;
        let conflicts = metrics.total_conflicts;

        let kpi_text = vec![
            Line::from(vec![
                Span::styled(" [Tasks] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(format!("Total: {} | ", tasks_total)),
                Span::styled(format!("Running: {} | ", tasks_executing), Style::default().fg(Color::Yellow)),
                Span::styled(format!("Completed: {} | ", tasks_done), Style::default().fg(Color::Green)),
                Span::styled(format!("Failed: {}   ", tasks_failed), if tasks_failed > 0 { Style::default().fg(Color::Red).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }),
                Span::styled(" [Deliveries] ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(format!("Success Rate: {:.1}%   ", delivery_rate), if delivery_rate >= 95.0 { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Yellow) }),
                Span::styled(" [Security & Conflicts] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(format!("Overlaps: {} | Alerts: {}", conflicts, audit_alerts), if audit_alerts > 0 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::White) }),
            ]),
            Line::from(vec![
                Span::styled(" [Fleet] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(format!("Agents: {} | ", metrics.total_agents)),
                Span::styled(format!("Healthy: {} | ", metrics.agents_healthy), Style::default().fg(Color::Green)),
                Span::styled(format!("Degraded: {} | ", metrics.agents_degraded), Style::default().fg(Color::Yellow)),
                Span::styled(format!("Unhealthy: {}   ", metrics.agents_unhealthy), if metrics.agents_unhealthy > 0 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::DarkGray) }),
                Span::styled(" [Status] ", Style::default().fg(Color::DarkGray)),
                Span::styled(if tasks_failed == 0 && audit_alerts == 0 { "MESH OPERATIONAL (NOMINAL)" } else { "ATTENTION REQUIRED" }, if tasks_failed == 0 && audit_alerts == 0 { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) }),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Operational KPI Metrics ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));

        let p = Paragraph::new(kpi_text).block(block);
        frame.render_widget(p, area);
    }

    /// Renders per-agent health status and error rates.
    fn render_fleet_health(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Fleet Health & Diagnostics ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        let rows: Vec<Row> = if state.agents.is_empty() {
            vec![Row::new(vec![
                "No agents connected.".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ])]
        } else {
            state
                .agents
                .iter()
                .map(|a| {
                    let name = a.human_owner.clone();
                    let (status_str, _status_col) = match a.status {
                        AgentStatus::Idle => ("IDLE", Color::Green),
                        AgentStatus::Busy => ("BUSY", Color::Yellow),
                        AgentStatus::Blocked => ("BLOCKED", Color::Magenta),
                        AgentStatus::Error => ("ERROR", Color::Red),
                        AgentStatus::Offline => ("OFFLINE", Color::DarkGray),
                    };

                    let (health_str, health_col) = match a.health_status {
                        HealthStatus::Healthy => ("HEALTHY", Color::Green),
                        HealthStatus::Degraded => ("DEGRADED", Color::Yellow),
                        HealthStatus::Unhealthy => ("UNHEALTHY", Color::Red),
                        HealthStatus::Offline => ("OFFLINE", Color::DarkGray),
                    };

                    let tasks_info = format!("{} / {}", a.tasks_completed_count, a.tasks_failed_count);
                    let err_rate = if a.tasks_completed_count + a.tasks_failed_count > 0 {
                        let total = (a.tasks_completed_count + a.tasks_failed_count) as f64;
                        format!("{:.1}%", (a.tasks_failed_count as f64 / total) * 100.0)
                    } else {
                        "0.0%".to_string()
                    };

                    Row::new(vec![
                        name,
                        status_str.to_string(),
                        health_str.to_string(),
                        tasks_info,
                        err_rate,
                    ])
                    .style(Style::default().fg(health_col))
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(32),
                Constraint::Percentage(16),
                Constraint::Percentage(18),
                Constraint::Percentage(18),
                Constraint::Percentage(16),
            ],
        )
        .header(
            Row::new(vec!["Agent", "State", "Health", "Done/Fail", "Err Rate"])
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .block(block);

        frame.render_widget(table, area);
    }

    /// Renders task timeline drill-down view.
    fn render_timeline_drilldown(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Task Timeline Drill-Down ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));

        if let Some(timeline) = &state.selected_timeline {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(format!("[{}] ", timeline.short_id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(&timeline.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" | Status: "),
                    Span::styled(format!("{:?}", timeline.status), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::styled("Agent: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(timeline.assigned_agent_name.as_deref().unwrap_or("Unassigned"), Style::default().fg(Color::Cyan)),
                    Span::styled(" | Duration: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        timeline.total_duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "in-progress".to_string()),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                Line::from(""),
            ];

            if timeline.events.is_empty() {
                lines.push(Line::from(Span::styled("No timeline milestones recorded yet.", Style::default().fg(Color::DarkGray))));
            } else {
                for item in &timeline.events {
                    let elapsed_str = item.elapsed_since_start_ms.map(|ms| format!("+{}ms", ms)).unwrap_or_else(|| "0ms".to_string());
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{:>7}] ", elapsed_str), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:<14} ", item.stage), Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{:<16} ", item.actor), Style::default().fg(Color::Yellow)),
                        Span::raw(&item.message),
                    ]));
                }
            }

            let p = Paragraph::new(lines).block(block);
            frame.render_widget(p, area);
        } else if let Some(selected_task) = state.review_tasks.get(state.selected_task_index) {
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("[{}] ", selected_task.task.short_id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(&selected_task.task.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{:?}", selected_task.task.status), Style::default().fg(Color::Yellow)),
                    Span::styled(" | Description: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&selected_task.task.description),
                ]),
                Line::from(""),
                Line::from(Span::styled("Press [Enter] or navigate to view full chronological lifecycle timeline.", Style::default().fg(Color::DarkGray))),
            ];
            let p = Paragraph::new(lines).block(block);
            frame.render_widget(p, area);
        } else {
            let p = Paragraph::new("No task selected for timeline inspection.").block(block);
            frame.render_widget(p, area);
        }
    }

    /// Renders the recent CoordinatorEvents log stream with severity colour coding.
    fn render_events_stream(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Coordinator Events Stream (Observability Log) ",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ));

        let rows: Vec<Row> = if state.recent_events.is_empty() {
            vec![Row::new(vec![
                "-".to_string(),
                "INFO".to_string(),
                "system.ready".to_string(),
                "Observability event pipeline initialized. Waiting for coordinator events...".to_string(),
                "-".to_string(),
            ])]
        } else {
            state
                .recent_events
                .iter()
                .take(30)
                .map(|ev| {
                    let ts_str = ev.timestamp.format("%H:%M:%S%.3f").to_string();
                    let (level_str, level_col) = if ev.event_type.contains("failed") || ev.event_type.contains("error") || ev.event_type.contains("alert") {
                        ("ERROR", Color::Red)
                    } else if ev.event_type.contains("warn") || ev.event_type.contains("blocked") || ev.event_type.contains("degraded") {
                        ("WARN", Color::Yellow)
                    } else if ev.event_type.contains("completed") || ev.event_type.contains("registered") {
                        ("OK", Color::Green)
                    } else {
                        ("INFO", Color::Cyan)
                    };

                    let entity_id = ev.task_id
                        .map(|id| format!("task:{}", &id.to_string()[..8]))
                        .or_else(|| ev.agent_id.map(|id| format!("agent:{}", &id.to_string()[..8])))
                        .unwrap_or_else(|| "-".to_string());

                    Row::new(vec![
                        ts_str,
                        level_str.to_string(),
                        ev.event_type.clone(),
                        ev.message.clone(),
                        entity_id,
                    ])
                    .style(Style::default().fg(level_col))
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(13),     // Time
                Constraint::Length(7),      // Level
                Constraint::Percentage(25), // Event Type
                Constraint::Percentage(40), // Message
                Constraint::Percentage(15), // Entity ID
            ],
        )
        .header(
            Row::new(vec!["Time", "Level", "Event Type", "Message", "Entity"])
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .block(block);

        frame.render_widget(table, area);
    }
}
