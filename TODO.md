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

## Current State

- [x] v0.1 — Core Coordination System (DONE)
- [x] Phase 8 — Real agy Integration (DONE)
- [x] Phase 9 — Real Repository / Git Coordination (DONE)
- [x] Phase 10 — Intelligent Planning v1 (DONE)
- [x] Phase 11 — Agent Capability System (DONE)
- [x] Phase 12 — Security (DONE)
- [ ] Phase 13 — Observability (NEXT)

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

---

# AgentMesh — v1.0 Roadmap

## Phase 8 — Real agy Integration

**Goal:** Replace mock execution with real agy instances.

- [x] 8.1 Audit current agent-agy scaffold
- [x] 8.2 Define real agy adapter contract
- [x] 8.3 Implement agy process lifecycle
- [x] 8.4 Connect agy adapter to Agent Protocol
- [x] 8.5 TaskAssignment → agy execution
- [x] 8.6 agy → TaskStarted
- [x] 8.7 agy → ProgressUpdate
- [x] 8.8 agy → Completed
- [x] 8.9 agy → Failed
- [x] 8.10 agy → Blocked
- [x] 8.11 Handle agy process crash
- [x] 8.12 Handle timeout
- [x] 8.13 Handle cancellation
- [x] 8.14 Test one real agy instance
- [x] 8.15 Test two real agy instances
- [x] 8.16 Test two agy instances on different machines

### Acceptance Criteria — Phase 8
- [x] Two real agy instances can receive different tasks from one AgentMesh coordinator and report their lifecycle correctly. ✓

---

## Phase 9 — Real Repository / Git Coordination [x] DONE

**Goal:** Make AgentMesh safe for real coding projects.

- [x] 9.1 Define repository identity (`coordinator/src/git/identity.rs`: `RepositoryIdentity` discovers root, base branch, HEAD SHA, remote URL, validates clean tree)
- [x] 9.2 Define agent workspace identity (`coordinator/src/git/workspace.rs`: `AgentWorkspace` manages isolated Git worktrees per task)
- [x] 9.3 Connect agent to repository (`TaskSpec` carries `repo_path`, `task_branch`, `base_branch`; `AgyProcess` and `AgyAgentRunner` execute agents in task worktree)
- [x] 9.4 Define branch strategy (`coordinator/src/git/branch.rs`: `BranchStrategy` formats `agentmesh/<short_id>`, checks existence, lists branches, handles deletion)
- [x] 9.5 Track branch per task (`tasks.task_branch`, `tasks.base_commit_sha`, `tasks.completion_commit_sha`, `tasks.actual_modified_resources` in PostgreSQL)
- [x] 9.6 Track files/resources modified by agents (`coordinator/src/git/changes.rs`: `ResourceTracker` detects untracked, uncommitted, and committed changes vs base commit)
- [x] 9.7 Detect unexpected resource changes (`coordinator/src/git/changes.rs`: compares modified files against planned `affected_resources`; records in `unexpected_resource_changes` table)
- [x] 9.8 Detect cross-agent Git conflicts (`coordinator/src/git/conflict.rs`: `ConflictDetector` runs 3-way `git merge-tree` simulation; records in `git_conflicts` table)
- [x] 9.9 Prevent unsafe simultaneous modifications (`ConflictDetector::check_concurrency_safety` flags overlapping resource footprints with `ConcurrencySafety::OverlapRisk`)
- [x] 9.10 Define task completion → Git state (`coordinator/src/git/completion.rs`: `CompletionManager` commits uncommitted agent work, records completion SHA, verifies clean mergeability to base branch, cleans up worktree)
- [x] 9.11 Test parallel coding workflow (`crates/coordinator/tests/phase9_git_integration.rs`: 6 integration tests verifying worktree isolation, zero overwriting, unexpected changes, merge conflicts, and JetStream parallel execution)

### Acceptance Criteria — Phase 9
Two real agents can work on separate tasks in the same project without silently overwriting each other's work. ✓ Verified in `phase9_git_integration.rs` and worktree isolation tests.

