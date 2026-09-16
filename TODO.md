# AgentMesh — Project TODO

> **This file is the authoritative source of development state.**
> Update it before starting, during, and after every task.
> Never mark a task DONE unless its acceptance criteria are fully met.

## Status Legend

```
[ ]  TODO
[~]  IN PROGRESS
[x]  DONE
[!]  BLOCKED  (explanation must follow)
[-]  CANCELLED (reason must follow)
```

## Git Commit Convention

**Commit at every meaningful checkpoint** — do not batch unrelated changes into one commit.

### When to commit
- End of every phase (Phase 0 done, Phase 1 done, etc.)
- After each stable feature or module is complete and compiles cleanly
- After every passing migration or schema change
- After every set of passing tests
- Before and after any large refactor
- Whenever work reaches a state worth preserving independently

### Commit message format
```
<type>(<scope>): <short summary>

<body — explain what and why, not how. Wrap at 72 chars.>

<footer — breaking changes, issue refs, etc. if applicable>
```

### Types
```
feat      New feature or capability
fix       Bug fix
refactor  Code restructure without behaviour change
test      Tests added or updated
docs      Documentation only
chore     Build, config, tooling, CI changes
db        Database schema or migration changes
```

### Examples
```
feat(agent-protocol): add TaskSpec and AgentMessage types

Defines the shared vocabulary between coordinator and all agent
adapters. No runtime dependencies — pure serde types only.

feat(coordinator): implement task state machine

Encodes the PROPOSED→HUMAN_REVIEW→APPROVED→ASSIGNED→EXECUTING
lifecycle as a Rust enum. Transitions are validated at compile time.

db: add initial PostgreSQL schema migration (001)

Creates tables for projects, tasks, task_dependencies, agents,
proposals, task_approvals, task_deliveries, agent_events, and
overlap_warnings.

chore(phase-0): scaffold workspace, docker-compose, ADRs

All Phase 0 acceptance criteria met. cargo check passes cleanly.
```

---

## Phase 0 — Foundation

### Workspace & Build
- [x] Create Cargo workspace (`Cargo.toml`)
- [x] Create crate stubs: `coordinator`, `agent-protocol`, `agent-mock`, `agent-agy`
- [x] Verify workspace compiles (`cargo check` — clean, 0 errors, 0 warnings)

### Infrastructure Configuration
- [x] Create `docker-compose.yml` (NATS 2.10 + PostgreSQL 16, health checks)
- [x] Create `.env.example` with all required variables documented
- [x] Create `.gitignore`

### Documentation
- [x] Create `README.md`
- [x] Create `docs/architecture.md`
- [x] Create `docs/domain-model.md` (explicit state machine, TaskDelivery semantics)
- [x] Create `docs/protocol.md` (agent protocol + NATS subject schema)

### Architecture Decision Records
- [x] ADR 001 — Language: Rust
- [x] ADR 002 — Message Queue: NATS + JetStream
- [x] ADR 003 — Database: PostgreSQL
- [x] ADR 004 — TUI: ratatui (keybinding spec included)
- [x] ADR 005 — Agent Protocol Design (transport-independent)
- [x] ADR 006 — LLM Provider Abstraction (runtime-configurable, LlmProvider trait)
- [x] ADR 007 — NATS Delivery Semantics (ownership, idempotency, reassignment)
- [x] ADR 008 — Human Approval Boundary (hard gate in domain model)

---

## Phase 1 — Domain & Persistence

### Domain Types
- [x] Define `Task`, `Project`, `Agent`, `Proposal`, `TaskDependency` types in coordinator
- [x] Define `TaskDelivery` type (explicit delivery ownership model)
- [x] Define `OverlapWarning` type
- [x] Define `TaskApproval` type
- [x] All types serialize/deserialize correctly (21/21 unit tests pass)

