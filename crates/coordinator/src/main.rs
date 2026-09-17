use anyhow::Result;

pub mod domain;      // Phase 1: domain types
pub mod db;          // Phase 1: database pool & repositories
pub mod messaging;   // Phase 2: NATS infrastructure & streams
pub mod ai;          // Phase 3: AI planning layer
pub mod tui;         // Phase 4: TUI skeleton
pub mod coordinator; // Phase 5: Coordinator Core


#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("AgentMesh Coordinator — Phase 4 TUI");

    // If running in a TTY, launch interactive TUI skeleton
    if crossterm::tty::IsTty::is_tty(&std::io::stdout()) {
        let mut state = tui::AppState::new().with_mock_data();
        let mut app = tui::TerminalApp::new()?;
        app.run(&mut state, |action, _state| {
            tracing::info!(?action, "TUI action emitted (decoupled from side effects)");
        })?;
    } else {
        tracing::info!("Non-interactive terminal detected. Run tests with `cargo test`.");
    }

    Ok(())
}

