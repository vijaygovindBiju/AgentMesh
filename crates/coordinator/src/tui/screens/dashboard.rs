use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::domain::{AgentStatus, ApprovalStatus, OverlapSeverity, TaskStatus};
use crate::tui::state::AppState;

pub struct DashboardScreen;

impl DashboardScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " System Overview & Live Dashboard ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Project Header Summary
                Constraint::Percentage(45), // Top half: Registered Agents Table
                Constraint::Percentage(55), // Bottom half: Task Metrics & Overlaps Split
            ])
            .split(inner_area);

        // 1. Project Header Summary
        let (name, desc, status) = if let Some(p) = &state.active_project {
            (p.name.as_str(), p.description.as_str(), format!("{:?}", p.status))
        } else {
            ("None", "No project currently active.", "Idle".to_string())
        };

        let summary_lines = vec![
            Line::from(vec![
                Span::styled("Active Project: ", Style::default().fg(Color::DarkGray)),
                Span::styled(name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  |  Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(status, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Scope: ", Style::default().fg(Color::DarkGray)),
                Span::styled(desc, Style::default().fg(Color::White)),
            ]),
        ];
        let summary_p = Paragraph::new(summary_lines);
        frame.render_widget(summary_p, chunks[0]);

        // 2. Registered Agents Table
        Self::render_agents_table(frame, chunks[1], state);

        // 3. Bottom row: Task Metrics (Left) & Active Overlap Warnings (Right)
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(45), // Task metrics
                Constraint::Percentage(55), // Active Overlap warnings
            ])
            .split(chunks[2]);

        Self::render_task_metrics(frame, bottom_chunks[0], state);
        Self::render_overlap_warnings(frame, bottom_chunks[1], state);
    }

    fn render_agents_table(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Registered Agents (Mesh Fleet) ");

        let rows: Vec<Row> = if state.agents.is_empty() {
            vec![Row::new(vec![
                "No agents currently registered.".to_string(),
                "-".to_string(),
                "-".to_string(),
            ])]
        } else {
            state
                .agents
                .iter()
                .map(|a| {
                    let (status_str, color) = match a.status {
                        AgentStatus::Idle => ("IDLE", Color::Green),
                        AgentStatus::Busy => ("BUSY", Color::Yellow),
                        AgentStatus::Blocked => ("BLOCKED", Color::Magenta),
                        AgentStatus::Error => ("ERROR", Color::Red),
                        AgentStatus::Offline => ("OFFLINE", Color::DarkGray),
                    };
                    Row::new(vec![
                        a.human_owner.clone(),
                        format!("{:?}", a.adapter_type),
                        status_str.to_string(),
                    ])
                    .style(Style::default().fg(color))
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(40),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ],
        )
        .header(
            Row::new(vec!["Agent Name", "Adapter Type", "Status"])
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .block(block);

        frame.render_widget(table, area);
    }

    fn render_task_metrics(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Task Status Metrics ");

        let total = state.review_tasks.len();
        let mut approved = 0;
        let mut rejected = 0;
        let mut pending = 0;

        for t in &state.review_tasks {
            match t.human_decision {
                Some(ApprovalStatus::Approved) => approved += 1,
                Some(ApprovalStatus::Rejected) => rejected += 1,
                _ => match t.task.status {
                    TaskStatus::Approved => approved += 1,
                    TaskStatus::Rejected => rejected += 1,
                    _ => pending += 1,
                },
            }
        }

        let lines = vec![
            Line::from(vec![
                Span::styled("Total Proposed Tasks: ", Style::default().fg(Color::White)),
                Span::styled(total.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  • Human Review (Pending): ", Style::default().fg(Color::Yellow)),
                Span::styled(pending.to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  • Operator Approved:      ", Style::default().fg(Color::Green)),
                Span::styled(approved.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("  • Operator Rejected:      ", Style::default().fg(Color::Red)),
                Span::styled(rejected.to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]),
        ];

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
    }

    fn render_overlap_warnings(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if !state.active_overlaps.is_empty() {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .title(" Active Overlap Warnings ");

        let lines: Vec<Line> = if state.active_overlaps.is_empty() {
            vec![Line::from(Span::styled(
                "✔ Zero resource conflicts detected across active tasks.",
                Style::default().fg(Color::Green),
            ))]
        } else {
            state
                .active_overlaps
                .iter()
                .map(|w| {
                    let sev_style = match w.severity {
                        OverlapSeverity::Critical => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        OverlapSeverity::Warning => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        OverlapSeverity::Info => Style::default().fg(Color::Blue),
                    };
                    Line::from(vec![
                        Span::styled(format!("[{:?}] ", w.severity), sev_style),
                        Span::styled(format!("'{}' ", w.resource), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("({} tasks)", w.task_id_list().len()), Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect()
        };

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
    }
}
