use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::state::AppState;

pub struct HelpModalWidget;

impl HelpModalWidget {
    pub fn render(frame: &mut Frame, area: Rect, _state: &AppState) {
        let popup_width = area.width.saturating_sub(4).min(78);
        let popup_height = area.height.saturating_sub(2).min(26);

        if popup_width < 10 || popup_height < 5 {
            return;
        }

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
                " Operator Keyboard Shortcuts & Help Reference (? to close) ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    " [1]..[5] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Direct screen jump  |  "),
                Span::styled(
                    " [Tab / BackTab / [ / ]] ",
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("Cycle screens"),
            ]),
            Line::from(vec![
                Span::styled(
                    " Mouse ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Click tabs to jump, wheel scroll across lists & DAG"),
            ]),
            Line::from(vec![
                Span::styled(
                    " [:] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Command palette  |  "),
                Span::styled(
                    " [?] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Help cheatsheet  |  "),
                Span::styled(
                    " [Q] ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Quit"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 1: Dashboard ────────────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(
                    " [← / →] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Switch pane focus (Agents, Tasks, Overlaps)  |  "),
                Span::styled(
                    " [A] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Ack overlap"),
            ]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Select item  |  "),
                Span::styled(
                    " [C / Enter] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Inspect agent capability profile"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 2: Project Management & Input ───────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Navigate projects  |  "),
                Span::styled(
                    " [Enter] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Load project  |  "),
                Span::styled(
                    " [I] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Create new project"),
            ]),
            Line::from(vec![
                Span::styled(
                    " [D] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Run the built-in demo project using the configured planner"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 3: Plan Review & DAG Graph ──────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(
                    " [G] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Toggle ASCII DAG Graph view  |  "),
                Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                Span::raw("Toggle details pane"),
            ]),
            Line::from(vec![
                Span::styled(
                    " [Y] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Approve  |  "),
                Span::styled(
                    " [Shift+A] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Approve ALL  |  "),
                Span::styled(
                    " [N] ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Reject"),
            ]),
            Line::from(vec![
                Span::styled(
                    " [E] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Edit  |  "),
                Span::styled(" [R] ", Style::default().fg(Color::Cyan)),
                Span::raw("Retry  |  "),
                Span::styled(" [C] ", Style::default().fg(Color::Red)),
                Span::raw("Cancel  |  "),
                Span::styled(" [ [ / ] ] ", Style::default().fg(Color::Cyan)),
                Span::raw("Blocker/Dep"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 4: Fleet Inspector ──────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Select agent  |  "),
                Span::styled(
                    " [C / Enter] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Inspect agent capabilities & active task assignment"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 5: Observability & Diagnostics ──────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(
                    " [← / →] ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Switch stream / timeline  |  "),
                Span::styled(
                    " [F] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Severity filter  |  "),
                Span::styled(
                    " [E] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Entity filter"),
            ]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Scroll event stream  |  "),
                Span::styled(
                    " [Enter] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("Inspect payload / task lifecycle drilldown"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press [?] or [Esc] to dismiss this help reference.",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }
}