### Database
- [~] Create `migrations/001_initial_schema.sql`
- [ ] Set up sqlx database connection pool (`coordinator/src/db/pool.rs`)
- [ ] Implement `ProjectRepository`
- [ ] Implement `TaskRepository`
- [ ] Implement `AgentRepository`
- [ ] Implement `TaskDeliveryRepository`
- [ ] Implement `AgentEventRepository`
- [ ] Implement `OverlapWarningRepository`

### Acceptance Criteria — Phase 1
- [ ] All domain types compile and round-trip through serde
- [ ] All repositories compile with correct SQL (sqlx compile-time checked)
- [ ] `sqlx migrate run` succeeds against a running PostgreSQL instance
- [ ] Unit tests pass for domain type construction and state transitions

---

## Phase 2 — Agent Protocol & NATS Infrastructure

### `agent-protocol` crate
- [ ] Define `AgentMessage` enum (all agent → coordinator messages)
- [ ] Define `CoordinatorMessage` enum (all coordinator → agent messages)
- [ ] Define `TaskSpec` struct
- [ ] Define `AgentStatus`, `DeliveryAck` types
- [ ] Round-trip serialization tests for all message types

### NATS Infrastructure (coordinator)
- [ ] NATS connection setup with JetStream (`messaging/client.rs`)
- [ ] Create JetStream streams: `TASK_ASSIGNMENTS`, `AGENT_EVENTS`
- [ ] Implement publisher: coordinator → agent task assignments
- [ ] Implement subscriber: coordinator ← agent events
- [ ] Implement agent registration handler
- [ ] Implement heartbeat monitor (agent timeout detection)

