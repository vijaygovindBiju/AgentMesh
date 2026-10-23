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
        if area.width == 0 || area.height == 0 {
            return;
        }

        let is_compact = area.width < 85;

        let chunks = if is_compact {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(32),
                    Constraint::Length(24.min(area.width.saturating_sub(32))),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(65),
                    Constraint::Percentage(35),
                ])
                .split(area)
        };

        // Keybinding hints based on modals, input mode, screen, and focused pane
        let spans = if state.pending_confirmation.is_some() {
            vec![
                Span::styled(" [Y/Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Confirm  "),
                Span::styled(" [N/Esc] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]
        } else if state.show_command_palette {
            vec![
                Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Navigate  "),
                Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Execute  "),
                Span::styled(" [Esc] ", Style::default().fg(Color::Yellow)),
                Span::raw("Close"),
            ]
        } else if state.show_help_modal {
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
                InputMode::Normal => {
                    let has_selected_task = state.selected_task().is_some();
                    let has_overlaps = !state.active_overlaps.is_empty();

                    if is_compact {
                        // Prioritize top 4-5 actions on compact screens (<85 cols)
                        if matches!(
                            state.current_screen,
                            CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement
                        ) {
                            vec![
                                Span::styled(
                                    " [D] ",
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                                ),
                                Span::raw("Run Demo "),
                                Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Cmd "),
                                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Nav "),
                                Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                Span::raw("Help"),
                            ]
                        } else if state.current_screen == CurrentScreen::PlanReview && has_selected_task {
                            vec![
                                Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                Span::raw("Help "),
                                Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Cmd "),
                                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Nav "),
                                Span::styled(" [Y/N] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                                Span::raw("Review "),
                                Span::styled(" [Esc] ", Style::default().fg(Color::DarkGray)),
                                Span::raw("Back"),
                            ]
                        } else if has_overlaps {
                            vec![
                                Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                Span::raw("Help "),
                                Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Cmd "),
                                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Nav "),
                                Span::styled(" [A] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                                Span::raw("Ack "),
                                Span::styled(" [Esc] ", Style::default().fg(Color::DarkGray)),
                                Span::raw("Back"),
                            ]
                        } else {
                            vec![
                                Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                Span::raw("Help "),
                                Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Cmd "),
                                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Nav "),
                                Span::styled(" [Enter] ", Style::default().fg(Color::Green)),
                                Span::raw("Action "),
                                Span::styled(" [Esc] ", Style::default().fg(Color::DarkGray)),
                                Span::raw("Back"),
                            ]
                        }
                    } else {
                        // Standard / wide layout (>=85 cols)
                        match state.current_screen {
                            CurrentScreen::PlanReview => {
                                let dag_label = if state.show_dag_view { "Cards" } else { "DAG" };
                                let mut hints = vec![
                                    Span::styled(
                                        format!(" [G: {dag_label}] "),
                                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                                    ),
                                ];
                                if has_selected_task {
                                    hints.extend([
                                        Span::styled(" [Y] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                                        Span::raw("Approve "),
                                        Span::styled(" [Shift+A] ", Style::default().fg(Color::Green)),
                                        Span::raw("All "),
                                        Span::styled(" [N] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                                        Span::raw("Reject "),
                                        Span::styled(" [E] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                                        Span::raw("Edit "),
                                        Span::styled(" [C] ", Style::default().fg(Color::Red)),
                                        Span::raw("Cancel "),
                                    ]);
                                }
                                hints.extend([
                                    Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                                    Span::raw("Nav "),
                                    Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                                    Span::raw("Details "),
                                ]);
                                if has_overlaps {
                                    hints.extend([
                                        Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                                        Span::raw("Ack "),
                                    ]);
                                }
                                hints.extend([
                                    Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                    Span::raw("Cmd "),
                                    Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                    Span::raw("Help "),
                                    Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                    Span::raw("Quit"),
                                ]);
                                hints
                            }
                            CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => vec![
                                Span::styled(
                                    " [D] ",
                                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                                ),
                                Span::raw("Run Demo  "),
                                Span::styled(
                                    " [Enter/i] ",
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::raw("Start Input  "),
                                Span::styled(" [Tab] ", Style::default().fg(Color::Blue)),
                                Span::raw("Next Screen  "),
                                Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                Span::raw("Cmd  "),
                                Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                Span::raw("Help  "),
                                Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                Span::raw("Quit"),
                            ],
                            CurrentScreen::Dashboard | CurrentScreen::FleetInspector => {
                                match state.dashboard_focus {
                                    crate::tui::state::DashboardFocus::AgentsTable => {
                                        let mut hints = vec![
                                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Focus  "),
                                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Select Agent  "),
                                            Span::styled(" [C/Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                                            Span::raw("Inspect  "),
                                        ];
                                        if has_overlaps {
                                            hints.extend([
                                                Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                                                Span::raw("Ack Overlap  "),
                                            ]);
                                        }
                                        hints.extend([
                                            Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Cmd  "),
                                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                            Span::raw("Help  "),
                                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                            Span::raw("Quit"),
                                        ]);
                                        hints
                                    }
                                    crate::tui::state::DashboardFocus::TasksTable => {
                                        let mut hints = vec![
                                            Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Focus  "),
                                            Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Select Task  "),
                                            Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Inspect  "),
                                        ];
                                        if has_overlaps {
                                            hints.extend([
                                                Span::styled(" [A] ", Style::default().fg(Color::Yellow)),
                                                Span::raw("Ack Overlap  "),
                                            ]);
                                        }
                                        hints.extend([
                                            Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                            Span::raw("Cmd  "),
                                            Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                            Span::raw("Help  "),
                                            Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                            Span::raw("Quit"),
                                        ]);
                                        hints
                                    }
                                    crate::tui::state::DashboardFocus::OverlapsTable => vec![
                                        Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Focus  "),
                                        Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Select Overlap  "),
                                        Span::styled(" [A/Enter] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                                        Span::raw("Acknowledge  "),
                                        Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Cmd  "),
                                        Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                        Span::raw("Help  "),
                                        Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                        Span::raw("Quit"),
                                    ],
                                }
                            }
                            CurrentScreen::Diagnostics => {
                                match state.diagnostics_focus {
                                    crate::tui::state::DiagnosticsFocus::EventsStream => vec![
                                        Span::styled(" [←/→] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Focus  "),
                                        Span::styled(" [↑/↓] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Scroll Log  "),
                                        Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                                        Span::raw("Payload  "),
                                        Span::styled(" [R] ", Style::default().fg(Color::Green)),
                                        Span::raw("Refresh  "),
                                        Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Cmd  "),
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
                                        Span::styled(" [:] ", Style::default().fg(Color::Cyan)),
                                        Span::raw("Cmd  "),
                                        Span::styled(" [?] ", Style::default().fg(Color::Yellow)),
                                        Span::raw("Help  "),
                                        Span::styled(" [Q] ", Style::default().fg(Color::DarkGray)),
                                        Span::raw("Quit"),
                                    ],
                                }
                            }
                        }
                    }
                }
            }
        };

        let hints_p = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(if area.height >= 3 { Borders::ALL } else { Borders::NONE })
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(hints_p, chunks[0]);

        // Status message
        if chunks[1].width > 0 {
            let status_text = state
                .status_message
                .as_deref()
                .unwrap_or("Ready. One-human operator mode active.");

            let lower = status_text.to_lowercase();
            let (icon, icon_color, text_color) = if lower.contains("fail")
                || lower.contains("error")
                || lower.contains("cancel")
                || lower.starts_with('✖')
            {
                ("✖ ", Color::Red, Color::LightRed)
            } else if lower.contains("warn")
                || lower.contains("block")
                || lower.contains("overlap")
                || lower.starts_with('⚠')
            {
                ("⚠ ", Color::Yellow, Color::LightYellow)
            } else if lower.contains("complet")
                || lower.contains("approv")
                || lower.contains("success")
                || lower.contains("refresh")
                || lower.contains("acknowledg")
                || lower.starts_with('✔')
            {
                ("✔ ", Color::Green, Color::LightGreen)
            } else {
                ("ℹ ", Color::Cyan, Color::White)
            };

            let status_p = Paragraph::new(Line::from(vec![
                Span::styled(" Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(icon, Style::default().fg(icon_color).add_modifier(Modifier::BOLD)),
                Span::styled(status_text, Style::default().fg(text_color)),
            ]))
            .block(
                Block::default()
                    .borders(if area.height >= 3 { Borders::ALL } else { Borders::NONE })
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(status_p, chunks[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::domain::{OverlapSeverity, OverlapWarning};
    use uuid::Uuid;

    #[test]
    fn test_footer_compact_mode_prioritizes_top_actions() {
        let backend = TestBackend::new(75, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        let state = AppState::new();

        terminal
            .draw(|f| {
                FooterWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content().iter().map(|c| c.symbol()).collect();

        assert!(rendered.contains("[?]"));
        assert!(rendered.contains("Help"));
        assert!(rendered.contains("[:]"));
        assert!(rendered.contains("Cmd"));
        assert!(rendered.contains("[Tab]"));
        assert!(rendered.contains("Nav"));
    }

    #[test]
    fn test_footer_conditional_ack_overlap_hint() {
        let backend = TestBackend::new(120, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        // 1. Without overlaps: no [A] Ack
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::Dashboard;
        state.active_overlaps.clear();

        terminal
            .draw(|f| {
                FooterWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buf1 = terminal.backend().buffer();
        let rend1: String = buf1.content().iter().map(|c| c.symbol()).collect();
        assert!(!rend1.contains("[A]"));

        // 2. With active overlaps: [A] Ack Overlap is shown
        state.active_overlaps.push(OverlapWarning {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            resource: "crates/test".to_string(),
            task_ids: serde_json::json!([]),
            severity: OverlapSeverity::Critical,
            acknowledged: false,
            created_at: chrono::Utc::now(),
        });

        terminal
            .draw(|f| {
                FooterWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buf2 = terminal.backend().buffer();
        let rend2: String = buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(rend2.contains("[A]"));
        assert!(rend2.contains("Ack Overlap"));
    }

    #[test]
    fn test_footer_modal_hints() {
        let backend = TestBackend::new(100, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.show_command_palette = true;

        terminal
            .draw(|f| {
                FooterWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let rend: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(rend.contains("[↑/↓]"));
        assert!(rend.contains("Navigate"));
        assert!(rend.contains("[Enter]"));
        assert!(rend.contains("Execute"));
    }

    #[test]
    fn test_footer_confirmation_modal_hints() {
        let backend = TestBackend::new(100, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.pending_confirmation = Some(crate::tui::ConfirmationModalState {
            title: "Confirm".to_string(),
            message: "Are you sure?".to_string(),
            action: crate::tui::DestructiveAction::CancelTask {
                task_id: Uuid::new_v4(),
                short_id: "TASK-1".to_string(),
            },
        });

        terminal
            .draw(|f| {
                FooterWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let rend: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(rend.contains("[Y/Enter]"));
        assert!(rend.contains("Confirm"));
        assert!(rend.contains("[N/Esc]"));
        assert!(rend.contains("Cancel"));
    }
}

