//! NATS messaging and JetStream transport layer for AgentMesh coordinator.

pub mod client;
pub mod heartbeat;
pub mod publisher;
pub mod registration;
pub mod streams;
pub mod subscriber;

pub use client::connect;
pub use heartbeat::HeartbeatMonitor;
pub use publisher::TaskPublisher;
pub use registration::RegistrationHandler;
pub use streams::{
    ensure_streams, AGENT_EVENTS_STREAM, AGENT_EVENTS_SUBJECT, TASK_ASSIGNMENTS_STREAM,
    TASK_ASSIGNMENTS_SUBJECT,
};
pub use subscriber::EventSubscriber;
