use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::domain::TaskStatus;
use crate::tui::state::{AppState, InputMode};

pub struct ProjectInputScreen;

impl ProjectInputScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if area.width < 120 {
            // Compact stacked layout
            let vert_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(area);
            Self::render_project_management(frame, vert_chunks[0], state);
            Self::render_input_form(frame, vert_chunks[1], state);
        } else {
            // Side-by-side dual purpose layout
            let horiz_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            Self::render_project_management(frame, horiz_chunks[0], state);
            Self::render_input_form(frame, horiz_chunks[1], state);
        }
    }

    fn render_project_management(frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        let compact = area.height < 14;
        let list_area = if compact { area } else { chunks[0] };

        // 1. Projects List Table
        let list_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Projects List ([Enter/L] Load, [↑/↓, j/k] Select) ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        if state.available_projects.is_empty() {
            let empty_text = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No projects found.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  Press [N] or [i] to define a new project.",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let p = Paragraph::new(empty_text).block(list_block);
            frame.render_widget(p, list_area);
        } else {
            let rows: Vec<Row> = state
                .available_projects
                .iter()
                .enumerate()
                .map(|(idx, p)| {
                    let is_sel = idx == state.selected_project_index;
                    let is_active = state.active_project.as_ref().map(|ap| ap.id) == Some(p.id);
                    let mark = if is_active {
                        "★ "
                    } else if is_sel {
                        "▶ "
                    } else {
                        "  "
                    };

                    let row_style = if is_sel {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let short_id = if p.id.to_string().len() >= 8 {
                        &p.id.to_string()[..8]
                    } else {
                        "default"
                    };

                    let status_str = format!("{:?}", p.status);
                    let status_style = match p.status {
                        crate::domain::ProjectStatus::Active => Style::default().fg(Color::Green),
                        crate::domain::ProjectStatus::Completed => Style::default().fg(Color::Blue),
                        crate::domain::ProjectStatus::Planning => {
                            Style::default().fg(Color::Yellow)
                        }
                        crate::domain::ProjectStatus::Paused => Style::default().fg(Color::Red),
                        _ => Style::default().fg(Color::Cyan),
                    };

                    let created_str = p.created_at.format("%Y-%m-%d").to_string();

                    Row::new(vec![
                        Span::styled(format!("{mark}{short_id}"), row_style),
                        Span::styled(p.name.clone(), row_style),
                        Span::styled(status_str, status_style),
                        Span::styled(created_str, Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let header_row = Row::new(vec![
                Span::styled(
                    "ID",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Name",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Status",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Created",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
            .bottom_margin(1);

            let table = Table::new(
                rows,
                [
                    Constraint::Length(10),
                    Constraint::Min(12),
                    Constraint::Length(10),
                    Constraint::Length(12),
                ],
            )
            .header(header_row)
            .block(list_block);
            frame.render_widget(table, list_area);
        }

        if compact {
            return;
        }

        // 2. Active Project Spec & Git Metadata Details
        let details_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Active Project Spec & Git Metadata ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let current_proj = state
            .available_projects
            .get(state.selected_project_index)
            .or(state.active_project.as_ref());

        if let Some(p) = current_proj {
            let total_tasks = state.review_tasks.len();
            let completed = state
                .review_tasks
                .iter()
                .filter(|t| t.task.status == TaskStatus::Completed)
                .count();
            let approved = state
                .review_tasks
                .iter()
                .filter(|t| t.task.status == TaskStatus::Approved)
                .count();
            let in_review = state
                .review_tasks
                .iter()
                .filter(|t| t.task.status == TaskStatus::HumanReview)
                .count();
            let executing = state
                .review_tasks
                .iter()
                .filter(|t| t.task.status == TaskStatus::Executing)
                .count();
            let failed = state
                .review_tasks
                .iter()
                .filter(|t| t.task.status == TaskStatus::Failed)
                .count();

            let lines = vec![
                Line::from(vec![
                    Span::styled("Project Name:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        &p.name,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" [{:?}]", p.status),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Repository:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "/workspace/AgentMesh",
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Base Branch:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled("master", Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("HEAD Commit:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled("9161054", Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled("Tasks Status:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{total_tasks} total | {completed} done | {approved} approved | {executing} active | {in_review} review | {failed} failed"),
                        Style::default().fg(Color::White),
                    ),
                ]),
            ];
            let p_widget = Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(details_block);
            frame.render_widget(p_widget, chunks[1]);
        } else {
            let empty = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No active project selected. Choose a project above or create a new one.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(details_block);
            frame.render_widget(empty, chunks[1]);
        }
    }

    fn render_input_form(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Project Definition & Planning Input ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let constraints = if inner_area.height < 12 {
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
        } else {
            [
                Constraint::Length(4),
                Constraint::Length(3),
                Constraint::Length(6),
                Constraint::Min(4),
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner_area);

        // Instructions
        let intro_text = vec![
            Line::from(vec![
                Span::styled(
                    "Enter project details below. Press ",
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    "[i]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" or ", Style::default().fg(Color::White)),
                Span::styled(
                    "[Enter]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " to start typing. Tab switches fields.",
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "[D] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Run Demo using the configured planner.",
                    Style::default().fg(Color::White),
                ),
            ]),
        ];
        let intro_p = Paragraph::new(intro_text).wrap(Wrap { trim: true });
        frame.render_widget(intro_p, chunks[0]);

        // Field 1: Name
        let is_name_focused =
            state.input_mode == InputMode::EnteringProject && state.project_focus_field == 0;
        let name_border_style = if is_name_focused {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let name_text = if is_name_focused {
            format!("{}_", state.project_name_input)
        } else if state.project_name_input.is_empty() {
            "e.g. Distributed Task Orchestrator".to_string()
        } else {
            state.project_name_input.clone()
        };

        let name_p = Paragraph::new(name_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(name_border_style)
                    .title(" Project Name "),
            )
            .style(if state.project_name_input.is_empty() && !is_name_focused {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            });
        frame.render_widget(name_p, chunks[1]);

        // Field 2: Description
        let is_desc_focused =
            state.input_mode == InputMode::EnteringProject && state.project_focus_field == 1;
        let desc_border_style = if is_desc_focused {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let desc_text = if is_desc_focused {
            format!("{}_", state.project_desc_input)
        } else if state.project_desc_input.is_empty() {
            "Describe the overall system goals, tech stack, and components to decompose..."
                .to_string()
        } else {
            state.project_desc_input.clone()
        };

        let desc_p = Paragraph::new(desc_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(desc_border_style)
                    .title(" Project Description "),
            )
            .wrap(Wrap { trim: false })
            .style(if state.project_desc_input.is_empty() && !is_desc_focused {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            });
        frame.render_widget(desc_p, chunks[2]);

        // Planning Contract Note
        let note_lines = vec![
            Line::from(vec![
                Span::styled(
                    "Planning Protocol Contract: ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("AI proposals are strictly advisory."),
            ]),
            Line::from(Span::styled(
                " • The AI generates task breakdown, dependencies, and affected resources.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " • All proposals pass deterministic cycle & validation checks before appearing.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " • No tasks are assigned or executed without human operator approval.",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let note_p = Paragraph::new(note_lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Protocol Guarantee "),
        );
        frame.render_widget(note_p, chunks[3]);
    }
}
