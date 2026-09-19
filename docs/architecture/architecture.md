# AgentMesh — Architecture

## System Overview

AgentMesh is a central AI coordinator for multiple human-controlled AI coding agents. The coordinator analyzes projects, proposes task plans using an LLM, collects human approval, then delivers tasks to agents via NATS JetStream. PostgreSQL is the authoritative state store; NATS is the transport layer only.

---

## Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                       Coordinator Process                        │
│                                                                  │
│  ┌──────────────────────┐    ┌────────────────────────────────┐  │
│  │    TUI (ratatui)     │◄──►│       Coordinator Core         │  │
│  │                      │    │                                │  │
│  │  ● Project input     │    │  ● Project state machine       │  │
│  │  ● Plan review       │    │  ● Task decomposer (LLM)       │  │
│  │    Y/N/E/A/↑↓/Enter  │    │  ● Dependency graph            │  │
│  │  ● Live dashboard    │    │  ● Assignment logic            │  │
│  │  ● Diagnostics       │    │  ● Overlap detector            │  │
│  │  ● Overlap warnings  │    │  ● Event processor             │  │
│  └──────────────────────┘    └──────────────┬──────────────────┘  │
│                                             │                    │
│                              ┌──────────────▼──────────────────┐  │
│                              │        LlmProvider Layer         │  │
│                              │  (trait, not coupled to vendor)  │  │
│                              │  ├── MockLlmProvider (default)   │  │
│                              │  └── AnthropicProvider           │  │
│                              │  (OpenAI/Gemini: not implemented)│  │
│                              └─────────────────────────────────┘  │
└────────────────────────┬─────────────────────────────────────────┘
                         │
             ┌───────────┴────────────┐
             │                        │
     ┌───────▼────────┐    ┌──────────▼──────────┐
     │  PostgreSQL 16  │    │  NATS 2.x JetStream  │
     │                 │    │                      │
     │  Source of      │    │  Transport layer     │
     │  truth for:     │    │  only — not the DB   │
     │  • Projects     │    │                      │
     │  • Tasks        │    │  Streams:            │
     │  • TaskDeps     │    │  TASK_ASSIGNMENTS    │
     │  • Agents       │    │  AGENT_EVENTS        │
     │  • TaskDelivery │    │                      │
     │  • Proposals    │    │  Subjects:           │
     │  • Approvals    │    │  coordinator.tasks   │
     │  • AgentEvents  │    │    .assign.{agent}   │
     │  • Overlaps     │    │  agents.{agent}      │
     └─────────────────┘    │    .events           │
                            │  coordinator.agents  │
                            │    .register         │
                            │    .heartbeat.{agent}│
                            └──────────┬───────────┘
                                       │
                       ┌───────────────┴───────────────┐
                       │                               │
             ┌─────────▼──────────┐        ┌──────────▼───────────┐
             │   Mock Agent        │        │   agy Adapter        │
             │                    │        │                      │
             │  Implements full   │        │  Spawns agy CLI      │
             │  AgentAdapter      │        │  as subprocess       │
             │  protocol          │        │  Parses stdout       │
             │  Simulates work    │        │  Translates to       │
             │  with delays       │        │  AgentMessage types  │
             └────────────────────┘        └──────────────────────┘
