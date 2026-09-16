//! Database access layer for AgentMesh coordinator.
//!
//! Provides connection pooling, schema migrations, and repositories
//! for all domain entities. PostgreSQL is the authoritative source of truth.

pub mod pool;
pub mod repositories;

pub use pool::{create_pool, create_pool_with_options, run_migrations};
