use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table},
    Frame,
};

use crate::tui::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandItem {
    pub name: String,
    pub description: String,
    pub category: String,
}

/// Returns the master list of all categorized operator commands.
pub fn all_commands() -> Vec<CommandItem> {
    vec![
        // Navigation commands
        CommandItem {
            name: "nav:dashboard".to_string(),
            description: "Go to Dashboard".to_string(),
            category: "Nav".to_string(),
        },
        CommandItem {
            name: "nav:projects".to_string(),
            description: "Go to Projects".to_string(),
            category: "Nav".to_string(),
        },
        CommandItem {
            name: "nav:plan".to_string(),
            description: "Go to Plan Review".to_string(),
            category: "Nav".to_string(),
        },
        CommandItem {
            name: "nav:fleet".to_string(),
            description: "Go to Fleet Inspector".to_string(),
            category: "Nav".to_string(),
        },
        CommandItem {
            name: "nav:diagnostics".to_string(),
            description: "Go to Diagnostics".to_string(),
            category: "Nav".to_string(),
        },
        // Task commands
        CommandItem {
            name: "task:approve".to_string(),
            description: "Approve Task".to_string(),
            category: "Task".to_string(),
        },
        CommandItem {
            name: "task:approve_all".to_string(),
            description: "Approve All Tasks".to_string(),
            category: "Task".to_string(),
        },
        CommandItem {
            name: "task:reject".to_string(),
            description: "Reject Task".to_string(),
            category: "Task".to_string(),
        },
        CommandItem {
            name: "task:cancel".to_string(),
            description: "Cancel Task".to_string(),
            category: "Task".to_string(),
        },
        CommandItem {
            name: "task:retry".to_string(),
            description: "Retry Task".to_string(),
            category: "Task".to_string(),
        },
        CommandItem {
            name: "task:edit".to_string(),
            description: "Edit Description".to_string(),
            category: "Task".to_string(),
        },
        // System commands
        CommandItem {
            name: "system:refresh".to_string(),
            description: "Refresh Data".to_string(),
            category: "System".to_string(),
        },
        CommandItem {
            name: "system:help".to_string(),
            description: "Open Help".to_string(),
            category: "System".to_string(),
        },
        CommandItem {
            name: "system:quit".to_string(),
            description: "Quit".to_string(),
            category: "System".to_string(),
        },
    ]
}

/// Case-insensitive substring filtering across name, description, and category.
pub fn filtered_commands(query: &str) -> Vec<CommandItem> {
    let q = query.trim().to_lowercase();
    all_commands()
        .into_iter()
        .filter(|cmd| {
            if q.is_empty() {
                true
            } else {
                cmd.name.to_lowercase().contains(&q)
                    || cmd.description.to_lowercase().contains(&q)
                    || cmd.category.to_lowercase().contains(&q)
            }
        })
        .collect()
}

pub struct CommandPaletteWidget;

