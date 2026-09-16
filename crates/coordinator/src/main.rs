use anyhow::Result;

// Modules uncommented as phases progress:
pub mod domain; // Phase 1: domain types
                // Phase 1: mod db;
                // Phase 3: mod ai;
                // Phase 4: mod tui;
                // Phase 5: mod coordinator; mod messaging;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("AgentMesh Coordinator — Phase 0 scaffold");
    tracing::warn!("Not yet implemented. See TODO.md for implementation plan.");
    Ok(())
}
