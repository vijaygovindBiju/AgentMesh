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
        if area.width == 0 || area.height == 0 {
            return;
        }

        let is_compact = area.width < 90;

        // Connectivity and status pills
        let pg_pill = if state.pg_connected {
            Span::styled("[PG: ● OK]", Style::default().fg(Color::Green))
        } else {
            Span::styled("[PG: ○ OFF]", Style::default().fg(Color::Red))
        };

        let nats_pill = if state.nats_connected {
            Span::styled("[NATS: ● OK]", Style::default().fg(Color::Green))
        } else {
            Span::styled("[NATS: ○ OFF]", Style::default().fg(Color::Red))
        };

        let (active_agents, total_agents) = state.active_agent_count();
        let agent_ratio = Span::styled(
            format!("[Agents: {active_agents}/{total_agents}]"),
            Style::default().fg(Color::Cyan),
        );

        let pending_count = state.pending_approvals_count();
        let pending_pill = Span::styled(
            format!("[Pending: {pending_count}]"),
            if pending_count > 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        );

        let proj_name = state
            .active_project
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("No Project Selected");

        let tab_titles = vec![
            Line::from("1:Dash"),
            Line::from("2:Proj"),
            Line::from("3:Plan"),
            Line::from("4:Fleet"),
            Line::from("5:Diag"),
        ];

        let selected_idx = match state.current_screen {
            CurrentScreen::Dashboard => 0,
            CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => 1,
            CurrentScreen::PlanReview => 2,
            CurrentScreen::FleetInspector => 3,
            CurrentScreen::Diagnostics => 4,
        };

        if is_compact {
            // 2-line header layout on compact terminals (<90 cols)
            if area.height >= 2 {
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(1)])
                    .split(area);

                // Row 1: Brand/version + Status pills
                let mut row1_spans = vec![
                    Span::styled(
                        "AgentMesh ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                if proj_name != "No Project Selected" {
                    row1_spans.push(Span::styled(
                        format!("[{proj_name}] "),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                row1_spans.extend([
                    pg_pill,
                    Span::raw(" "),
                    nats_pill,
                    Span::raw(" "),
                    agent_ratio,
                    Span::raw(" "),
                    pending_pill,
                ]);
                let row1_p = Paragraph::new(Line::from(row1_spans));
                frame.render_widget(row1_p, rows[0]);

                // Row 2: Compact screen tabs
                let tabs = Tabs::new(tab_titles)
                    .select(selected_idx)
                    .style(Style::default().fg(Color::Gray))
                    .highlight_style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED),
                    );
                frame.render_widget(tabs, rows[1]);
            } else {
                // Extreme vertical constraint (1 line)
                let row1_spans = vec![
                    Span::styled(
                        "AgentMesh ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    pg_pill,
                    Span::raw(" "),
                    nats_pill,
                    Span::raw(" "),
                    agent_ratio,
                    Span::raw(" "),
                    pending_pill,
                ];
                let row1_p = Paragraph::new(Line::from(row1_spans));
                frame.render_widget(row1_p, area);
            }
        } else {
            // 3 horizontal chunks on standard/wide terminals (>=90 cols)
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(22), // Brand/version
                    Constraint::Min(25),    // Active project name & status pills
                    Constraint::Length(52), // Screen tabs
                ])
                .split(area);

            // Chunk 0: Brand / version
            let brand_line = Line::from(vec![
                Span::styled(
                    " AgentMesh ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("v0.1 ", Style::default().fg(Color::DarkGray)),
            ]);
            let brand_p = Paragraph::new(brand_line).block(
                Block::default()
                    .borders(if area.height >= 3 { Borders::ALL } else { Borders::NONE })
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(brand_p, chunks[0]);

            // Chunk 1: Active project name & status pills
            let inner_width = chunks[1].width.saturating_sub(if area.height >= 3 { 2 } else { 0 });
            let status_lines = if inner_width >= 75 {
                vec![Line::from(vec![
                    Span::styled(" Project: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{proj_name} "),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    pg_pill,
                    Span::raw(" "),
                    nats_pill,
                    Span::raw(" "),
                    agent_ratio,
                    Span::raw(" "),
                    pending_pill,
                ])]
            } else if area.height >= 3 {
                vec![
                    Line::from(vec![
                        Span::styled(" Project: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{proj_name} "),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        pg_pill,
                    ]),
                    Line::from(vec![
                        nats_pill,
                        Span::raw(" "),
                        agent_ratio,
                        Span::raw(" "),
                        pending_pill,
                    ]),
                ]
            } else {
                vec![Line::from(vec![
                    pg_pill,
                    Span::raw(" "),
                    nats_pill,
                    Span::raw(" "),
                    agent_ratio,
                    Span::raw(" "),
                    pending_pill,
                ])]
            };

            let proj_p = Paragraph::new(status_lines).block(
                Block::default()
                    .borders(if area.height >= 3 { Borders::ALL } else { Borders::NONE })
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(proj_p, chunks[1]);

            // Chunk 2: Screen tabs
            let tabs = Tabs::new(tab_titles)
                .block(
                    Block::default()
                        .borders(if area.height >= 3 { Borders::ALL } else { Borders::NONE })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::domain::Project;
    use uuid::Uuid;

    #[test]
    fn test_header_renders_connectivity_pills_healthy() {
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.pg_connected = true;
        state.nats_connected = true;
        state.active_project = Some(Project {
            id: Uuid::new_v4(),
            name: "Cloud Mesh".to_string(),
            description: "Test".to_string(),
            status: crate::domain::ProjectStatus::Planning,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });

        terminal
            .draw(|f| {
                HeaderWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(rendered.contains("[PG: ● OK]"));
        assert!(rendered.contains("[NATS: ● OK]"));
        assert!(rendered.contains("Cloud Mesh"));
    }

    #[test]
    fn test_header_renders_connectivity_pills_offline() {
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState::new(); // pg_connected = false, nats_connected = false

        terminal
            .draw(|f| {
                HeaderWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(rendered.contains("[PG: ○ OFF]"));
        assert!(rendered.contains("[NATS: ○ OFF]"));
    }

    #[test]
    fn test_header_compact_render_and_minimal_dimensions_safety() {
        let backend = TestBackend::new(75, 4);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.pg_connected = true;
        state.nats_connected = true;

        terminal
            .draw(|f| {
                HeaderWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();
        assert!(rendered.contains("AgentMesh"));
        assert!(rendered.contains("[PG: ● OK]"));

        // Minimal dimensions test (60x2)
        let backend_min = TestBackend::new(60, 2);
        let mut term_min = Terminal::new(backend_min).unwrap();
        term_min
            .draw(|f| {
                HeaderWidget::render(f, f.area(), &state);
            })
            .unwrap();
        let buf_min = term_min.backend().buffer();
        let rend_min: String = buf_min.content().iter().map(|c| c.symbol()).collect();
        assert!(rend_min.contains("AgentMesh"));
    }
}

