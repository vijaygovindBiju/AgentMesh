# ADR 012 — Production Reliability, Stale Task Reclamation, and Event Deduplication

**Date:** 2026-09-19  
**Status:** Accepted  

---

## Context

Real-world distributed execution entails agent process crashes, coordinator restarts, and transient network partitions. Without automated self-healing mechanisms:
- Crashed agents leave tasks indefinitely locked in `Executing` or `Assigned` states.
- Coordinator restarts leave in-flight deliveries unmonitored.
- Network retransmissions can deliver duplicate event notifications, leading to race conditions or duplicate state transitions.

---

## Decision

1. **Coordinator Startup Recovery:**
   - On startup, `CoordinatorRecoveryService` scans PostgreSQL for unconfirmed deliveries and pending tasks. In-flight deliveries whose agents are unreachable are marked `Terminal` and tasks are safely returned to `Approved` or `HumanReview`.

2. **Periodic Stale Task Sweeper:**
   - `StaleTaskSweeper` periodically identifies unresponsive agents (heartbeats older than configurable threshold).
   - Tasks held by dead or offline workers are automatically reclaimed and reset to `Approved`, releasing concurrency slots for healthy agents.

3. **Event Deduplication Cache:**
   - `EventDeduplicator` uses a thread-safe, TTL-bounded cache to discard duplicate agent event messages arriving via NATS before processing database state machine transitions.

4. **Resilient Connections with Exponential Backoff:**
   - Database operations and NATS client connections use `ResilientConnection::with_retry` with exponential backoff and jitter to survive transient hiccups.

---

## Consequences

- The coordinator recovers gracefully from crashes and restarts without manual operator intervention.
- Tasks are never permanently lost or abandoned due to worker failures.
- Idempotency is guaranteed across message transport boundaries.
