//! Production-Quality Reliability subsystem for AgentMesh Coordinator.
//!
//! Provides coordinator restart crash-recovery, stale task reclamation,
//! agent reconnect reconciliation, message deduplication, and resilient connection management.

pub mod deduplication;
pub mod reconnect;
pub mod recovery;
pub mod stale_sweeper;

pub use deduplication::EventDeduplicator;
pub use reconnect::{with_retry, ResilientConnection};
pub use recovery::{CoordinatorRecoveryService, RecoveryReport};
pub use stale_sweeper::{StaleTaskSweeper, SweepResult};
