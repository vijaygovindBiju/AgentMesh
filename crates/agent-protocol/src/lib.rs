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

pub mod adapter;
pub mod messages;
pub mod spec;
pub mod status;

pub use adapter::AgentAdapter;
pub use messages::{AgentMessage, CoordinatorMessage};
pub use spec::TaskSpec;
pub use status::{AckKind, AgentStatus, DeliveryAck};