### Mock Agent (`agent-mock`)
- [ ] Implement NATS connection with JetStream consumer
- [ ] Implement task subscription from `TASK_ASSIGNMENTS` stream
- [ ] Report full lifecycle: `TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, `Failed`
- [ ] Configurable work-simulation delay (env var `MOCK_TASK_DELAY_MS`)
- [ ] Implement heartbeat loop
- [ ] Scenario: task that gets `Blocked` then unblocked and `Completed`
- [ ] Implements same `AgentAdapter` trait that agy adapter will implement

### Acceptance Criteria — Phase 2
- [ ] Mock agent connects, receives task, reports full lifecycle to coordinator
- [ ] Coordinator receives all events via `AGENT_EVENTS` JetStream stream
- [ ] JetStream durable delivery verified: restart agent mid-task, task redelivered
- [ ] TaskDelivery record in PostgreSQL reflects correct state after each event

---

## Phase 3 — AI Planning

### `LlmProvider` Trait
- [ ] Define `LlmProvider` trait (`coordinator/src/ai/provider.rs`)
- [ ] Define `ProposedPlan`, `ProposedTask`, `ProposedDependency` types
- [ ] Implement one LLM provider (selected by `AI_PROVIDER` env var)
- [ ] Implement task decomposition prompt with structured JSON output
- [ ] Parse + validate LLM JSON output; handle malformed output gracefully
- [ ] Implement `MockLlmProvider` for testing (returns deterministic plans)

### Acceptance Criteria — Phase 3
- [ ] Given a project description, LLM returns a valid `ProposedPlan` with tasks + dependencies
- [ ] Switching `AI_PROVIDER` env var changes the provider without code changes
- [ ] Malformed LLM output logs a warning and surfaces error in TUI; does not crash
- [ ] `MockLlmProvider` is usable in integration tests without real API keys

---

## Phase 4 — TUI Skeleton

### ratatui Application
- [ ] Set up ratatui + crossterm event loop (`coordinator/src/tui/app.rs`)
- [ ] Implement project input screen (`project_input.rs`)
- [ ] Implement plan review screen with per-task approval flow:
  - [ ] `[Y]` Approve
  - [ ] `[N]` Reject
  - [ ] `[E]` Edit description inline
  - [ ] `[↑/↓]` Navigate between tasks
  - [ ] `[Enter]` Show details pane (dependencies, overlap warnings)
- [ ] Task card widget: title, assigned agent, dependencies, affected resources
- [ ] Details pane: full description + dependency list + overlap warnings
- [ ] Overlap warning banner appears on affected task cards

### Acceptance Criteria — Phase 4
- [ ] TUI renders without errors in a standard terminal
- [ ] All 5 keybindings work correctly in plan review
- [ ] Human can view dependency and overlap info before approving a task
- [ ] Edit flow: `[E]` makes description editable; `Enter` confirms edit + approves

---

## Phase 5 — Coordinator Core

### State Machine
- [ ] Implement coordinator state machine (`coordinator/src/coordinator/state.rs`)
  - States: `Idle` → `ProjectInput` → `Planning` → `HumanReview` → `Assigning` → `Executing` → `Done`
- [ ] Assignment logic: only assigns tasks whose dependencies are `Completed`
- [ ] Approval gating: no task advances past `Approved` to `Assigned` without human action
- [ ] Event processor: update task state from incoming `AgentMessage` events
- [ ] Dependency tracker: gate `Assigned` tasks until blocking tasks reach `Completed`
- [ ] Human can override LLM assignment proposal (reassign to different agent)

### Acceptance Criteria — Phase 5
- [ ] No task reaches `Assigned` state without a `TaskApproval` record with `Approved` status
- [ ] Task B does not start until Task A (blocker) reports `Completed`
- [ ] `Blocked` agent event sets task to `Blocked` in DB and TUI
- [ ] State machine transitions are persisted to PostgreSQL before side effects

---

## Phase 6 — Dashboard & Overlap Detection

### Live Dashboard TUI
- [ ] Implement dashboard screen (`coordinator/src/tui/screens/dashboard.rs`)
- [ ] Show all agents: name, owner, current task, status
- [ ] Show all tasks: short ID, title, status, assigned agent
- [ ] Real-time updates from NATS events (async channel → TUI)

### Overlap Detection
- [ ] Implement resource-level overlap detection (`coordinator/src/coordinator/overlap.rs`)
- [ ] On proposal generation: compare `affected_resources` across all proposed tasks
- [ ] Generate `OverlapWarning` record for each shared resource; persist to DB
- [ ] Surface warning on affected task card in plan review screen
- [ ] Surface active unacknowledged warnings in dashboard

### Acceptance Criteria — Phase 6
- [ ] Dashboard updates live as agent events arrive (no full restart needed)
- [ ] When Task A and Task B share a resource, `OverlapWarning` is created in DB
- [ ] Warning visible in plan review screen before human approves the task
- [ ] Warning visible in dashboard for unacknowledged overlaps

---

## Phase 7 — Integration & Demo

### End-to-End Validation
- [ ] Integration test: 2 mock agents, project description, full lifecycle
- [ ] Demonstrate dependency chain: Task B waits for Task A `Completed`
- [ ] Demonstrate overlap warning: Task A and Task B share a resource path
- [ ] All 10 v0.1 success criteria satisfied (see `docs/architecture.md`)

### Deployment
- [ ] Full stack starts cleanly via `docker compose up`
- [ ] README setup instructions verified end-to-end by following them literally
- [ ] No hardcoded secrets in source or Docker images

### Acceptance Criteria — v0.1 Complete
- [ ] Human enters project description in TUI ✓
- [ ] LLM produces dependency-aware task plan ✓
- [ ] Human reviews tasks one-at-a-time with Y/N/E/↑↓/Enter ✓
- [ ] Coordinator assigns approved tasks, dependency-ordered ✓
- [ ] Tasks delivered via NATS JetStream to mock agents ✓
- [ ] Mock agents report: started, progress, blocked, completed ✓
- [ ] TUI live dashboard shows agent + task state ✓
- [ ] One dependency chain demonstrated ✓
- [ ] One overlap warning demonstrated ✓
- [ ] Full stack starts with `docker compose up` ✓