---

## Phase 10 — Intelligent Planning v1 [x] DONE

**Goal:** Improve the planner for real software projects.

- [x] 10.1 Improve project understanding (`RepositoryScanner` extracts ecosystems, languages, sample file tree, and README summaries)
- [x] 10.2 Repository-aware planning (`PlanningService::generate_repo_aware_plan` injects architecture into prompts)
- [x] 10.3 Detect existing architecture (Detects Cargo/Rust, npm/Node/TypeScript, Python, Go ecosystems and workspace crates)
- [x] 10.4 Detect affected files/modules (Tasks grounded in real repository paths with validation)
- [x] 10.5 Detect task dependencies (Enforces DAG dependencies, prevents cycles, persists task relationships)
- [x] 10.6 Detect resource overlap (Detects overlapping file modifications and generates `OverlapWarning` records)
- [x] 10.7 Suggest agent based on capabilities (`AgentCapabilityMatcher` matches agent tags with task descriptions and file extensions)
- [x] 10.8 Estimate task complexity (`ComplexityEstimator` sizes tasks XS/S/M/L/XL and analyzes architectural risk factors)
- [x] 10.9 Re-plan after task completion/failure (`ReplanEngine::gather_replan_context` and `execute_replan` preserve completed work and remediate failed tasks)
- [x] 10.10 Re-plan when project state changes (Incorporates unexpected resource modifications and cross-agent merge conflicts into corrective re-planning)

### Acceptance Criteria — Phase 10
Given a real repository + project requirements, AgentMesh produces a useful dependency-aware execution plan. ✓ Verified in `phase10_intelligent_planning.rs` and full workspace suite (129 passing tests).

---

## Phase 11 — Agent Capability System [x] DONE

**Goal:** Coordinator understands what each agent can do.

- [x] 11.1 Define capability model (`AgentCapabilities`, `RuntimeCapability`, `LanguageCapability`, `ToolCapability`, `TaskRequirements`, `HealthStatus` in `agent-protocol/src/capabilities.rs`)
- [x] 11.2 Register capabilities (`AgentMessage::Register` carries structured profile, `UpdateCapabilities` protocol message, stored in `agents.capability_profile` JSONB)
- [x] 11.3 Detect agent runtime (`CapabilityDetector::detect_runtime` extracts OS, CPU arch, logical cores, adapter details)
- [x] 11.4 Detect language/framework capabilities (`CapabilityDetector::detect_languages` detects Rust, Python, Node/TS, Go, Dart/Flutter, C/C++)
- [x] 11.5 Detect available tools (`CapabilityDetector::detect_tools` discovers git, docker, cargo, sqlx, agy, etc. from PATH)
- [x] 11.6 Capability-based task matching (`AgentCapabilityMatcher::rank_candidates` scores candidates on language, tools, OS, and skill tags with match breakdown)
- [x] 11.7 Agent availability state (`AgentAvailability`, draining state, active task concurrency gating in `AgentRepository::find_available_agents`)
- [x] 11.8 Agent health information (`HealthStatus`, consecutive task failure tracking, heartbeat latency tracking, unhealthy agent task gating)

### Acceptance Criteria — Phase 11
Coordinator assigns tasks based on capabilities + availability + health metrics. ✓ Verified in `phase11_capabilities.rs` and workspace test suite.

*Capability Matching Example:*
```
Agent A:  [rust, linux, backend]
Agent B:  [flutter, dart, frontend]
Coordinator: assigns tasks based on requirements + availability.
```

---

## Phase 12 — Security [x] DONE

**Goal:** Make remote agents safe to operate.

