//! Event Deduplication to ensure idempotent message processing.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// In-memory deduplicator guarding coordinator state machine against duplicate event processing.
pub struct EventDeduplicator {
    seen_events: RwLock<HashMap<String, Instant>>,
    ttl: Duration,
}

impl Default for EventDeduplicator {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // 5 minute default window
    }
}

impl EventDeduplicator {
    pub fn new(ttl: Duration) -> Self {
        Self {
            seen_events: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Computes a standard idempotency key for an incoming agent event.
    pub fn compute_event_key(
        agent_id: Uuid,
        task_id: Uuid,
        event_type: &str,
        payload_hash: Option<&str>,
    ) -> String {
        if let Some(hash) = payload_hash {
            format!("{agent_id}:{task_id}:{event_type}:{hash}")
        } else {
            format!("{agent_id}:{task_id}:{event_type}")
        }
    }

    /// Checks if an event has already been seen within the TTL window.
    /// Returns `true` if the event is NEW and should be processed.
    /// Returns `false` if the event is a DUPLICATE and should be ignored.
    pub fn check_or_record(&self, key: &str) -> bool {
        let now = Instant::now();

        // Fast path read lock check
        {
            let map = self.seen_events.read().unwrap();
            if let Some(timestamp) = map.get(key) {
                if now.duration_since(*timestamp) < self.ttl {
                    return false; // Duplicate
                }
            }
        }

        // Write lock path
        let mut map = self.seen_events.write().unwrap();
        // Double check under write lock
        if let Some(timestamp) = map.get(key) {
            if now.duration_since(*timestamp) < self.ttl {
                return false;
            }
        }

        // Cleanup expired entries if cache is growing large
        if map.len() > 10_000 {
            map.retain(|_, &mut ts| now.duration_since(ts) < self.ttl);
        }

        map.insert(key.to_string(), now);
        true
    }

    /// Clears all recorded deduplication keys (useful for testing).
    pub fn clear(&self) {
        let mut map = self.seen_events.write().unwrap();
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplicator_filters_duplicates_within_ttl() {
        let dedup = EventDeduplicator::new(Duration::from_secs(10));
        let agent_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        let key = EventDeduplicator::compute_event_key(agent_id, task_id, "TaskStarted", None);

        // First attempt -> should process
        assert!(dedup.check_or_record(&key));

        // Immediate second attempt with same key -> should be rejected as duplicate
        assert!(!dedup.check_or_record(&key));

        // Different event type for same agent/task -> should process
        let key2 =
            EventDeduplicator::compute_event_key(agent_id, task_id, "ProgressUpdate", Some("50%"));
        assert!(dedup.check_or_record(&key2));
    }

    #[test]
    fn test_deduplicator_expires_after_ttl() {
        let dedup = EventDeduplicator::new(Duration::from_millis(50));
        let key = "agent-1:task-1:progress";

        assert!(dedup.check_or_record(key));
        assert!(!dedup.check_or_record(key));

        std::thread::sleep(Duration::from_millis(60));

        // After TTL expires, event key can be seen again
        assert!(dedup.check_or_record(key));
    }
}
