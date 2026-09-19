//! TUI module for the AgentMesh coordinator.
//!
//! Provides the ratatui interface for project initiation, per-task human review,
//! inline description editing, and system fleet monitoring.
//!
//! Strict architectural boundary: The TUI only renders state and emits `TuiAction`
//! events. It does not perform side effects (DB queries, NATS publishing, agent assignments, LLM calls).

pub mod screens;
pub mod state;
pub mod ui;
pub mod widgets;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub use state::{AppState, CurrentScreen, InputMode, ReviewTaskState, TuiAction, TuiUpdateEvent};
pub use ui::render;

/// Manages terminal initialization, event loop, and cleanup.
pub struct TerminalApp {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalApp {
    /// Initializes terminal raw mode and alternate screen.
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Runs the interactive event loop with an incoming update channel.
    /// Real-time updates from NATS / background coordinator tasks are received over `update_rx`.
    pub fn run_with_channel<F>(
        &mut self,
        state: &mut AppState,
        mut update_rx: tokio::sync::mpsc::UnboundedReceiver<TuiUpdateEvent>,
        mut action_handler: F,
    ) -> Result<()>
    where
        F: FnMut(TuiAction, &mut AppState),
    {
        let tick_rate = Duration::from_millis(50);

        while !state.should_quit {
            // Drain all pending incoming updates from async channel
            while let Ok(update) = update_rx.try_recv() {
                state.apply_update(update);
            }

            self.terminal.draw(|f| render(f, state))?;

            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    if let Some(action) = state.handle_key(key) {
                        action_handler(action, state);
                    }
                }
            }
        }

        Ok(())
    }

    /// Runs the interactive event loop with the given mutable AppState.
    /// Returns any `TuiAction` emitted during the session.
    pub fn run<F>(&mut self, state: &mut AppState, action_handler: F) -> Result<()>
    where
        F: FnMut(TuiAction, &mut AppState),
    {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.run_with_channel(state, rx, action_handler)
    }
}

impl Drop for TerminalApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_render_all_screens_headless() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        // 1. Render ProjectInput screen
        let mut state = AppState::new();
        terminal.draw(|f| render(f, &state)).unwrap();

        // 2. Render PlanReview screen with mock tasks
        state = state.with_mock_data();
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);
        terminal.draw(|f| render(f, &state)).unwrap();

        // 3. Render PlanReview in inline editing mode
        state.input_mode = InputMode::EditingDescription;
        state.edit_buffer = "Editing description for test".to_string();
        terminal.draw(|f| render(f, &state)).unwrap();

        // 4. Render Dashboard screen
        state.current_screen = CurrentScreen::Dashboard;
        state.input_mode = InputMode::Normal;
        terminal.draw(|f| render(f, &state)).unwrap();
    }
}
