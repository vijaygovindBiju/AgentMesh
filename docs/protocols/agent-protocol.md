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
| Max Deliver | 5 (set on the agent consumers) |
| Ack Wait | `agent-mock`: 60 s; `agent-agy`: 600 s (real coding tasks are long-running) |
| Consumer | Per-agent durable pull consumer: `agent-{agent_id_simple}` (mock) / `agent-agy-{agent_id_simple}` (agy), filtered to `coordinator.tasks.assign.{agent_id}` |

Independently of JetStream redelivery, the coordinator records a `TaskDelivery` row per attempt with `expires_at = now + 60 s`; a delivery that is not acknowledged by `TaskStarted` before then is treated as expired by the stale sweeper.

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
// Registration (adapter_type is matched case-insensitively: "mock" | "agy")
{
  "type": "Register",
  "agent_id": "uuid",
  "human_owner": "Alice",
  "adapter_type": "Mock",
  "capabilities": ["rust", "python"],
  "profile": {                       // optional AgentCapabilities (v1.0)
    "runtime":   { "os": "linux", "arch": "x86_64", "adapter_type": "Agy", "cpu_count": 8 },
    "languages": [ { "name": "rust", "version": "1.97.1", "frameworks": [] } ],
    "tools":     [ { "name": "git", "version": "2.55.0", "path": "/usr/bin/git" } ],
    "tags":      ["rust", "backend"]
  },
  "api_key": "am_ak_<64 hex chars>"
}

// Capability refresh (v1.0)
{
  "type": "UpdateCapabilities",
  "agent_id": "uuid",
  "profile": { "...": "AgentCapabilities as above" },
  "timestamp": "2024-01-01T00:00:00Z"
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

// Heartbeat (core NATS, not JetStream) — sent every 5 s by both adapters
{
  "type": "Heartbeat",
  "agent_id": "uuid",
  "status": "Busy",
  "current_task_id": "uuid",
  "health": {                        // optional AgentHealth (v1.0)
    "status": "Healthy",
    "last_heartbeat": "2024-01-01T00:00:30Z",
    "heartbeat_latency_ms": 12,
    "consecutive_failures": 0,
    "tasks_completed": 3,
    "tasks_failed": 0,
    "last_error": null,
    "disk_free_mb": 120000
  },
  "timestamp": "2024-01-01T00:00:30Z"
}
```

Note: the shipped `agent-mock` and `agent-agy` binaries always heartbeat with `status: "Idle"` and no `health` block; the coordinator derives `Busy`/`Idle` from `TaskStarted`/`Completed` and latency from the heartbeat timestamp. Agents with no heartbeat for 30 s are marked `offline`.

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
  "assigned_at": "2024-01-01T00:00:00Z",
  "task_branch": "agentmesh/task-003",   // optional Git context (v1.0); null when no
  "base_branch": "main",                 // worktree has been prepared for the task
  "repo_path": "/repo/.agentmesh/worktrees/TASK-003"
}

// Registration reply (request-reply on coordinator.agents.register)
{ "type": "RegisterResponse", "status": "ok", "nats_subject": "agents.<agent_id>.events" }
{ "type": "RegisterResponse", "status": "error", "error": "Invalid API key" }

// Task cancelled — type defined; NOT currently published by the coordinator.
// Adapters log and ignore it.
{
  "type": "TaskCancelled",
  "task_id": "uuid",
  "reason": "Human rejected this task during re-review",
  "timestamp": "2024-01-01T00:00:00Z"
}

// Wait instruction — type defined; NOT currently published by the coordinator.
// Dependency gating happens before assignment (blocked tasks are never delivered).
{
  "type": "WaitForDependency",
  "task_id": "uuid",
  "blocking_task_id": "uuid",
  "message": "TASK-001 must complete before you can start",
  "timestamp": "2024-01-01T00:00:00Z"
}
```

`agent-agy` turns the `TaskSpec` into a prompt containing the identifier, title, description, affected resources, branch names and repository path, then runs `agy -p <prompt> --output-format stream-json` in `repo_path` (when set).

---

## Agent Registration Flow

```
Agent starts
    │
    ▼
Agent publishes to coordinator.agents.register (request-reply)
    │   payload: { agent_id, human_owner, adapter_type, capabilities, profile?, api_key }
    ▼
Coordinator (RegistrationHandler::process_registration)
    │
    ├── adapter_type not "mock"/"agy"  → { status: "error", error: "Unsupported adapter type" }
    │
    ├── agent_id unknown (first registration)
    │     → creates the agents row: role "worker", max_concurrency 1, default PermissionBoundary
    │       api_key stored as SHA-256 hash if it starts with "am_ak_", otherwise stored as given
    │     → audit_logs: agent_register / success
    │     → { status: "ok", nats_subject: "agents.{id}.events" }
    │
    └── agent_id known (re-registration)
          ├── is_revoked            → { status: "error", error: "Agent key is revoked" }   (audited)
          ├── key expired           → { status: "error", error: "Agent key has expired" }  (audited)
          ├── key mismatch          → { status: "error", error: "Invalid API key" }        (audited)
          └── key matches           → profile updated if supplied; audited;
                                      { status: "ok", nats_subject: "agents.{id}.events" }

Agent then creates its own durable JetStream consumer on TASK_ASSIGNMENTS
and starts the 5 s heartbeat loop.
```

There is no allow-list or human approval step for *new* agent IDs: any client holding the NATS token can register a new agent. Identity continuity is enforced only on re-registration. See the README "Security" section.

---

## Idempotency

Every `TaskAssignment` carries `idempotency_key = "{task_id}:{attempt}"`, and every `TaskStarted` echoes it so the coordinator can match the event to the exact `TaskDelivery` row (`TaskDeliveryRepository::find_by_idempotency_key` → `Acknowledged`).

Recommended agent behaviour (contract):

1. Agent receives `TaskAssignment` with `idempotency_key = "task-uuid:1"`
2. Agent checks its local state: has this key been processed?
3. If yes: ACK the message immediately, do not reprocess
4. If no: begin processing, record the key, then report `TaskStarted`

**Current implementation status:** the shipped `agent-mock` and `agent-agy` adapters transport-ACK on receipt and echo the key in `TaskStarted`, but do not keep a local set of processed keys. Duplicate protection is therefore provided on the coordinator side: `EventDeduplicator` (in-memory TTL keyed by agent/task/event type/percent) drops repeated lifecycle events, and `TaskAuthorizer` rejects events from any agent other than the task's assignee. Redelivery of an already-completed assignment would cause a mock/agy agent to re-run the task; the resulting duplicate `TaskStarted`/`Completed` events are dropped, but the work itself is repeated.
