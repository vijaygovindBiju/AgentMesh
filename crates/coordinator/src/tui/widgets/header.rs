use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::tui::state::{AppState, CurrentScreen};

pub struct HeaderWidget;

impl HeaderWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(26), // Title & version
                Constraint::Min(20),    // Active project info
                Constraint::Length(58), // Screen tabs
            ])
            .split(area);

        // Title
        let title_line = Line::from(vec![
            Span::styled(" AgentMesh ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("v0.1 ", Style::default().fg(Color::DarkGray)),
        ]);
        let title_p = Paragraph::new(title_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(title_p, chunks[0]);

        // Project
        let proj_name = state
            .active_project
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("No Project Selected");
        let proj_line = Line::from(vec![
            Span::styled(" Project: ", Style::default().fg(Color::DarkGray)),
            Span::styled(proj_name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]);
        let proj_p = Paragraph::new(proj_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(proj_p, chunks[1]);

        // Tabs
        let titles = vec![
            Line::from("1: Input"),
            Line::from("2: Review"),
            Line::from("3: Dashboard"),
            Line::from("4: Diagnostics"),
        ];
        let selected_idx = match state.current_screen {
            CurrentScreen::ProjectInput => 0,
            CurrentScreen::PlanReview => 1,
            CurrentScreen::Dashboard => 2,
            CurrentScreen::Diagnostics => 3,
        };

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .select(selected_idx)
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED),
            );
        frame.render_widget(tabs, chunks[2]);
    }
}
