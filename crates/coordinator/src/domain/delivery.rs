use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── DeliveryStatus ───────────────────────────────────────────────────────────

/// State of a single task delivery attempt.
///
/// PostgreSQL enum type: `delivery_status`
///
/// Full lifecycle and transitions are defined in `docs/decisions/007-nats-delivery-semantics.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "delivery_status", rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// `TaskDelivery` record created in PostgreSQL; NATS publish not yet confirmed.
    /// If the coordinator crashes here it can replay on restart by scanning `Pending` records.
    Pending,
    /// Published to NATS JetStream; awaiting agent ACK.
    Delivered,
    /// Agent sent ACK. Task is now in agent control.
    /// Further state changes come via `AgentEvent` records.
    Acknowledged,
    /// Agent sent NAK. JetStream will redeliver automatically.
    /// Attempt number does NOT increment on NAK — only on reassignment.
    NakRequeued,
    /// JetStream exhausted `max_deliver` attempts without ACK.
    /// Coordinator sets `Task.status = Failed`. Human may approve reassignment.
    Terminal,
    /// This delivery was superseded by a new `TaskDelivery` for a different agent.
    /// The old record is closed; it forms part of the immutable audit trail.
    Reassigned,
}

// ─── AckKind ──────────────────────────────────────────────────────────────────

/// How the agent responded to a task delivery.
///
/// PostgreSQL enum type: `ack_kind`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "ack_kind", rename_all = "snake_case")]
pub enum AckKind {
    /// Agent accepted the task and will begin processing.
    Ack,
    /// Agent is not ready; JetStream should redeliver.
    Nak,
    /// Agent cannot process this message; JetStream should not redeliver.
    Term,
}

// ─── TaskDelivery ─────────────────────────────────────────────────────────────

/// An explicit record of a single task delivery attempt to a specific agent.
///
/// This is the authoritative source of delivery state — NATS is the transport only.
/// Immutable append-only: old records are never deleted; they form the audit trail.
///
/// See `docs/decisions/007-nats-delivery-semantics.md` for the full lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TaskDelivery {
    pub id: Uuid,
    pub task_id: Uuid,
    /// The specific agent this delivery targets. Not just "whoever picks it up".
    pub agent_id: Uuid,
    /// Starts at 1. Increments only on REASSIGNMENT (not on JetStream redelivery).
    /// JetStream redelivery preserves the same attempt number and idempotency key.
    pub attempt: i32,
    /// JetStream stream name, e.g. "TASK_ASSIGNMENTS".
    pub nats_stream: String,
    /// Full NATS subject, e.g. "coordinator.tasks.assign.{agent_id}".
    pub nats_subject: String,
    /// JetStream sequence number of the published message. Set after publish confirms.
    pub nats_sequence: Option<i64>,
    /// Deduplication key: "{task_id}:{attempt}".
    /// Agents must ACK without reprocessing if this key has already been handled.
    pub idempotency_key: String,
    /// When coordinator published to NATS.
    pub delivered_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub ack_kind: Option<AckKind>,
    /// After this timestamp, coordinator considers the delivery failed if not ACKed.
    pub expires_at: DateTime<Utc>,
    pub status: DeliveryStatus,
    pub failure_reason: Option<String>,
    /// Set when `status = Reassigned`. Points to the replacement agent.
    pub reassigned_to: Option<Uuid>,
}

impl TaskDelivery {
    /// Constructs the idempotency key for a given task + attempt.
    /// Agents use this to detect redelivery of already-processed messages.
    pub fn make_idempotency_key(task_id: Uuid, attempt: i32) -> String {
        format!("{task_id}:{attempt}")
    }
}

// ─── NewTaskDelivery ──────────────────────────────────────────────────────────

/// Data required to create a `TaskDelivery` record before publishing to NATS.
/// Id and `delivered_at` are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTaskDelivery {
    pub task_id: Uuid,
    pub agent_id: Uuid,
    pub attempt: i32,
    pub nats_stream: String,
    pub nats_subject: String,
    pub idempotency_key: String,
    pub expires_at: DateTime<Utc>,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_status_serde_round_trip() {
        for status in [
            DeliveryStatus::Pending,
            DeliveryStatus::Delivered,
            DeliveryStatus::Acknowledged,
            DeliveryStatus::NakRequeued,
            DeliveryStatus::Terminal,
            DeliveryStatus::Reassigned,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: DeliveryStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }

    #[test]
    fn ack_kind_serde_round_trip() {
        for kind in [AckKind::Ack, AckKind::Nak, AckKind::Term] {
            let json = serde_json::to_string(&kind).unwrap();
            let recovered: AckKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, recovered);
        }
    }

    #[test]
    fn idempotency_key_format() {
        let task_id = Uuid::nil();
        let key = TaskDelivery::make_idempotency_key(task_id, 1);
        assert!(key.contains(':'));
        assert!(key.ends_with(":1"));

        // Attempt 2 produces a different key (reassignment)
        let key2 = TaskDelivery::make_idempotency_key(task_id, 2);
        assert_ne!(key, key2);
    }
}
