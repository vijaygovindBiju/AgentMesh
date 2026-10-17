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
        let popup_width = area.width.saturating_sub(4).min(75);
        let popup_height = area.height.saturating_sub(4).min(24);

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
                Span::styled(" [1]..[5] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Direct screen jump (1:Dash, 2:Proj, 3:Plan, 4:Fleet, 5:Diag)"),
            ]),
            Line::from(vec![
                Span::styled(" [Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Cycle screens  |  "),
                Span::styled(" [:] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Command palette  |  "),
                Span::styled(" [?] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Help cheatsheet  |  "),
                Span::styled(" [Q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("Quit"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 2: Plan Review & DAG ────────────────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(" [G] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Toggle ASCII DAG Graph view  |  "),
                Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
                Span::raw("Toggle details pane"),
            ]),
            Line::from(vec![
                Span::styled(" [Y] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Approve  |  "),
                Span::styled(" [N] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("Reject  |  "),
                Span::styled(" [E] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Inline edit description & approve"),
            ]),
            Line::from(vec![
                Span::styled(" [A] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Acknowledge overlap warning  |  "),
                Span::styled(" [C] ", Style::default().fg(Color::Red)),
                Span::raw("Cancel task"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 3: Dashboard & Fleet Inspection ─────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(" [← / →] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Switch focus between Agents, Tasks, and Overlaps tables"),
            ]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Select agent or task  |  "),
                Span::styled(" [C / Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Inspect agent capability profile"),
            ]),
            Line::from(vec![
                Span::styled(" [A] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Acknowledge resource overlap warning from any dashboard pane"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "─── Screen 4: Observability & Diagnostics ──────────────────────────────",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(vec![
                Span::styled(" [← / →] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Switch focus between Event Stream log and Task Timeline"),
            ]),
            Line::from(vec![
                Span::styled(" [↑ / ↓] ", Style::default().fg(Color::Cyan)),
                Span::raw("Scroll event timeline log (sliding window past 30+ events)"),
            ]),
            Line::from(vec![
                Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Open event payload viewer popup or full task lifecycle timeline"),
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
