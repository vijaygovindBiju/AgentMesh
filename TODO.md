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
- [x] Create `migrations/001_initial_schema.sql`
- [x] Set up sqlx database connection pool (`coordinator/src/db/pool.rs`)
- [x] Implement `ProjectRepository`
- [x] Implement `TaskRepository`
- [x] Implement `AgentRepository`
- [x] Implement `TaskDeliveryRepository`
- [x] Implement `AgentEventRepository`
- [x] Implement `OverlapWarningRepository`

### Acceptance Criteria — Phase 1
- [x] All domain types compile and round-trip through serde (21 tests pass)
- [x] All repositories compile with correct SQL (sqlx compile-time checked)
- [x] `sqlx migrate run` succeeds against a running PostgreSQL instance (verified on PostgreSQL 16)
- [x] Unit tests pass for domain type construction and state transitions (28/28 tests pass)

---

## Phase 2 — Agent Protocol & NATS Infrastructure

### `agent-protocol` crate
- [x] Define `AgentMessage` enum (all agent → coordinator messages)
- [x] Define `CoordinatorMessage` enum (all coordinator → agent messages)
- [x] Define `TaskSpec` struct
- [x] Define `AgentStatus`, `DeliveryAck` types
- [x] Round-trip serialization tests for all message types

### NATS Infrastructure (coordinator)
- [x] NATS connection setup with JetStream (`messaging/client.rs`)
- [x] Create JetStream streams: `TASK_ASSIGNMENTS`, `AGENT_EVENTS`
- [x] Implement publisher: coordinator → agent task assignments
- [x] Implement subscriber: coordinator ← agent events
- [x] Implement agent registration handler
- [x] Implement heartbeat monitor (agent timeout detection)