- [x] 12.1 Agent authentication (`ApiKeyManager` generates `am_ak_` keys, SHA-256 hashing, constant-time verification, session tokens)
- [x] 12.2 Agent authorization (`AgentRole` enum: Worker, Reviewer, ReadOnly, Admin; enforced at task assignment and completion)
- [x] 12.3 Secure NATS configuration (`NatsSecurityConfig` auth tokens, user/pass credentials, subject gating in `NatsSubjectAuthorizer`)
- [x] 12.4 TLS (TLS connection support, root CA verification, and mTLS client cert configuration in `connect_secure`)
- [x] 12.5 Credential management (Secure key generation, key rotation via `AgentRepository::rotate_api_key`, key revocation via `revoke_agent`)
- [x] 12.6 Secret isolation (`SecretScoper` isolates execution environments; `SecretRedactor` redacts keys, passwords, private keys in logs/JSON)
- [x] 12.7 Agent permission boundaries (`PermissionBoundary` glob paths denylists/allowlists, code modification flags, enforced in `PermissionEnforcer`)
- [x] 12.8 Task authorization (`TaskAuthorizer` enforces task ownership, rejects impersonation, prevents unauthorized task events in subscriber)
- [x] 12.9 Audit security-sensitive actions (`AuditLogger` and `AuditRepository` record alerts and auth events in `audit_logs` table)
- [x] 12.10 Security testing (`crates/coordinator/tests/phase12_security.rs`: 8 passing integration tests verifying full security matrix)

### Acceptance Criteria — Phase 12
Remote agents operate within strict authentication, authorization, and permission boundaries without secret leakage or impersonation. ✓ Verified in `phase12_security.rs` and workspace test suite.

---

## Phase 13 — Observability

**Goal:** Understand everything happening in the system.

- [ ] 13.1 Structured logs
- [ ] 13.2 Task execution timeline
- [ ] 13.3 Agent activity timeline
- [ ] 13.4 NATS delivery visibility
- [ ] 13.5 Failure diagnostics
- [ ] 13.6 Coordinator events
- [ ] 13.7 Persistent audit log
- [ ] 13.8 Execution metrics
- [ ] 13.9 TUI diagnostics

---

## Phase 14 — Production-Quality Reliability

**Goal:** Make the coordinator resilient.

- [ ] 14.1 Coordinator restart recovery
- [ ] 14.2 Agent reconnect recovery
- [ ] 14.3 NATS reconnect handling
- [ ] 14.4 Database reconnect handling
- [ ] 14.5 Stale task recovery
- [ ] 14.6 Duplicate event handling
- [ ] 14.7 Network partition testing
- [ ] 14.8 Agent crash testing
- [ ] 14.9 Coordinator crash testing
- [ ] 14.10 End-to-end recovery tests

---

## Phase 15 — v1.0 End-to-End Validation

**Goal:** Validate the original AgentMesh idea.

- [ ] 15.1 Real project repository
- [ ] 15.2 Two real agy agents
- [ ] 15.3 Two different machines
- [ ] 15.4 AI-generated task decomposition
- [ ] 15.5 Human task approval
- [ ] 15.6 Parallel task execution
- [ ] 15.7 Dependency enforcement
- [ ] 15.8 Overlap detection
- [ ] 15.9 Agent failure/recovery
- [ ] 15.10 Git integration
- [ ] 15.11 Complete project execution
- [ ] 15.12 Document results

---

## v1.0 Acceptance Criteria

- [ ] Human provides a real software project.
- [ ] AgentMesh understands the project and proposes tasks.
- [ ] Human approves the plan.
- [ ] AgentMesh identifies dependencies and overlaps.
- [ ] Multiple real agy agents connect remotely.
- [ ] Coordinator assigns appropriate tasks.
- [ ] Agents execute tasks independently.
- [ ] Agents communicate lifecycle/progress through AgentMesh.
- [ ] Dependencies prevent unsafe execution order.
- [ ] Overlapping work is detected.
- [ ] Agent failures do not corrupt coordinator state.
- [ ] Git/repository state remains controlled.
- [ ] Human remains the final authority.
- [ ] Complete workflow works across two physical machines.

---

## Final Principle

> **AI suggests.**  
> **Humans decide.**  
> **Agents execute.**  
> **AgentMesh coordinates.**  
> **PostgreSQL is the source of truth.**  
> **NATS transports the work.**  
> **Agent Protocol keeps runtimes interchangeable.**

