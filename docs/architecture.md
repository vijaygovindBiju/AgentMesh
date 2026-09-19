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
│  │    Y/N/E/↑↓/Enter    │    │  ● Dependency graph            │  │
│  │  ● Live dashboard    │    │  ● Assignment logic            │  │
│  │  ● Overlap warnings  │    │  ● Overlap detector            │  │
│  └──────────────────────┘    │  ● Event processor             │  │
│                              └──────────────┬──────────────────┘  │
│                                             │                    │
│                              ┌──────────────▼──────────────────┐  │
│                              │        LlmProvider Layer         │  │
│                              │  (trait, not coupled to vendor)  │  │
│                              │  ├── AnthropicProvider           │  │
│                              │  ├── OpenAiProvider              │  │
│                              │  └── GeminiProvider              │  │
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

The v1.0 architecture expands beyond the initial prototype to provide an enterprise-grade coordination platform:

1. **Intelligent Planning & Dynamic Re-planning (Phases 3 & 10):**
   - AST / crate discovery via `RepositoryScanner`.
   - Dynamic replanning engine (`ReplanEngine`) responding to completed/failed tasks.
2. **Real Agent Integration (`agy`) & Worktree Isolation (Phases 8 & 9):**
   - Headless CLI subprocess management via `AgyAgent` and `AgyProcess`.
   - Isolated Git worktrees (`AgentWorkspace`) per task preventing index locking and cross-agent code contamination.
3. **Agent Capability Matching & Health Gating (Phase 11):**
   - Deterministic capability scoring (`AgentCapabilityMatcher`) and operational health tracking (`Healthy`, `Degraded`, `Unhealthy`).
4. **Security, Authentication & Audit Logging (Phase 12):**
   - SHA-256 API key authentication, constant-time verification, path-based `PermissionBoundary`, secret redaction, and immutable `audit_logs`.
5. **Observability, Timelines & Diagnostics (Phase 13):**
   - Structured JSON logging, real-time metrics, end-to-end task/agent timelines (`TimelineService`), automated failure root-cause analysis (`FailureDiagnostics`), and TUI Diagnostics screen.
6. **Production Reliability & Self-Healing (Phase 14):**
   - Startup recovery (`CoordinatorRecoveryService`), stale task sweeper (`StaleTaskSweeper`), TTL event deduplication (`EventDeduplicator`), and exponential backoff retry (`ResilientConnection`).

---

## Related Documents

- [`docs/domain-model.md`](domain-model.md) — Domain types, task state machine, and invariants
- [`docs/protocol.md`](protocol.md) — Agent protocol specification, JetStream streams, and message schemas
- [`docs/v1-validation.md`](v1-validation.md) — End-to-end v1.0 multi-agent validation report and test matrix
- [`docs/decisions/`](decisions/) — Architecture Decision Records:
  - [`ADR 001 — Rust Implementation Language`](decisions/001-language-rust.md)
  - [`ADR 002 — NATS JetStream Message Transport`](decisions/002-message-queue-nats-jetstream.md)
  - [`ADR 003 — PostgreSQL 16 State Store`](decisions/003-database-postgresql.md)
  - [`ADR 004 — Ratatui Terminal User Interface`](decisions/004-tui-ratatui.md)
  - [`ADR 005 — Transport-Independent Agent Protocol`](decisions/005-agent-protocol-design.md)
  - [`ADR 006 — LLM Provider Abstraction`](decisions/006-llm-provider-abstraction.md)
  - [`ADR 007 — NATS Delivery Semantics & Idempotency`](decisions/007-nats-delivery-semantics.md)
  - [`ADR 008 — Human Approval Boundary`](decisions/008-human-approval-boundary.md)
  - [`ADR 009 — Agent Capability System & Health Gating`](decisions/009-agent-capability-system.md)
  - [`ADR 010 — Security Architecture & Audit Boundaries`](decisions/010-security-and-audit-boundaries.md)
  - [`ADR 011 — Observability, Timelines & Diagnostics`](decisions/011-observability-and-diagnostics.md)
  - [`ADR 012 — Production Reliability & Stale Task Recovery`](decisions/012-resilience-and-stale-task-recovery.md)