```

---

## Core Design Principles

### 1. PostgreSQL is authoritative
NATS is the transport layer. Task state (status, assignment, delivery, history) always lives in PostgreSQL. NATS events trigger state updates in PostgreSQL; they are not the state themselves.

### 2. LLM proposes; humans decide; coordinator enforces
No task advances past `HumanReview` without explicit human approval. The coordinator's deterministic logic handles state transitions after approval; the LLM is only invoked for planning.

### 3. Agent protocol is transport-independent
The `agent-protocol` crate defines message types with no dependency on NATS, HTTP, or any specific transport. All adapters (mock, agy, future) implement the same interface.

### 4. LlmProvider is runtime-configurable
The coordinator core depends on a `LlmProvider` trait. The concrete provider is selected at startup from environment configuration. The coordinator never imports provider-specific code directly.

### 5. Explicit delivery semantics
Task delivery is not a generic queue. `TaskDelivery` records in PostgreSQL track agent identity, attempt number, idempotency keys, acknowledgements, and reassignment history.

---

## v1.0 Architecture Additions

v1.0 (Phases 8–15) adds the following modules to the v0.1 core. Each is implemented in `crates/coordinator/src/` and covered by the corresponding `tests/phaseN_*.rs` suite. Modules marked *library* are exercised by tests and available as APIs but are not yet invoked automatically by the interactive `coordinator` binary (see the README "Current Status" section).

1. **Intelligent Planning & Dynamic Re-planning (Phases 3 & 10)** — `ai/`
   - Repository discovery via `RepositoryScanner` (ecosystems, languages, crates, file tree) feeding repository-aware prompts. *Wired: the binary scans its working directory at planning time.*
   - `ReplanEngine` builds a corrective proposal from completed/failed/blocked tasks, unexpected resource changes and Git conflicts. *Library.*
2. **Real Agent Integration (`agy`) & Worktree Isolation (Phases 8 & 9)** — `crates/agent-agy`, `git/`
   - `AgyAgent` / `AgyProcess` / `AgyStreamEvent` manage the `agy` CLI subprocess and translate its NDJSON stream into protocol events. *Wired (separate binary).*
   - `AgentWorkspace` creates one Git worktree per task on an `agentmesh/<short-id>` branch; `ResourceTracker`, `ConflictDetector` and `CompletionManager` audit and finalize the result. *Library; `AssignmentService` forwards branch/path info when present.*
3. **Agent Capability Matching & Health Gating (Phase 11)** — `agent-protocol/capabilities.rs`, `ai/matcher.rs`
   - `AgentCapabilityMatcher` scores candidates on languages, tools, OS and tags; `HealthStatus` and availability gate assignment. *Wired: suggestions at planning time; availability/health filters in the assignment query.*
4. **Security, Authentication & Audit Logging (Phase 12)** — `security/`
   - SHA-256 `am_ak_` API keys with constant-time verification, `AgentRole`, path-based `PermissionBoundary`, `TaskAuthorizer`, `SecretRedactor`, `audit_logs`. *Wired in registration, assignment and event ingestion. TLS/mTLS via `connect_secure` is library-only.*
5. **Observability, Timelines & Diagnostics (Phase 13)** — `observability/`
   - `TimelineService`, `DeliveryDiagnostics`, `FailureDiagnostics`, `MetricsCollector`, `coordinator_events`, and the Diagnostics TUI screen. *Screen wired; metric/timeline feeds are library.*
6. **Reliability (Phase 14)** — `reliability/`
   - `CoordinatorRecoveryService` (startup), `StaleTaskSweeper`, `EventDeduplicator`, `ResilientConnection`. *Deduplication wired; recovery and sweeper are library (exposed via `CoordinatorCore`).*

---

## Related Documents

- [`README.md`](../../README.md) — installation, quick start, agent setup, multi-machine deployment, troubleshooting, current status
- [`domain-model.md`](domain-model.md) — Domain types, task state machine, and invariants
- [`../protocols/agent-protocol.md`](../protocols/agent-protocol.md) — Agent protocol specification, JetStream streams, and message schemas
- [`../deployment/multi-machine.md`](../deployment/multi-machine.md) — Running agents on other machines
- [`../development/v1-validation.md`](../development/v1-validation.md) — End-to-end v1.0 multi-agent validation report and test matrix
- [`../decisions/`](../decisions/) — Architecture Decision Records:
  - [`ADR 001 — Rust Implementation Language`](../decisions/001-language-rust.md)
  - [`ADR 002 — NATS JetStream Message Transport`](../decisions/002-message-queue-nats-jetstream.md)
  - [`ADR 003 — PostgreSQL 16 State Store`](../decisions/003-database-postgresql.md)
  - [`ADR 004 — Ratatui Terminal User Interface`](../decisions/004-tui-ratatui.md)
  - [`ADR 005 — Transport-Independent Agent Protocol`](../decisions/005-agent-protocol-design.md)
  - [`ADR 006 — LLM Provider Abstraction`](../decisions/006-llm-provider-abstraction.md)
  - [`ADR 007 — NATS Delivery Semantics & Idempotency`](../decisions/007-nats-delivery-semantics.md)
  - [`ADR 008 — Human Approval Boundary`](../decisions/008-human-approval-boundary.md)
  - [`ADR 009 — Agent Capability System & Health Gating`](../decisions/009-agent-capability-system.md)
  - [`ADR 010 — Security Architecture & Audit Boundaries`](../decisions/010-security-and-audit-boundaries.md)
  - [`ADR 011 — Observability, Timelines & Diagnostics`](../decisions/011-observability-and-diagnostics.md)
  - [`ADR 012 — Production Reliability & Stale Task Recovery`](../decisions/012-resilience-and-stale-task-recovery.md)
