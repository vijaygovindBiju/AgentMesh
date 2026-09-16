# AgentMesh

> An AI-powered project coordination system for multiple human-controlled AI coding agents.

AgentMesh sits above your repository workflow. It decomposes work, detects overlap, proposes task assignments across multiple AI coding agents, and asks humans to approve before anything executes.

**Core principle:** AI suggests → Humans decide → Agents execute.

---

## Problem

When multiple developers each use their own AI coding agent on the same project, coordination breaks down:

- Agents misunderstand task boundaries
- Overlapping work gets done twice
- Different technical assumptions lead to incompatible code
- Shared interfaces (models, APIs, schemas) get modified concurrently
- Integration becomes a serious problem

AgentMesh provides a central coordination layer to reduce these problems.

---

## Architecture Overview

```
HUMAN TEAM
    │
    ▼
COORDINATOR (this system)
    │ analyzes project
    │ proposes task plan
    │ asks humans to approve
    │
    ├──────────────────────┐
    ▼                      ▼
Human A reviews        Human B reviews
    │                      │
    ▼                      ▼
[APPROVED]             [APPROVED]
    │                      │
    ▼                      ▼
NATS JetStream  ←  PostgreSQL (source of truth)
    │                      
    ├──────────────────────┐
    ▼                      ▼
Agent A                Agent B
(mock / agy)           (mock / agy)
    │                      │
    └──────────┬───────────┘
               ▼
         Shared Project
```

See [`docs/architecture.md`](docs/architecture.md) for the full architecture.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable, 1.75+)
- [Docker](https://docs.docker.com/get-docker/) and Docker Compose v2
- An API key for at least one supported LLM provider (Anthropic, OpenAI, or Gemini)

---

## Quick Start

### 1. Clone and configure

```bash
git clone <repo-url>
cd AgentMesh
cp .env.example .env
# Edit .env and fill in your AI provider API key
```

### 2. Start infrastructure

```bash
docker compose up -d
```

This starts:
- **PostgreSQL 16** on `localhost:5432`
- **NATS 2.x + JetStream** on `localhost:4222` (monitoring at `localhost:8222`)

### 3. Run database migrations

```bash
# Install sqlx-cli if you don't have it
cargo install sqlx-cli --no-default-features --features rustls,postgres

sqlx migrate run
```

### 4. Start the coordinator

```bash
cargo run --bin coordinator
```

### 5. Start mock agents (in separate terminals)

```bash
# Agent A
MOCK_AGENT_ID=mock-agent-a MOCK_AGENT_OWNER=Alice cargo run --bin agent-mock

# Agent B
MOCK_AGENT_ID=mock-agent-b MOCK_AGENT_OWNER=Bob cargo run --bin agent-mock
```

---

## Project Structure

```
AgentMesh/
├── Cargo.toml                  # Cargo workspace
├── docker-compose.yml          # NATS + PostgreSQL
├── .env.example                # Required environment variables
├── TODO.md                     # Authoritative implementation task list
│
├── docs/
│   ├── architecture.md         # System architecture
│   ├── domain-model.md         # Domain types and state machines
│   ├── protocol.md             # Agent protocol specification
│   └── decisions/              # Architecture Decision Records (ADRs)
│
├── migrations/                 # PostgreSQL schema migrations
│
└── crates/
    ├── coordinator/            # Main binary: TUI + AI planning + coordination
    ├── agent-protocol/         # Shared message types (no runtime deps)
    ├── agent-mock/             # Mock agent: full protocol, simulated work
    └── agent-agy/              # agy adapter: subprocess + stdout parsing
```

---

## Technology

| Component | Technology |
|---|---|
| Language | Rust |
| TUI | ratatui + crossterm |
| Database | PostgreSQL 16 (via sqlx) |
| Message queue | NATS 2.x + JetStream |
| AI provider | Configurable: Anthropic / OpenAI / Gemini |
| Deployment | Docker Compose |

---

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — system design
- [`docs/domain-model.md`](docs/domain-model.md) — domain types and state machines
- [`docs/protocol.md`](docs/protocol.md) — agent protocol
- [`docs/decisions/`](docs/decisions/) — Architecture Decision Records
- [`TODO.md`](TODO.md) — implementation status

---

## Status

**Phase 0 — Foundation** (in progress)

See [`TODO.md`](TODO.md) for full implementation status.
