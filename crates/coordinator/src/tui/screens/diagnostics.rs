use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Row, Table},
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
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
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

        // 4. Overlays / Modals
        if state.show_event_payload_modal {
            Self::render_event_payload_modal(frame, area, state);
        }
        if state.show_timeline_modal {
            Self::render_timeline_modal(frame, area, state);
        }
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
                Span::styled(
                    " [Tasks] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("Total: {} | ", tasks_total)),
                Span::styled(
                    format!("Running: {} | ", tasks_executing),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("Completed: {} | ", tasks_done),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("Failed: {}   ", tasks_failed),
                    if tasks_failed > 0 {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(
                    " [Deliveries] ",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("Success Rate: {:.1}%   ", delivery_rate),
                    if delivery_rate >= 95.0 {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
                Span::styled(
                    " [Security & Conflicts] ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("Overlaps: {} | Alerts: {}", conflicts, audit_alerts),
                    if audit_alerts > 0 {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " [Fleet] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("Agents: {} | ", metrics.total_agents)),
                Span::styled(
                    format!("Healthy: {} | ", metrics.agents_healthy),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("Degraded: {} | ", metrics.agents_degraded),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("Unhealthy: {}   ", metrics.agents_unhealthy),
                    if metrics.agents_unhealthy > 0 {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(" [Status] ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if tasks_failed == 0 && audit_alerts == 0 {
                        "MESH OPERATIONAL (NOMINAL)"
                    } else {
                        "ATTENTION REQUIRED"
                    },
                    if tasks_failed == 0 && audit_alerts == 0 {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    },
                ),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Operational KPI Metrics ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
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
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
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

                    let tasks_info =
                        format!("{} / {}", a.tasks_completed_count, a.tasks_failed_count);
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
            Row::new(vec!["Agent", "State", "Health", "Done/Fail", "Err Rate"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block);

        frame.render_widget(table, area);
    }

    /// Renders task timeline drill-down view.
    fn render_timeline_drilldown(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.diagnostics_focus == crate::tui::state::DiagnosticsFocus::Timeline;
        let border_color = if is_focused {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let border_type = if is_focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        };
        let title_text = if is_focused {
            " Task Timeline Drill-Down [Focused: ↑/↓ Select Task, Enter Full Stages] "
        } else {
            " Task Timeline Drill-Down "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        if let Some(timeline) = &state.selected_timeline {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", timeline.short_id),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        &timeline.title,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" | Status: "),
                    Span::styled(
                        format!("{:?}", timeline.status),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Agent: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        timeline
                            .assigned_agent_name
                            .as_deref()
                            .unwrap_or("Unassigned"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(" | Duration: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        timeline
                            .total_duration_ms
                            .map(|d| format!("{}ms", d))
                            .unwrap_or_else(|| "in-progress".to_string()),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                Line::from(""),
            ];

            if timeline.events.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No timeline milestones recorded yet.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for item in &timeline.events {
                    let elapsed_str = item
                        .elapsed_since_start_ms
                        .map(|ms| format!("+{}ms", ms))
                        .unwrap_or_else(|| "0ms".to_string());
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("[{:>7}] ", elapsed_str),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!("{:<14} ", item.stage),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            format!("{:<16} ", item.actor),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(&item.message),
                    ]));
                }
            }

            let p = Paragraph::new(lines).block(block);
            frame.render_widget(p, area);
        } else if let Some(selected_task) = state.review_tasks.get(state.selected_task_index) {
            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", selected_task.task.short_id),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        &selected_task.task.title,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:?}", selected_task.task.status),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(" | Description: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&selected_task.task.description),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "No timeline loaded. Press [Enter] on a task to query real timeline.",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                )),
            ];
            let p = Paragraph::new(lines).block(block);
            frame.render_widget(p, area);
        } else {
            let p = Paragraph::new("No timeline loaded. Press [Enter] on a task to query real timeline.")
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(block);
            frame.render_widget(p, area);
        }
    }

    /// Renders the recent CoordinatorEvents log stream with sliding window scrolling and severity colour coding.
    fn render_events_stream(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.diagnostics_focus == crate::tui::state::DiagnosticsFocus::EventsStream;
        let border_color = if is_focused {
            Color::Magenta
        } else {
            Color::DarkGray
        };
        let border_type = if is_focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        };

        let sev_filter = state.event_filter_severity.as_deref().unwrap_or("ALL");
        let ent_filter = state.event_filter_entity.as_deref().unwrap_or("ALL");

        let filtered = state.filtered_recent_events();
        let selected_idx = if filtered.is_empty() {
            0
        } else {
            state.selected_event_index.min(filtered.len() - 1)
        };

        let title_text = if is_focused {
            format!(
                " Coordinator Events Stream ({}/{} events) [Focused: ↑/↓ Scroll, Enter Payload, f: Severity, e: Entity] [Filter: Severity: {} | Entity: {}] ",
                if filtered.is_empty() {
                    0
                } else {
                    selected_idx + 1
                },
                filtered.len(),
                sev_filter,
                ent_filter
            )
        } else {
            format!(
                " Coordinator Events Stream ({} events) [Filter: Severity: {} | Entity: {}] ",
                filtered.len(),
                sev_filter,
                ent_filter
            )
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let visible_rows = (area.height.saturating_sub(4)).max(5) as usize;
        let scroll_offset = if selected_idx >= visible_rows {
            selected_idx - visible_rows + 1
        } else {
            0
        };

        let rows: Vec<Row> = if state.recent_events.is_empty() {
            vec![Row::new(vec![
                "-".to_string(),
                "INFO".to_string(),
                "system.ready".to_string(),
                "Observability event pipeline initialized. Waiting for coordinator events..."
                    .to_string(),
                "-".to_string(),
            ])]
        } else if filtered.is_empty() {
            vec![Row::new(vec![
                "-".to_string(),
                "INFO".to_string(),
                "no.events".to_string(),
                format!("No events matching filter [Severity: {sev_filter} | Entity: {ent_filter}]. Press [f] or [e] to change filters."),
                "-".to_string(),
            ])]
        } else {
            filtered
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible_rows)
                .map(|(idx, ev)| {
                    let ts_str = ev.timestamp.format("%H:%M:%S%.3f").to_string();
                    let (level_str, level_col) = if ev.event_type.contains("failed")
                        || ev.event_type.contains("error")
                        || ev.event_type.contains("alert")
                    {
                        ("ERROR", Color::Red)
                    } else if ev.event_type.contains("warn")
                        || ev.event_type.contains("blocked")
                        || ev.event_type.contains("degraded")
                    {
                        ("WARN", Color::Yellow)
                    } else if ev.event_type.contains("completed")
                        || ev.event_type.contains("registered")
                    {
                        ("OK", Color::Green)
                    } else {
                        ("INFO", Color::Cyan)
                    };

                    let entity_id = ev
                        .task_id
                        .map(|id| format!("task:{}", &id.to_string()[..8]))
                        .or_else(|| {
                            ev.agent_id
                                .map(|id| format!("agent:{}", &id.to_string()[..8]))
                        })
                        .unwrap_or_else(|| "-".to_string());

                    let is_selected = idx == selected_idx;
                    let prefix = if is_selected && is_focused { "▶ " } else { "  " };
                    let time_display = format!("{prefix}{ts_str}");

                    let mut row_style = Style::default().fg(level_col);
                    if is_selected && is_focused {
                        row_style = row_style.add_modifier(Modifier::BOLD);
                    }

                    Row::new(vec![
                        time_display,
                        level_str.to_string(),
                        ev.event_type.clone(),
                        ev.message.clone(),
                        entity_id,
                    ])
                    .style(row_style)
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(15),     // Time with cursor
                Constraint::Length(7),      // Level
                Constraint::Percentage(25), // Event Type
                Constraint::Percentage(38), // Message
                Constraint::Percentage(15), // Entity ID
            ],
        )
        .header(
            Row::new(vec!["Time", "Level", "Event Type", "Message", "Entity"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block);

        frame.render_widget(table, area);
    }

    /// Renders event detail & structured JSON payload modal dialog.
    fn render_event_payload_modal(frame: &mut Frame, area: Rect, state: &AppState) {
        let filtered = state.filtered_recent_events();
        let ev = match filtered.get(state.selected_event_index).copied() {
            Some(e) => e,
            None => match state.recent_events.get(state.selected_event_index) {
                Some(e) => e,
                None => return,
            },
        };

        let popup_width = (area.width * 75 / 100).clamp(55, 85).min(area.width);
        let popup_height = (area.height * 70 / 100).clamp(16, 26).min(area.height);

        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Magenta))
            .title(Span::styled(
                " Coordinator Event Details & Structured Payload [Esc/Enter: Close] ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let payload_pretty = serde_json::to_string_pretty(&ev.payload)
            .unwrap_or_else(|_| "{}".to_string());

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Event Type: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &ev.event_type,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  Time: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    ev.timestamp.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Event ID: ", Style::default().fg(Color::DarkGray)),
                Span::styled(ev.id.to_string(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Message: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&ev.message, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("Target Task: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    ev.task_id.map(|id| id.to_string()).unwrap_or_else(|| "None".to_string()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled("  |  Target Agent: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    ev.agent_id.map(|id| id.to_string()).unwrap_or_else(|| "None".to_string()),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── JSON Payload ───────────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let max_payload_lines = inner.height.saturating_sub(9) as usize;
        for pl_line in payload_pretty.lines().take(max_payload_lines) {
            lines.push(Line::from(Span::styled(
                format!("  {pl_line}"),
                Style::default().fg(Color::Green),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press [Esc] or [Enter] to dismiss.",
            Style::default().fg(Color::DarkGray),
        )));

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }

    /// Renders full task chronological lifecycle stages modal.
    fn render_timeline_modal(frame: &mut Frame, area: Rect, state: &AppState) {
        let selected_task = match state.review_tasks.get(state.selected_task_index) {
            Some(t) => t,
            None => return,
        };

        let popup_width = (area.width * 75 / 100).clamp(55, 85).min(area.width);
        let popup_height = (area.height * 70 / 100).clamp(16, 26).min(area.height);

        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                format!(
                    " Task [{}] Full Lifecycle Timeline Drill-Down [Esc/Enter: Close] ",
                    selected_task.task.short_id
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Task: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("[{}] {}", selected_task.task.short_id, selected_task.task.title),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:?}", selected_task.task.status),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Description: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&selected_task.task.description),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Chronological Lifecycle Stages ─────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        // If active selected_timeline has events, render real recorded milestones; otherwise render clean prompt
        if let Some(tl) = &state.selected_timeline {
            if tl.events.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No timeline milestones recorded yet.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for item in &tl.events {
                    let elapsed_str = item
                        .elapsed_since_start_ms
                        .map(|ms| format!("+{}ms", ms))
                        .unwrap_or_else(|| "0ms".to_string());
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{:>7}] ", elapsed_str), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{:<14} ", item.stage), Style::default().fg(Color::Cyan)),
                        Span::styled(format!("{:<16} ", item.actor), Style::default().fg(Color::Yellow)),
                        Span::raw(&item.message),
                    ]));
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No timeline loaded. Press [Enter] on a task to query real timeline.",
                Style::default().fg(Color::Yellow),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press [Esc] or [Enter] to dismiss.",
            Style::default().fg(Color::DarkGray),
        )));

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }
}
