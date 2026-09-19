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
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),      // 1. Project Header Summary
                Constraint::Percentage(28), // 2. Registered Agents (Fleet) Table
                Constraint::Percentage(36), // 3. All Tasks Table
                Constraint::Percentage(36), // 4. Bottom row: Task Metrics & Overlaps Split
            ])
            .split(inner_area);

        // 1. Project Header Summary
        let (name, desc, status) = if let Some(p) = &state.active_project {
            (
                p.name.as_str(),
                p.description.as_str(),
                format!("{:?}", p.status),
            )
        } else {
            ("None", "No project currently active.", "Idle".to_string())
        };

        let summary_lines = vec![
            Line::from(vec![
                Span::styled("Active Project: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    name,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
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

        // 2. Registered Agents Table: name, owner, current task, status
        Self::render_agents_table(frame, chunks[1], state);

        // 3. Tasks Overview Table: short ID, title, status, assigned agent
        Self::render_tasks_table(frame, chunks[2], state);

        // 4. Bottom row: Task Metrics (Left) & Active Overlap Warnings (Right)
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(42), // Task metrics
                Constraint::Percentage(58), // Active Overlap warnings
            ])
            .split(chunks[3]);

        Self::render_task_metrics(frame, bottom_chunks[0], state);
        Self::render_overlap_warnings(frame, bottom_chunks[1], state);
    }

    /// Renders the fleet table: name, owner, current task, status.
    fn render_agents_table(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Registered Agents (Mesh Fleet) ",
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
            ])]
        } else {
            state
                .agents
                .iter()
                .map(|a| {
                    let (name, owner) = if let Some(idx) = a.human_owner.find(" (") {
                        let name = &a.human_owner[..idx];
                        let owner = a.human_owner[idx + 2..].trim_end_matches(')');
                        (name.to_string(), owner.to_string())
                    } else {
                        (
                            format!("agent-{}", &a.id.to_string()[..8]),
                            a.human_owner.clone(),
                        )
                    };

                    let current_task = if let Some(tid) = a.current_task_id {
                        if let Some(t) = state.review_tasks.iter().find(|rt| rt.task.id == tid) {
                            format!("[{}] {}", t.task.short_id, t.task.title)
                        } else {
                            format!("[{}]", &tid.to_string()[..8])
                        }
                    } else {
                        "- (Idle)".to_string()
                    };

                    let (status_str, color) = match a.status {
                        AgentStatus::Idle => ("IDLE", Color::Green),
                        AgentStatus::Busy => ("BUSY", Color::Yellow),
                        AgentStatus::Blocked => ("BLOCKED", Color::Magenta),
                        AgentStatus::Error => ("ERROR", Color::Red),
                        AgentStatus::Offline => ("OFFLINE", Color::DarkGray),
                    };

                    Row::new(vec![name, owner, current_task, status_str.to_string()])
                        .style(Style::default().fg(color))
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(40),
                Constraint::Percentage(15),
            ],
        )
        .header(
            Row::new(vec!["Agent Name", "Human Owner", "Current Task", "Status"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(block);

        frame.render_widget(table, area);
    }

    /// Renders the tasks table: short ID, title, status, assigned agent.
    fn render_tasks_table(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " All Tasks Overview ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let rows: Vec<Row> = if state.review_tasks.is_empty() {
            vec![Row::new(vec![
                "-".to_string(),
                "No tasks currently in project.".to_string(),
                "-".to_string(),
                "-".to_string(),
            ])]
        } else {
            state
                .review_tasks
                .iter()
                .map(|item| {
                    let task = &item.task;
                    let (status_str, status_color) = match task.status {
                        TaskStatus::Proposed => ("Proposed", Color::DarkGray),
                        TaskStatus::HumanReview => ("HumanReview", Color::Yellow),
                        TaskStatus::Approved => ("Approved", Color::Green),
                        TaskStatus::Rejected => ("Rejected", Color::Red),
                        TaskStatus::Assigned => ("Assigned", Color::Cyan),
                        TaskStatus::Executing => ("Executing", Color::Cyan),
                        TaskStatus::Blocked => ("Blocked", Color::Magenta),
                        TaskStatus::Completed => ("Completed", Color::Green),
                        TaskStatus::Failed => ("Failed", Color::Red),
                        TaskStatus::Cancelled => ("Cancelled", Color::DarkGray),
                    };

                    let assigned_agent = if let Some(agent_id) = task.assigned_agent_id {
                        if let Some(a) = state.agents.iter().find(|a| a.id == agent_id) {
                            a.human_owner.clone()
                        } else {
                            format!("agent-{}", &agent_id.to_string()[..8])
                        }
                    } else if let Some(ref suggested) = item.suggested_agent_name {
                        format!("{suggested} (suggested)")
                    } else {
                        "- (Unassigned)".to_string()
                    };

                    Row::new(vec![
                        task.short_id.clone(),
                        task.title.clone(),
                        status_str.to_string(),
                        assigned_agent,
                    ])
                    .style(Style::default().fg(status_color))
                })
                .collect()
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Percentage(45),
                Constraint::Length(16),
                Constraint::Percentage(25),
            ],
        )
        .header(
            Row::new(vec!["Short ID", "Title", "Status", "Assigned Agent"]).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
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
        let mut executing = 0;
        let mut completed = 0;

        for t in &state.review_tasks {
            match t.human_decision {
                Some(ApprovalStatus::Approved) | Some(ApprovalStatus::EditedAndApproved) => {
                    match t.task.status {
                        TaskStatus::Executing => executing += 1,
                        TaskStatus::Completed => completed += 1,
                        _ => approved += 1,
                    }
                }
                Some(ApprovalStatus::Rejected) => rejected += 1,
                None => match t.task.status {
                    TaskStatus::Approved | TaskStatus::Assigned => approved += 1,
                    TaskStatus::Executing => executing += 1,
                    TaskStatus::Completed => completed += 1,
                    TaskStatus::Rejected => rejected += 1,
                    _ => pending += 1,
                },
            }
        }

        let lines = vec![
            Line::from(vec![
                Span::styled("Total Tasks: ", Style::default().fg(Color::White)),
                Span::styled(
                    total.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  Pending Review: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    pending.to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Approved / Queued: ", Style::default().fg(Color::Green)),
                Span::styled(
                    approved.to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  Executing: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    executing.to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Completed: ", Style::default().fg(Color::Green)),
                Span::styled(
                    completed.to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  Rejected: ", Style::default().fg(Color::Red)),
                Span::styled(
                    rejected.to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
    }

    fn render_overlap_warnings(frame: &mut Frame, area: Rect, state: &AppState) {
        let has_critical = state
            .active_overlaps
            .iter()
            .any(|w| w.severity == OverlapSeverity::Critical);

        let border_color = if has_critical {
            Color::Red
        } else if !state.active_overlaps.is_empty() {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                " Active Overlap Warnings [Press 'A' to acknowledge] ",
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let lines: Vec<Line> = if state.active_overlaps.is_empty() {
            vec![Line::from(Span::styled(
                "✔ Zero resource conflicts detected across active tasks.",
                Style::default().fg(Color::Green),
            ))]
        } else {
            state
                .active_overlaps
                .iter()
                .enumerate()
                .map(|(idx, w)| {
                    let sev_style = match w.severity {
                        OverlapSeverity::Critical => {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        }
                        OverlapSeverity::Warning => Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                        OverlapSeverity::Info => Style::default().fg(Color::Blue),
                    };

                    let cursor = if idx == state.selected_overlap_index {
                        "▶ "
                    } else {
                        "  "
                    };

                    let task_ids = w.task_id_list();
                    let task_names: Vec<String> = task_ids
                        .iter()
                        .map(|tid| {
                            state
                                .review_tasks
                                .iter()
                                .find(|rt| rt.task.id == *tid)
                                .map(|rt| rt.task.short_id.clone())
                                .unwrap_or_else(|| tid.to_string()[..8].to_string())
                        })
                        .collect();

                    Line::from(vec![
                        Span::styled(cursor, Style::default().fg(Color::Cyan)),
                        Span::styled(format!("[{:?}] ", w.severity), sev_style),
                        Span::styled(
                            format!("'{}' ", w.resource),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("(Tasks: {})", task_names.join(", ")),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ])
                })
                .collect()
        };

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
    }
}
