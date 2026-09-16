use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Operational status of an agent reported during heartbeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Offline,
    Idle,
    Busy,
    Blocked,
    Error,
}

/// Acknowledgment response kind for a task delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckKind {
    Ack,
    Nak,
    Term,
}

/// Delivery acknowledgment record exchanged across protocol boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryAck {
    pub idempotency_key: String,
    pub kind: AckKind,
    pub timestamp: DateTime<Utc>,
}
