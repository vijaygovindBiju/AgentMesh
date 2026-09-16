# ADR 008 — Human Approval Boundary

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

A central risk in an AI coordination system is that the AI's proposals bypass human judgment and drive execution directly. This must be structurally impossible, not just a convention.

The system must guarantee:
- The LLM can only produce plans, never execute them.
- No task ever transitions to `Assigned` (i.e., delivered to an agent) without a recorded human decision.
- Humans can override any part of the AI's proposal.
- The audit trail preserves who approved what and when.

---

## Decision

**Human approval is a hard gate encoded in the domain model state machine. It cannot be bypassed by coordinator logic or LLM output.**

---

## State Machine

```
LLM output
    │  (only entry point for AI-generated content)
    ▼
PROPOSED ──────────────────────────────────────────────────────────────────────┐
    │                                                                          │
    │  coordinator queues for TUI review                                       │
    ▼                                                                          │
HUMAN_REVIEW ◄── only state where human action is required                    │
    │                                                                          │
    ├── [Y] Approve ──────────────────────────────► APPROVED                  │
    │                                                    │                    │
    ├── [E] Edit + confirm ──────────────────────────────┤                    │
    │       (human changes description; stored in         │                    │
    │        TaskApproval.edited_desc)                    │                    │
    │                                                     │                    │
    └── [N] Reject ──────────────────────────────────► REJECTED (terminal)    │
                                                         │                    │
                                                         │  reassignment      │
                                                         └──────────────────► ┘
                                                           creates new Proposal

APPROVED
    │  coordinator checks: are all dependencies Completed?
    │  if no: task waits in Approved state
    │  if yes: coordinator creates TaskDelivery and publishes to NATS
    ▼
ASSIGNED
    │  agent ACKs the TaskAssignment
    ▼
EXECUTING
    │
    ├──► BLOCKED (agent sent Blocked event)
    │         │  blocker task reaches Completed → back to EXECUTING
    │
    ├──► COMPLETED (terminal)
    │
    └──► FAILED (terminal — may trigger reassignment Proposal, human re-approves)
```

---

## Invariants (must be enforced by coordinator code, not just convention)

| Invariant | Enforcement |
|---|---|
| LLM output always enters at `Proposed` | `propose_plan()` result is always converted to `Proposed` tasks; no other code path creates tasks |
| `Approved` requires a `TaskApproval` record | Coordinator's assignment logic queries `TaskApproval` before creating `TaskDelivery`; no `TaskApproval` → no assignment |
| `TaskApproval` is only written by TUI input handlers | The approval write path is in `tui/screens/plan_review.rs` only |
| `Assigned` requires `Approved` | `Approved → Assigned` transition checks for `TaskApproval` row with `status IN (Approved, EditedAndApproved)` |
| `NATS publish` only follows `TaskDelivery` creation | Assignment logic: (1) create TaskDelivery in DB, (2) publish to NATS — never publish without DB record |

---

## Human Override Capabilities

Humans can override the AI proposal at every stage:

| Stage | Override |
|---|---|
| Task description | Edit inline (`[E]`) before approving |
| Task assignment | Reject AI's suggested agent; coordinator re-proposes or human picks |
| Task existence | Reject a task entirely (`[N]`) |
| Task reassignment | After failure, human must explicitly approve re-assignment to a new agent |
| Plan abandonment | `[ESC]` during plan review aborts the entire proposal (all tasks remain `Proposed`) |

---

## Audit Trail

Every approval is recorded:

```sql
TaskApproval {
  task_id:      UUID,
  proposal_id:  UUID,
  status:       Approved | Rejected | EditedAndApproved,
  edited_desc:  Option<Text>,   -- human's exact changes
  approved_by:  String,         -- human display name
  approved_at:  Timestamp,
}
```

This means at any time, the coordinator can answer:
- Which tasks were approved by whom?
- Which tasks were edited before approval (and what was changed)?
- Which tasks were rejected (and therefore not executed)?

---

## Consequences

- The coordinator cannot be used as a "just run everything the AI says" tool — human input is required before any agent starts work.
- The approval screen UX must be efficient enough that reviewing 10–20 tasks is not painful.
- For automated testing, `MockLlmProvider` + a test-mode approval path (auto-approve all) can be used in integration tests.
- The "auto-approve" test path must be explicitly gated (e.g., `AGENTMESH_AUTO_APPROVE=true` env var) and must not be reachable in production builds.