### Mock Agent (`agent-mock`)
- [x] Implement NATS connection with JetStream consumer
- [x] Implement task subscription from `TASK_ASSIGNMENTS` stream
- [x] Report full lifecycle: `TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, `Failed`
- [x] Configurable work-simulation delay (env var `MOCK_TASK_DELAY_MS`)
- [x] Implement heartbeat loop
- [x] Scenario: task that gets `Blocked` then unblocked and `Completed`
- [x] Implements same `AgentAdapter` trait that agy adapter will implement

### Acceptance Criteria — Phase 2
- [x] Mock agent connects, receives task, reports full lifecycle to coordinator
- [x] Coordinator receives all events via `AGENT_EVENTS` JetStream stream
- [x] JetStream durable delivery verified: restart agent mid-task, task redelivered
- [x] TaskDelivery record in PostgreSQL reflects correct state after each event

---

## Phase 3 — AI Planning

### AI Planning Layer (`coordinator/src/ai/`)
- [x] Define `LlmProvider` trait and planning schemas (`ai/provider.rs`, `ai/schema.rs`)
- [x] Implement deterministic plan validator (`ai/validator.rs`): cycles, dependencies, agents, overlaps, non-empty tasks
- [x] Implement `MockLlmProvider` for deterministic testing
- [x] Comprehensive validation & test matrix (malformed JSON, broken deps, cycles, invalid agents, empty plan)
- [x] Implement prompt templates with structured JSON output schema (`ai/prompts.rs`)
- [x] Implement real LLM provider (`ai/anthropic.rs` or `ai/gemini.rs` via reqwest, runtime configurable)
- [x] Proposal persistence service (`ai/service.rs`): converts validated plan into PostgreSQL `Proposal` & `tasks`

### Acceptance Criteria — Phase 3
- [x] Given a project description, LLM returns a valid `ProposedPlan` with tasks + dependencies
- [x] Switching `AI_PROVIDER` env var changes the provider without code changes
- [x] Malformed LLM output logs a warning and surfaces error in TUI; does not crash
- [x] `MockLlmProvider` is usable in integration tests without real API keys

---

## Phase 4 — TUI Skeleton

### ratatui Application
- [x] Set up ratatui + crossterm event loop (`coordinator/src/tui/mod.rs`)
- [x] Implement project input screen (`project_input.rs`)
- [x] Implement plan review screen with per-task approval flow:
  - [x] `[Y]` Approve
  - [x] `[N]` Reject
  - [x] `[E]` Edit description inline
  - [x] `[↑/↓]` Navigate between tasks
  - [x] `[Enter]` Show details pane (dependencies, overlap warnings)
- [x] Task card widget: title, assigned agent, dependencies, affected resources
- [x] Details pane: full description + dependency list + overlap warnings
- [x] Overlap warning banner appears on affected task cards

### Acceptance Criteria — Phase 4
- [x] TUI renders without errors in a standard terminal
- [x] All 5 keybindings work correctly in plan review
- [x] Human can view dependency and overlap info before approving a task
- [x] Edit flow: `[E]` makes description editable; `Enter` confirms edit + approves

---

## Phase 5 — Coordinator Core

### State Machine
- [x] Implement coordinator state machine (`coordinator/src/coordinator/state.rs`)
  - States: `Idle` → `ProjectInput` → `Planning` → `HumanReview` → `Assigning` → `Executing` → `Done`
- [x] Assignment logic: only assigns tasks whose dependencies are `Completed`
- [x] Approval gating: no task advances past `Approved` to `Assigned` without human action
- [x] Event processor: update task state from incoming `AgentMessage` events
- [x] Dependency tracker: gate `Assigned` tasks until blocking tasks reach `Completed`
- [x] Human can override LLM assignment proposal (reassign to different agent)

### Acceptance Criteria — Phase 5
- [x] No task reaches `Assigned` state without a `TaskApproval` record with `Approved` status (verified in tests)
- [x] Task B does not start until Task A (blocker) reports `Completed` (verified in tests)
- [x] `Blocked` agent event sets task to `Blocked` in DB and TUI (verified in tests)
- [x] State machine transitions are persisted to PostgreSQL before side effects (verified in tests)

---

## Phase 6 — Dashboard & Overlap Detection

### Live Dashboard TUI
- [x] Implement dashboard screen (`coordinator/src/tui/screens/dashboard.rs`)
- [x] Show all agents: name, owner, current task, status
- [x] Show all tasks: short ID, title, status, assigned agent
- [x] Real-time updates from NATS events (async channel → TUI)

### Overlap Detection
- [x] Implement resource-level overlap detection (`coordinator/src/coordinator/overlap.rs`)
- [x] On proposal generation: compare `affected_resources` across all proposed tasks
- [x] Generate `OverlapWarning` record for each shared resource; persist to DB
- [x] Surface warning on affected task card in plan review screen
- [x] Surface active unacknowledged warnings in dashboard

### Acceptance Criteria — Phase 6
- [x] Dashboard updates live as agent events arrive (no full restart needed)
- [x] When Task A and Task B share a resource, `OverlapWarning` is created in DB
- [x] Warning visible in plan review screen before human approves the task
- [x] Warning visible in dashboard for unacknowledged overlaps


---

## Phase 7 — Integration & Demo

### End-to-End Validation
- [x] Integration test: 2 mock agents, project description, full lifecycle (`phase7_e2e_demo.rs`)
- [x] Demonstrate dependency chain: Task B waits for Task A `Completed` (verified in tests)
- [x] Demonstrate overlap warning: Task A and Task B share a resource path (verified in tests)
- [x] All 10 v0.1 success criteria satisfied (see `docs/architecture.md`)

### Deployment
- [x] Full stack starts cleanly via `docker compose up` (PostgreSQL 16 + NATS JetStream 2.10 verified healthy)
- [x] README setup instructions verified end-to-end by following them literally
- [x] No hardcoded secrets in source or Docker images (verified across workspace)

### Acceptance Criteria — v0.1 Complete
- [x] Human enters project description in TUI ✓
- [x] LLM produces dependency-aware task plan ✓
- [x] Human reviews tasks one-at-a-time with Y/N/E/↑↓/Enter ✓
- [x] Coordinator assigns approved tasks, dependency-ordered ✓
- [x] Tasks delivered via NATS JetStream to mock agents ✓
- [x] Mock agents report: started, progress, blocked, completed ✓
- [x] TUI live dashboard shows agent + task state ✓
- [x] One dependency chain demonstrated ✓
- [x] One overlap warning demonstrated ✓
- [x] Full stack starts with `docker compose up` ✓