impl CommandPaletteWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if !state.show_command_palette {
            return;
        }

        let width = (area.width * 70 / 100).clamp(40, 75).min(area.width);
        let height = (area.height * 70 / 100).clamp(10, 20).min(area.height);

        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let modal_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Command Palette (:) ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(modal_area);
        frame.render_widget(block, modal_area);

        if inner.height < 3 || inner.width < 10 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // input prompt
                Constraint::Length(1), // divider
                Constraint::Min(1),    // command list
            ])
            .split(inner);

        // Input prompt
        let input_line = Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(&state.command_palette_query, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[0]);

        // Divider
        let divider = Paragraph::new("─".repeat(chunks[1].width as usize))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(divider, chunks[1]);

        // Filtered commands list
        let matches = filtered_commands(&state.command_palette_query);
        let max_visible = chunks[2].height as usize;
        let selected_idx = state.command_palette_selected_index;

        let scroll_offset = if selected_idx >= max_visible {
            selected_idx.saturating_sub(max_visible.saturating_sub(1))
        } else {
            0
        };

        let visible_items = matches.iter().skip(scroll_offset).take(max_visible);
        let rows: Vec<Row> = visible_items
            .enumerate()
            .map(|(rel_idx, cmd)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_sel = actual_idx == selected_idx;
                let prefix = if is_sel { "▶ " } else { "  " };
                let cat_badge = match cmd.category.as_str() {
                    "Nav" => Span::styled("[Nav]", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                    "Task" => Span::styled("[Task]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    "System" => Span::styled("[System]", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    _ => Span::styled(format!("[{}]", cmd.category), Style::default().fg(Color::DarkGray)),
                };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Row::new(vec![
                    Span::styled(format!("{prefix}{}", cmd.description), style),
                    cat_badge,
                    Span::styled(format!(":{}", cmd.name), Style::default().fg(Color::DarkGray)),
                ])
            })
            .collect();

        if rows.is_empty() {
            let empty_p = Paragraph::new(Line::from(vec![
                Span::styled("  No matching commands found.", Style::default().fg(Color::DarkGray)),
            ]));
            frame.render_widget(empty_p, chunks[2]);
        } else {
            let table = Table::new(
                rows,
                [
                    Constraint::Min(20),
                    Constraint::Length(10),
                    Constraint::Length(18),
                ],
            );
            frame.render_widget(table, chunks[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use crate::tui::state::{CurrentScreen, TuiAction};

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_all_commands_contain_required_categorized_actions() {
        let cmds = all_commands();
        assert!(cmds.len() >= 13);

        // [Nav] commands
        assert!(cmds.iter().any(|c| c.name == "nav:dashboard" && c.category == "Nav"));
        assert!(cmds.iter().any(|c| c.name == "nav:projects" && c.category == "Nav"));
        assert!(cmds.iter().any(|c| c.name == "nav:plan" && c.category == "Nav"));
        assert!(cmds.iter().any(|c| c.name == "nav:fleet" && c.category == "Nav"));
        assert!(cmds.iter().any(|c| c.name == "nav:diagnostics" && c.category == "Nav"));

        // [Task] commands
        assert!(cmds.iter().any(|c| c.name == "task:approve" && c.category == "Task"));
        assert!(cmds.iter().any(|c| c.name == "task:reject" && c.category == "Task"));
        assert!(cmds.iter().any(|c| c.name == "task:cancel" && c.category == "Task"));
        assert!(cmds.iter().any(|c| c.name == "task:retry" && c.category == "Task"));
        assert!(cmds.iter().any(|c| c.name == "task:edit" && c.category == "Task"));

        // [System] commands
        assert!(cmds.iter().any(|c| c.name == "system:refresh" && c.category == "System"));
        assert!(cmds.iter().any(|c| c.name == "system:help" && c.category == "System"));
        assert!(cmds.iter().any(|c| c.name == "system:quit" && c.category == "System"));
    }

    #[test]
    fn test_filtered_commands_case_insensitive_matching() {
        assert_eq!(filtered_commands("").len(), all_commands().len());

        let dash = filtered_commands("DASH");
        assert_eq!(dash.len(), 1);
        assert_eq!(dash[0].name, "nav:dashboard");

        let fleet = filtered_commands("FlEeT");
        assert_eq!(fleet.len(), 1);
        assert_eq!(fleet[0].name, "nav:fleet");

        let task = filtered_commands("Task");
        assert_eq!(task.len(), 6);
        assert!(task.iter().any(|command| command.name == "task:approve_all"));

        let none = filtered_commands("xyz_nonexistent_token");
        assert!(none.is_empty());
    }

    #[test]
    fn test_command_palette_render_rendering_and_safety() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.show_command_palette = true;
        state.command_palette_query = "fleet".to_string();

        terminal
            .draw(|f| {
                CommandPaletteWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered_text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered_text.contains("Command Palette (:)"));
        assert!(rendered_text.contains("fleet"));
        assert!(rendered_text.contains("Fleet Inspector"));
    }

    #[test]
    fn test_command_palette_render_empty_matches_does_not_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.show_command_palette = true;
        state.command_palette_query = "nonexistent_query".to_string();

        terminal
            .draw(|f| {
                CommandPaletteWidget::render(f, f.area(), &state);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let rendered_text: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered_text.contains("No matching commands found."));
    }

    #[test]
    fn test_command_palette_key_handling_workflow() {
        let mut state = AppState::new();
        assert!(!state.show_command_palette);

        // Open palette with ':'
        state.handle_key(make_key(KeyCode::Char(':')));
        assert!(state.show_command_palette);
        assert_eq!(state.command_palette_query, "");
        assert_eq!(state.command_palette_selected_index, 0);

        // Type "fleet"
        for c in "fleet".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }
        assert_eq!(state.command_palette_query, "fleet");

        // Backspace
        state.handle_key(make_key(KeyCode::Backspace));
        assert_eq!(state.command_palette_query, "flee");

        // Navigate Down/Up
        state.handle_key(make_key(KeyCode::Down));
        state.handle_key(make_key(KeyCode::Up));

        // Close with Esc
        state.handle_key(make_key(KeyCode::Esc));
        assert!(!state.show_command_palette);

        // Reopen and execute nav:fleet
        state.handle_key(make_key(KeyCode::Char(':')));
        for c in "fleet".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }
        let action = state.handle_key(make_key(KeyCode::Enter));
        assert_eq!(action, None);
        assert_eq!(state.current_screen, CurrentScreen::FleetInspector);
        assert!(!state.show_command_palette);

        // Reopen and execute system:quit
        state.handle_key(make_key(KeyCode::Char(':')));
        for c in "quit".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }
        let quit_action = state.handle_key(make_key(KeyCode::Enter));
        assert_eq!(quit_action, Some(TuiAction::Quit));
        assert!(state.should_quit);
    }
}

