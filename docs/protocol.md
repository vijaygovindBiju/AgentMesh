# AgentMesh — Agent Protocol

## Overview

The agent protocol defines the vocabulary between the coordinator and all agent adapters. It is intentionally transport-independent: the message types are defined in the `agent-protocol` crate and have no dependency on NATS, HTTP, or any specific runtime.

The current transport is **NATS with JetStream**. A different transport could be used by a future adapter without changing the message types.

---

## NATS Subject Schema

```
# Coordinator → Agent (via JetStream stream: TASK_ASSIGNMENTS)
coordinator.tasks.assign.{agent_id}

# Agent → Coordinator (via JetStream stream: AGENT_EVENTS)
agents.{agent_id}.events

# Agent → Coordinator (request-reply, not JetStream)
coordinator.agents.register

# Agent → Coordinator (core NATS, no JetStream, fire-and-forget)
coordinator.agents.heartbeat.{agent_id}
```

---

## JetStream Streams

### `TASK_ASSIGNMENTS`
| Property | Value |
|---|---|
| Subjects | `coordinator.tasks.assign.*` |
| Retention | WorkQueue |
| Storage | File (persisted in Docker volume) |
| Max Deliver | 5 (coordinator marks as Terminal after this) |
| Ack Wait | 60s |
| Consumer | Per-agent durable consumer named `agent-{agent_id}` |

**Rationale for WorkQueue:** Each task assignment is intended for exactly one agent. WorkQueue semantics ensure a message is delivered to exactly one consumer and removed after ACK.

### `AGENT_EVENTS`
| Property | Value |
|---|---|
| Subjects | `agents.*.events` |
| Retention | Limits (24h or 100k messages) |
| Storage | File |
| Consumer | Single durable consumer in coordinator: `coordinator-events` |

---

## Message Types

All messages are JSON-encoded.

### Agent → Coordinator (`AgentMessage`)

```json
// Registration
{
  "type": "Register",
  "agent_id": "uuid",
  "human_owner": "Alice",
  "adapter_type": "Mock",
  "capabilities": ["rust", "python"],
  "api_key": "raw-key-shown-once"
}

// Task lifecycle
{
  "type": "TaskStarted",
  "agent_id": "uuid",
  "task_id": "uuid",
  "idempotency_key": "task-uuid:1",
  "timestamp": "2024-01-01T00:00:00Z"
}

{
  "type": "ProgressUpdate",
  "agent_id": "uuid",
  "task_id": "uuid",
  "message": "Writing database schema",
  "percent": 40,
  "timestamp": "2024-01-01T00:01:00Z"
}

{
  "type": "Blocked",
  "agent_id": "uuid",
  "task_id": "uuid",
  "reason": "Waiting for TASK-001 to complete — need the schema first",
  "blocking_task_id": "uuid-of-blocker",
  "timestamp": "2024-01-01T00:02:00Z"
}

{
  "type": "Completed",
  "agent_id": "uuid",
  "task_id": "uuid",
  "summary": "Database schema created and migration applied",
  "timestamp": "2024-01-01T00:05:00Z"
}

{
  "type": "Failed",
  "agent_id": "uuid",
  "task_id": "uuid",
  "error": "Migration failed: column already exists",
  "timestamp": "2024-01-01T00:05:30Z"
}

// Heartbeat (core NATS, not JetStream)
{
  "type": "Heartbeat",
  "agent_id": "uuid",
  "status": "Busy",
  "current_task_id": "uuid",
  "timestamp": "2024-01-01T00:00:30Z"
}
```

### Coordinator → Agent (`CoordinatorMessage`)

```json
// Task assignment (published to TASK_ASSIGNMENTS stream)
{
  "type": "TaskAssignment",
  "task_id": "uuid",
  "short_id": "TASK-003",
  "title": "Set up authentication middleware",
  "description": "Implement JWT validation...",
  "affected_resources": ["src/auth/", "src/middleware/auth.rs"],
  "depends_on": ["uuid-of-task-001"],
  "idempotency_key": "task-uuid:1",
  "assigned_at": "2024-01-01T00:00:00Z"
}

// Task cancelled
{
  "type": "TaskCancelled",
  "task_id": "uuid",
  "reason": "Human rejected this task during re-review",
  "timestamp": "2024-01-01T00:00:00Z"
}

// Wait instruction (sent when coordinator detects unresolved dependency)
{
  "type": "WaitForDependency",
  "task_id": "uuid",
  "blocking_task_id": "uuid",
  "message": "TASK-001 must complete before you can start",
  "timestamp": "2024-01-01T00:00:00Z"
}
```

---

## Agent Registration Flow

```
Agent starts
    │
    ▼
Agent publishes to coordinator.agents.register (request-reply)
    │   payload: { agent_id, human_owner, adapter_type, capabilities, api_key }
    ▼
Coordinator validates api_key against stored hash
    │
    ├── Valid   → replies { status: "ok", nats_subject: "agents.{id}.events" }
    │             Creates/updates Agent record in PostgreSQL
    │             Sets up durable JetStream consumer for this agent
    │
    └── Invalid → replies { status: "error", message: "Invalid API key" }
                  Agent must not proceed
```

---

## Idempotency

Agents must check the `idempotency_key` in every `TaskAssignment` before processing:

1. Agent receives `TaskAssignment` with `idempotency_key = "task-uuid:1"`
2. Agent checks its local state (or database): has this key been processed?
3. If yes: ACK the message immediately, do not reprocess
4. If no: begin processing, record the key, then report `TaskStarted`

The coordinator also validates idempotency on the `AgentEvent` side: if a `TaskStarted` event arrives for a task already in `Executing` state, it is logged and discarded (not treated as an error).
