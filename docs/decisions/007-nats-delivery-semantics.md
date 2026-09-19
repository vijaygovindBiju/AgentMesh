# ADR 007 — NATS Delivery Semantics

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

Task assignment is not a generic job queue. Each task has:
- A specific **owning agent** (determined by coordinator + human approval)
- A **delivery attempt counter** (increments on reassignment, not on redelivery)
- An **idempotency requirement** (agent must not process the same task twice)
- A **failure path** (coordinator must detect non-delivery and act)
- A **reassignment path** (human can approve a reassignment after failure)

Treating the NATS stream as the authoritative source of task state would lose this information on NATS restart or stream expiry. PostgreSQL must own this state.

---

## Decision

**`TaskDelivery` records in PostgreSQL are the authoritative model for task delivery state. NATS JetStream is the transport layer only — it delivers bytes; PostgreSQL records what happened.**

---

## TaskDelivery Record

```
id:               UUID       — primary key
task_id:          UUID       — FK → Task
agent_id:         UUID       — FK → Agent (the assigned agent, not just a queue consumer)
attempt:          i32        — starts at 1; increments only on REASSIGNMENT (not redelivery)
nats_stream:      String     — "TASK_ASSIGNMENTS"
nats_subject:     String     — "coordinator.tasks.assign.{agent_id}"
nats_sequence:    Option<i64>— JetStream message sequence; set after successful publish
idempotency_key:  String     — "{task_id}:{attempt}"; used by agent to detect redelivery
delivered_at:     Timestamp  — when coordinator published to NATS
acknowledged_at:  Option<Timestamp>
ack_kind:         Option<Ack | Nak | Term>
expires_at:       Timestamp  — coordinator considers delivery failed after this
status:           Enum       — see states below
failure_reason:   Option<String>
reassigned_to:    Option<UUID> — FK → Agent; set when status = Reassigned
```

## TaskDelivery States

```
Pending      — coordinator created record but has not yet published to NATS
               (allows crash-safe: if coordinator crashes here, it can replay)

Delivered    — published to NATS JetStream; awaiting agent ACK

Acknowledged — agent sent ACK; task is now in agent's hands
               (transitions: Executing → Completed | Failed | Blocked in Task table)

NakRequeued  — agent sent NAK; JetStream will redeliver automatically
               (same attempt number; idempotency_key unchanged)

Terminal     — JetStream exhausted max_deliver attempts without ACK
               coordinator sets Task.status = Failed
               human may approve reassignment → new TaskDelivery, attempt+1

Reassigned   — this delivery was superseded by a new TaskDelivery for a different agent
               (old TaskDelivery is closed; new one starts at next attempt number)
```

---

## Delivery Flow

```
Coordinator decides to assign Task A to Agent X
    │
    ▼
INSERT TaskDelivery { status=Pending, attempt=1, agent_id=X, idempotency_key="task-A:1" }
    │
    ▼
Publish TaskAssignment to NATS JetStream (coordinator.tasks.assign.X)
    │
    ▼
UPDATE TaskDelivery { status=Delivered, nats_sequence=..., delivered_at=now() }
    │
    ├─── Agent ACKs (ack_kind=Ack)
    │        UPDATE TaskDelivery { status=Acknowledged, ack_at=now() }
    │        (task lifecycle continues via AgentEvents)
    │
    ├─── Agent NAKs (ack_kind=Nak)
    │        UPDATE TaskDelivery { status=NakRequeued }
    │        JetStream redelivers with same idempotency_key → agent deduplicates
    │
    └─── expires_at passes / max_deliver exceeded
             UPDATE TaskDelivery { status=Terminal, failure_reason="..." }
             UPDATE Task { status=Failed }
             TUI shows alert; human may approve reassignment
```

---

## Idempotency Contract

### Coordinator side
- Before creating a `TaskDelivery`, check: is there already an `Acknowledged` or `Delivered` delivery for this `task_id`? If yes, do not create a duplicate.
- When processing an `AgentMessage::TaskStarted` event, check: is the task already in `Executing` state? If yes, log and discard (do not double-count).

### Agent side
- Agent must store processed `idempotency_key` values persistently (or check task state via NATS before processing).
- On receipt of a `TaskAssignment`, agent checks its local state: has `idempotency_key` been processed?
  - Yes → ACK immediately, send no events.
  - No  → begin work, record key, then send `TaskStarted`.

---

## Reassignment

When a `TaskDelivery` reaches `Terminal` state:
1. Coordinator marks `Task.status = Failed`.
2. TUI alerts the human team.
3. If the human approves reassignment to Agent Y:
   - Old `TaskDelivery` remains in `Terminal` state (immutable audit trail).
   - New `TaskDelivery` created: `{ agent_id=Y, attempt=2, idempotency_key="task-A:2", ... }`.
   - Task returns to `Approved` state (awaiting new delivery).
4. New delivery published to NATS under Agent Y's subject.

**The human must approve reassignment.** The coordinator does not silently reassign failed tasks.

### Delivery Failure vs. Execution Failure
In v0.1, both delivery failures (agent never started before timeout/exhaustion) and execution failures (agent started work but encountered an error) transition `Task.status` to `Failed`. The distinction is preserved in the audit layer:
- `TaskDelivery` stores transport/startup failure details (`failure_reason`, `attempt`, `Terminal` status).
- `agent_events` stores runtime execution failure details (`message`, `payload`).
This design avoids premature expansion of the core `TaskStatus` enum while preserving the full diagnostic trail for human review.

---

## Consequences

- Coordinator must handle the crash-recovery case: on startup, scan for `TaskDelivery` records with `status=Pending` and republish.
- The NATS JetStream sequence is recorded for debugging but is not the primary identifier — `TaskDelivery.id` is.
- `TaskDelivery` table is append-only (no deletes); old records are the audit trail.
- Max deliver limit (5) and ack wait (60s) are configuration values, not hardcoded.
