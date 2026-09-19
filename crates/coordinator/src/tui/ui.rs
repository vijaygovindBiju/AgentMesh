use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use crate::tui::screens::{
    DashboardScreen, DiagnosticsScreen, PlanReviewScreen, ProjectInputScreen,
};
use crate::tui::state::{AppState, CurrentScreen};
use crate::tui::widgets::{FooterWidget, HeaderWidget};

/// Top-level layout orchestrator
pub fn render(frame: &mut Frame, state: &AppState) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header bar
            Constraint::Min(10),   // Main body area
            Constraint::Length(3), // Footer key hints and status
        ])
        .split(size);

    // 1. Header
    HeaderWidget::render(frame, chunks[0], state);

    // 2. Main Screen Body
    match state.current_screen {
        CurrentScreen::ProjectInput => ProjectInputScreen::render(frame, chunks[1], state),
        CurrentScreen::PlanReview => PlanReviewScreen::render(frame, chunks[1], state),
        CurrentScreen::Dashboard => DashboardScreen::render(frame, chunks[1], state),
        CurrentScreen::Diagnostics => DiagnosticsScreen::render(frame, chunks[1], state),
    }

    // 3. Footer
    FooterWidget::render(frame, chunks[2], state);
}
