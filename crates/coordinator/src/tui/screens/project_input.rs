use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::tui::state::{AppState, InputMode};

pub struct ProjectInputScreen;

impl ProjectInputScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
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

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Instructions
                Constraint::Length(3), // Project Name field
                Constraint::Length(8), // Project Description field
                Constraint::Min(4),    // Information / Hints
            ])
            .split(inner_area);

        // Instructions
        let intro_text = Line::from(vec![
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
        ]);
        let intro_p = Paragraph::new(intro_text);
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
        let note_p = Paragraph::new(note_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Protocol Guarantee "),
        );
        frame.render_widget(note_p, chunks[3]);
    }
}
