use anyhow::Result;

// Phase 7: mod adapter;
//
// The agy adapter will:
//   1. Connect to NATS as a registered agent
//   2. Receive TaskAssignment messages from the coordinator
//   3. Spawn `agy` as a subprocess with the task description
//   4. Parse agy stdout/events and translate to AgentMessage lifecycle events
//   5. Publish those events back to the coordinator via NATS
//
// The agy CLI integration mechanism will be documented in docs/decisions/
// once the exact agy API surface is confirmed.

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("AgentMesh agy Adapter — Phase 0 scaffold");
    tracing::warn!("Not yet implemented. agy integration mechanism TBD — see TODO.md.");
    Ok(())
}
