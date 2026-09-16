# AgentMesh — Domain Model

## Task State Machine

This is the central invariant of the system. The LLM can only ever produce `Proposed` tasks. Human approval is the exclusive gate between planning and execution.

```
                     ┌─────────────┐
                     │  PROPOSED   │  ← LLM output (only entry point for AI)
                     └──────┬──────┘
                            │  coordinator queues for review
                            ▼
                     ┌─────────────┐
                     │ HUMAN_REVIEW│  ← TUI presents task card
                     └──────┬──────┘
                    ┌───────┴────────┐
             [Y/E]  │                │  [N]
                    ▼                ▼
               ┌─────────┐     ┌──────────┐
               │ APPROVED│     │ REJECTED │  (terminal)
               └────┬────┘     └──────────┘
                    │  coordinator checks dependencies
                    │  (only assigns when all blockers = COMPLETED)
                    ▼
               ┌──────────┐
               │ ASSIGNED │  ← TaskDelivery record created in PostgreSQL
               └────┬─────┘    NATS message published
                    │  agent sends TaskStarted event
                    ▼
               ┌───────────┐
               │ EXECUTING │
               └─────┬─────┘
          ┌──────────┼──────────┐
          │          │          │
          ▼          ▼          ▼
    ┌─────────┐ ┌─────────┐ ┌────────┐
    │ BLOCKED │ │COMPLETED│ │ FAILED │  (terminal — coordinator may reassign)
    └────┬────┘ └─────────┘ └────────┘
         │  blocker resolves
         └──────► EXECUTING
```

**Rules:**
- The LLM never writes to any state beyond `Proposed`.
- Only a `TaskApproval` record with status `Approved` or `EditedAndApproved` advances a task from `HumanReview`.
- The coordinator's assignment logic (deterministic code) advances `Approved → Assigned`.
- The coordinator never skips `HumanReview`, even on retry or reassignment of a previously-approved task. New assignments go through a new `Proposal`.

### Note on Failure Modes: Delivery Failure vs. Execution Failure
In v0.1, both failure modes map to `TaskStatus::Failed` to keep the initial state model clean while preserving full context:
1. **Delivery Failure (Transport / Startup Window):** The agent never reaches `Executing` (e.g. NATS redelivery exhausted, or agent crashed after JetStream ACK before reporting `TaskStarted`, causing `expires_at` timeout). The failure context is captured in `TaskDelivery.failure_reason` and `TaskDelivery.status = 'terminal'`.
2. **Execution Failure:** The agent successfully entered `Executing`, but subsequently reported `AgentMessage::Failed` due to build errors, runtime exceptions, or missing dependencies. The context is captured in `agent_events`.

In a future version, these may be split into explicit states (`DeliveryFailed` vs `ExecutionFailed`) if distinct automated recovery policies are required.

---

## Domain Types

### Project
```
id:          UUID (PK)
name:        String
description: Text
status:      Draft | Planning | Active | Paused | Completed
created_at:  Timestamp
updated_at:  Timestamp
```

### Task
```
id:                  UUID (PK)
project_id:          UUID (FK → Project)
short_id:            String          e.g. "TASK-003"
title:               String
description:         Text
status:              [see state machine above]
assigned_agent_id:   Option<UUID>   (FK → Agent)
affected_resources:  JSONB           Vec<String> — file paths, modules, APIs
estimated_size:      Option<String>  S | M | L
proposal_id:         UUID            (FK → Proposal)
created_at:          Timestamp
updated_at:          Timestamp
```

### TaskDependency
```
dependent_id:   UUID (FK → Task)  — "this task..."
depends_on_id:  UUID (FK → Task)  — "...needs this to be Completed first"
kind:           Blocks | RelatesTo
```

### Agent
```
id:             UUID (PK)
human_owner:    String          display name (e.g. "Alice")
api_key_hash:   String          bcrypt hash; raw key shown once at registration
adapter_type:   Mock | Agy
capabilities:   JSONB           Vec<String>
nats_subject:   String          e.g. "agents.abc123.events"
status:         Offline | Idle | Busy | Blocked | Error
current_task:   Option<UUID>    (FK → Task)
last_seen:      Option<Timestamp>
created_at:     Timestamp
```

### Proposal
```
id:            UUID (PK)
project_id:    UUID (FK → Project)
ai_provider:   String
ai_model:      String
raw_prompt:    Text    (stored for auditability)
raw_response:  Text    (stored for auditability)
status:        Pending | PartiallyApproved | FullyApproved | Rejected
created_at:    Timestamp
```

### TaskApproval
```
task_id:      UUID (FK → Task)       — composite PK with proposal_id
proposal_id:  UUID (FK → Proposal)
status:       Approved | Rejected | EditedAndApproved
edited_desc:  Option<Text>           — human's edited description if changed
approved_by:  String                 — human display name
approved_at:  Timestamp
```

### TaskDelivery
```
id:               UUID (PK)
task_id:          UUID (FK → Task)
agent_id:         UUID (FK → Agent)
attempt:          i32              starts at 1; increments on reassignment
nats_stream:      String           "TASK_ASSIGNMENTS"
nats_sequence:    Option<i64>      JetStream sequence number after publish
nats_subject:     String           "coordinator.tasks.assign.{agent_id}"
idempotency_key:  String           "{task_id}:{attempt}" — used for dedup on redelivery
delivered_at:     Timestamp        when coordinator published to NATS
acknowledged_at:  Option<Timestamp>
ack_kind:         Option<Ack | Nak | Term>
expires_at:       Timestamp        max wait before coordinator marks as failed
status:           Pending | Delivered | Acknowledged | NakRequeued | Terminal | Reassigned
failure_reason:   Option<String>
reassigned_to:    Option<UUID>     (FK → Agent) — set when status = Reassigned
```

### AgentEvent
```
id:           UUID (PK)
agent_id:     UUID (FK → Agent)
task_id:      UUID (FK → Task)
event_type:   TaskStarted | ProgressUpdate | Blocked | Completed | Failed
message:      Option<Text>
payload:      JSONB
received_at:  Timestamp
```

### OverlapWarning
```
id:            UUID (PK)
project_id:    UUID (FK → Project)
task_ids:      JSONB   Vec<UUID> — tasks that share the resource
resource:      String  file path, module name, or API endpoint
severity:      Info | Warning | Critical
acknowledged:  bool
created_at:    Timestamp
```

---

## TaskDelivery Lifecycle

```
Coordinator publishes to NATS
    │
    ▼ [status = Pending, attempt = N]
NATS JetStream delivers to agent
    │
    ├── Agent ACKs → [status = Acknowledged, acked_kind = Ack]
    │       │
    │       └── Agent sends TaskStarted → task.status = Executing
    │
    ├── Agent NAKs → JetStream redelivers
    │       │        [status = NakRequeued]
    │       └── New delivery attempt → attempt stays same
    │
    └── Max deliver exceeded → [status = Terminal]
            │
            └── Coordinator sets task.status = Failed
                    │
                    └── Coordinator may create new Proposal for reassignment
                            (new attempt, new agent, human must re-approve)
```

**Idempotency:** If an agent receives a redelivery of a message it has already processed (same `idempotency_key`), it must ACK immediately without reprocessing. The coordinator detects this case by checking the `idempotency_key` in the `TaskDelivery` table before updating task state.
