use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::Paragraph,
    Frame,
};

use crate::tui::screens::{
    DashboardScreen, DiagnosticsScreen, PlanReviewScreen, ProjectInputScreen,
};
use crate::tui::state::{AppState, CurrentScreen};
use crate::tui::widgets::{
    CommandPaletteWidget, ConfirmationModalWidget, FooterWidget, HeaderWidget, HelpModalWidget,
};

/// Top-level layout orchestrator
pub fn render(frame: &mut Frame, state: &AppState) {
    let size = frame.area();

    // Extreme size guard: terminal below minimum operational bounds
    if size.width < 48 || size.height < 10 {
        let msg = format!("Terminal too small ({}x{}, min 60x18)", size.width, size.height);
        let p = Paragraph::new(msg)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(p, size);
        return;
    }

    let header_height = if size.width < 90 { 2 } else { 3 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Header bar
            Constraint::Min(6),                // Main body area
            Constraint::Length(3),             // Footer key hints and status
        ])
        .split(size);

    // 1. Header
    HeaderWidget::render(frame, chunks[0], state);

    // 2. Main Screen Body
    match state.current_screen {
        CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => {
            ProjectInputScreen::render(frame, chunks[1], state)
        }
        CurrentScreen::PlanReview => PlanReviewScreen::render(frame, chunks[1], state),
        CurrentScreen::Dashboard | CurrentScreen::FleetInspector => {
            DashboardScreen::render(frame, chunks[1], state)
        }
        CurrentScreen::Diagnostics => DiagnosticsScreen::render(frame, chunks[1], state),
    }

    // 3. Footer
    FooterWidget::render(frame, chunks[2], state);

    // 4. Global Help Modal Dialog
    if state.show_help_modal {
        HelpModalWidget::render(frame, size, state);
    }

    // 5. Global Command Palette Modal
    if state.show_command_palette {
        CommandPaletteWidget::render(frame, size, state);
    }

    // 6. Safety Confirmation Modal Dialog Overlay
    if state.pending_confirmation.is_some() {
        ConfirmationModalWidget::render(frame, size, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_extreme_size_guard_triggers_under_bounds() {
        // Terminal width < 48
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState::new().with_mock_data();

        terminal.draw(|f| render(f, &state)).unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Terminal too small"));
        assert!(text.contains("min 60x18"));

        // Terminal height < 10
        let backend2 = TestBackend::new(80, 8);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2.draw(|f| render(f, &state)).unwrap();
        let buf2 = terminal2.backend().buffer();
        let text2: String = buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(text2.contains("Terminal too small"));
        assert!(text2.contains("min 60x18"));
    }

    #[test]
    fn test_render_with_command_palette_overlay() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::new().with_mock_data();
        state.show_command_palette = true;

        terminal.draw(|f| render(f, &state)).unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Command Palette (:)"));
    }

    #[test]
    fn test_render_with_confirmation_modal_overlay() {
        use crate::tui::state::{ConfirmationModalState, DestructiveAction};
        use uuid::Uuid;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::new().with_mock_data();
        let task_id = Uuid::new_v4();
        state.pending_confirmation = Some(ConfirmationModalState {
            title: "Confirm Task Cancellation".to_string(),
            message: "Are you sure you want to cancel task PLAN-1?".to_string(),
            action: DestructiveAction::CancelTask {
                task_id,
                short_id: "PLAN-1".to_string(),
            },
        });

        terminal.draw(|f| render(f, &state)).unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("Confirm Task Cancellation"));
        assert!(text.contains("Are you sure you want to cancel task PLAN-1?"));
    }

    #[test]
    fn test_render_at_standard_bounds_does_not_panic() {
        for (w, h) in [(60, 18), (70, 20), (80, 24), (100, 30), (160, 50)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();
            let state = AppState::new().with_mock_data();
            terminal.draw(|f| render(f, &state)).expect("Render should not panic");
        }
    }
}

