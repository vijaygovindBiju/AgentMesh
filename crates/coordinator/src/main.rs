use anyhow::Result;

// Modules uncommented as phases progress:
pub mod domain;    // Phase 1: domain types
pub mod db;        // Phase 1: database pool & repositories
pub mod messaging; // Phase 2: NATS infrastructure & streams
pub mod ai;        // Phase 3: AI planning layer

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("AgentMesh Coordinator — Phase 0 scaffold");
    tracing::warn!("Not yet implemented. See TODO.md for implementation plan.");
    Ok(())
}
