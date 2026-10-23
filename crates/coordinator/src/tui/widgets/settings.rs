use crate::tui::state::AppState;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub struct SettingsWidget;

impl SettingsWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(2), Constraint::Length(2), Constraint::Min(1)])
            .split(area);
        frame.render_widget(Paragraph::new("Choose a theme and text style for the entire interface.")
            .block(Block::default().borders(Borders::ALL).title(" Appearance Settings ").border_style(Style::default().fg(Color::Cyan))), rows[0]);
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
        frame.render_widget(Paragraph::new(vec![theme, font]).block(Block::default().borders(Borders::LEFT | Borders::RIGHT)), rows[1]);
        frame.render_widget(
            Paragraph::new("↑/↓ select   ←/→ change   Esc returns to Dashboard\nFont family follows your terminal settings")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL)),
            rows[3],
        );
    }
}
