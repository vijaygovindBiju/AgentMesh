use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::state::{AppState, CurrentScreen, InputMode};

pub struct FooterWidget;

impl FooterWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(65), // Contextual keybindings
                Constraint::Percentage(35), // Status / notification message
            ])
            .split(area);

        // Keybinding hints based on modals, input mode, screen, and focused pane
        let spans = if state.show_help_modal {
            vec![
                Span::styled(" [?] / [Esc] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Close Help Reference  "),
                Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                Span::raw("Quit"),
            ]
        } else if state.show_agent_inspector {
            vec![
                Span::styled(" [Esc] / [C] / [Enter] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Close Capability Inspector  "),
                Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                Span::raw("Quit"),
            ]
        } else if state.show_event_payload_modal {
            vec![
                Span::styled(" [Esc] / [Enter] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Close Payload Viewer  "),
                Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                Span::raw("Quit"),
            ]
        } else if state.show_timeline_modal {
            vec![
                Span::styled(" [Esc] / [Enter] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Close Lifecycle Timeline  "),
                Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                Span::raw("Quit"),
            ]
        } else {
            match state.input_mode {
                InputMode::EditingDescription => vec![
                    Span::styled(
                        " [Enter] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Confirm & Approve  "),
                    Span::styled(
                        " [Esc] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Cancel Edit  "),
                ],
                InputMode::EnteringProject => vec![
                    Span::styled(
                        " [Tab] ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Switch Field  "),
                    Span::styled(
                        " [Enter] ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Submit  "),
                    Span::styled(
                        " [Esc] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("Cancel  "),
                ],
                InputMode::Normal => match state.current_screen {
                    CurrentScreen::PlanReview => {
                        let dag_label = if state.show_dag_view { "Cards" } else { "DAG" };
                        vec![
                            Span::styled(
                                format!(" [G: {dag_label}] "),
                                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                " [Y] ",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw("Approve "),
                            Span::styled(
                                " [N] ",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw("Reject "),
                            Span::styled(
                                " [E] ",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw("Edit "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Nav "),
                            Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Details "),
                            Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Ack "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ]
                    }
                    CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => vec![
                        Span::styled(
                            " [Enter/i] ",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("Start Input  "),
                        Span::styled(" [Tab] ", Style::default().fg(Color::Blue)),
                        Span::raw("Next Screen  "),
                        Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                        Span::raw("Help  "),
                        Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                        Span::raw("Quit"),
                    ],
                    CurrentScreen::Dashboard | CurrentScreen::FleetInspector => match state.dashboard_focus {
                        crate::tui::state::DashboardFocus::AgentsTable => vec![
                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Focus  "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Select Agent  "),
                            Span::styled(" [C/Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            Span::raw("Inspect  "),
                            Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Ack Overlap  "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help  "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ],
                        crate::tui::state::DashboardFocus::TasksTable => vec![
                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Focus  "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Select Task  "),
                            Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Inspect  "),
                            Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Ack Overlap  "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help  "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ],
                        crate::tui::state::DashboardFocus::OverlapsTable => vec![
                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Focus  "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Select Overlap  "),
                            Span::styled(" [A/Enter] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                            Span::raw("Acknowledge  "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help  "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ],
                    },
                    CurrentScreen::Diagnostics => match state.diagnostics_focus {
                        crate::tui::state::DiagnosticsFocus::EventsStream => vec![
                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Focus  "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Scroll Log  "),
                            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            Span::raw("Payload  "),
                            Span::styled(" [R] ", Style::default().fg(Color::Green)),
                            Span::raw("Refresh  "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help  "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ],
                        crate::tui::state::DiagnosticsFocus::Timeline => vec![
                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Focus  "),
                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                            Span::raw("Select Task  "),
                            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            Span::raw("Full Stages  "),
                            Span::styled(" [R] ", Style::default().fg(Color::Green)),
                            Span::raw("Refresh  "),
                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                            Span::raw("Help  "),
                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                            Span::raw("Quit"),
                        ],
                    },
                },
            }
        };

        let hints_p = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(hints_p, chunks[0]);

        // Status message
        let status_text = state
            .status_message
            .as_deref()
            .unwrap_or("Ready. One-human operator mode active.");
        let status_p = Paragraph::new(Line::from(vec![
            Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(status_text, Style::default().fg(Color::White)),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(status_p, chunks[1]);
    }
}
