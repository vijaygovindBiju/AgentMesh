//! AgentMesh Agent Protocol
//!
//! Shared message types and interface definitions for all AgentMesh agents.
//!
//! # Design principles
//!
//! - This crate contains **no runtime logic** — only data types and trait definitions.
//! - It has **no dependency on NATS, tokio, or any specific transport**.
//!   Any async dependencies belong in the crates that do actual I/O.
//! - Every agent adapter (mock, agy, future runtimes) imports this crate
//!   and speaks the same vocabulary.
//! - The coordinator imports this crate to construct and parse messages.
//!
//! # Message types (defined in Phase 2)
//!
//! - [`messages::AgentMessage`]   — agent → coordinator lifecycle events
//! - [`messages::CoordinatorMessage`] — coordinator → agent commands
//! - [`spec::TaskSpec`]           — task assignment payload

// Phase 2 will populate these modules.
// pub mod messages;
// pub mod spec;
