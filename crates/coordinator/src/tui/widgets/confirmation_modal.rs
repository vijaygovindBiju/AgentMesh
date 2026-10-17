use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::state::{AppState, ConfirmationModalState, DestructiveAction};

pub struct ConfirmationModalWidget;

impl ConfirmationModalWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if let Some(ref dialog) = state.pending_confirmation {
            Self::render_dialog(frame, area, dialog);
        }
    }

    pub fn render_dialog(frame: &mut Frame, area: Rect, dialog: &ConfirmationModalState) {
        let width = (area.width * 60 / 100).clamp(40, 70).min(area.width);
        let height = 8.min(area.height);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let modal_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, modal_area);

        let (action_badge, border_color) = match &dialog.action {
            DestructiveAction::CancelTask { short_id, .. } => (format!("Cancel [{short_id}]"), Color::Red),
            DestructiveAction::RejectTask { short_id, .. } => (format!("Reject [{short_id}]"), Color::Red),
            DestructiveAction::ApproveAllTasks => ("Approve All".to_string(), Color::Green),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Span::styled(
                format!(" ⚠ {} — {} ", dialog.title, action_badge),
                Style::default().fg(border_color).add_modifier(Modifier::BOLD),
            ))
            .border_style(Style::default().fg(border_color));
        frame.render_widget(block, modal_area);

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(2), Constraint::Length(1)])
            .split(modal_area);

        let msg = Paragraph::new(dialog.message.as_str())
            .style(Style::default().fg(Color::White));
        frame.render_widget(msg, inner[0]);

        let buttons = Line::from(vec![
            Span::styled(
                " [Y/Enter] Confirm ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                " [N/Esc] Cancel ",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(buttons), inner[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use uuid::Uuid;

    #[test]
    fn test_confirmation_modal_renders_expected_text() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let task_id = Uuid::new_v4();
        let dialog = ConfirmationModalState {
            title: "Confirm Task Cancellation".to_string(),
            message: "Are you sure you want to cancel task PLAN-3?".to_string(),
            action: DestructiveAction::CancelTask {
                task_id,
                short_id: "PLAN-3".to_string(),
            },
        };

        let mut state = AppState::new();
        state.pending_confirmation = Some(dialog);

        terminal
            .draw(|f| ConfirmationModalWidget::render(f, f.area(), &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        let content: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Confirm Task Cancellation"));
        assert!(content.contains("Are you sure you want to cancel task PLAN-3?"));
        assert!(content.contains("[Y/Enter] Confirm"));
        assert!(content.contains("[N/Esc] Cancel"));
    }
}
