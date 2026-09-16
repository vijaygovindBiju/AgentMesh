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

## v0.1 Success Criteria

| # | Criterion |
|---|---|
| 1 | Human enters project description in TUI |
| 2 | LLM produces dependency-aware task plan |
| 3 | Human reviews tasks one-at-a-time with Y/N/E/↑↓/Enter |
| 4 | Coordinator assigns approved tasks, dependency-ordered |
| 5 | Tasks delivered via NATS JetStream to mock agents |
| 6 | Mock agents report: started, progress, blocked, completed |
| 7 | TUI live dashboard shows agent + task state |
| 8 | One dependency chain demonstrated (B waits for A) |
| 9 | One overlap warning demonstrated (two tasks, same resource) |
| 10 | Full stack starts with `docker compose up` |

---

## Related Documents

- [`docs/domain-model.md`](domain-model.md) — domain types and state machines
- [`docs/protocol.md`](protocol.md) — agent protocol specification
- [`docs/decisions/`](decisions/) — Architecture Decision Records
