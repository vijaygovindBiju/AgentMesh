use crate::tui::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct SettingsWidget;

impl SettingsWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let width = 56.min(area.width.saturating_sub(4));
        let height = 12.min(area.height.saturating_sub(2));
        let popup = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(popup);
        frame.render_widget(
            Paragraph::new("Appearance")
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Settings ")
                        .border_style(Style::default().fg(Color::Cyan)),
                ),
            rows[0],
        );
        let theme = Line::from(vec![
            Span::raw(if state.settings_selected_index == 0 {
                "> "
            } else {
                "  "
            }),
            Span::styled("Theme: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(state.theme.name()),
        ]);
        let font = Line::from(vec![
            Span::raw(if state.settings_selected_index == 1 {
                "> "
            } else {
                "  "
            }),
            Span::styled(
                "Font style: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(state.font_style.name()),
        ]);
        frame.render_widget(
            Paragraph::new(vec![theme, font])
                .block(Block::default().borders(Borders::LEFT | Borders::RIGHT)),
            rows[1],
        );
        frame.render_widget(
            Paragraph::new(
                "↑/↓ select   ←/→ change   Esc close\nFont family follows your terminal settings",
            )
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
            rows[3],
        );
    }
}
