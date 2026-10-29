# AgentMesh 🌐

> **AI suggests → Humans decide → Real coding agents execute → The coordinator enforces.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![NATS](https://img.shields.io/badge/Transport-NATS%20JetStream-brightgreen.svg)](https://nats.io)
[![PostgreSQL](https://img.shields.io/badge/State-PostgreSQL-336791.svg)](https://www.postgresql.org)
[![TUI](https://img.shields.io/badge/UI-Ratatui-purple.svg)](https://ratatui.rs)

---

## 📌 Overview

**AgentMesh** is an open-source distributed coordination and supervisor platform engineered in Rust for orchestrating swarms of autonomous coding agents working concurrently on the same software repository.

Unlike simplistic linear agent scripts that risk merge conflicts, race conditions, and unreviewed code writes, AgentMesh enforces strict architectural boundaries:
- **Repository-Aware Plan DAGs:** Breaks complex high-level engineering objectives into directed acyclic dependency graphs (DAGs) using Google Antigravity (`agy`) or Anthropic Claude.
- **Human-in-the-Loop Gatekeeping:** Agents propose changes; human engineers inspect, edit, approve, or reject each task in an interactive Ratatui terminal UI before any agent is dispatched.
- **Git Worktree Isolation:** Every assigned task executes in its own isolated Git worktree branch, eliminating filesystem write collisions.
- **Durable Event Streaming:** Uses **NATS JetStream** for guaranteed at-least-once task delivery, heartbeats, automatic worker reassignment upon failure, and complete audit logging backed by **PostgreSQL**.
- **Real Agents Only:** Integrates with real external Google Antigravity (`agy`) and Claude processes. Zero simulated or mock agents.

---

## 🏗️ System Architecture

```text
                               ┌────────────────────────────────────────┐
                               │       Human Operator (Keyboard)        │
                               └───────────────────┬────────────────────┘
                                                   │
                                                   ▼
┌───────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                       AgentMesh Coordinator                                           │
│                                                                                                       │
│   ┌─────────────────────┐      ┌─────────────────────────┐      ┌─────────────────────────────────┐   │
│   │    Ratatui TUI      │      │    Plan DAG Engine      │      │        Git Worktree Engine      │   │
│   │  (Screens 1 - 7)    │      │  Validation & Approvals │      │  Branch isolation & Merge checks│   │
│   └──────────┬──────────┘      └────────────┬────────────┘      └────────────────┬────────────────┘   │
└──────────────┼──────────────────────────────┼────────────────────────────────────┼────────────────────┘
               │                              │                                    │
               ▼                              ▼                                    ▼
       ┌───────────────┐              ┌───────────────┐                    ┌───────────────┐
       │  PostgreSQL   │              │     NATS      │                    │  Filesystem   │
       │ Durable State │              │   JetStream   │                    │ Git Worktrees │
       └───────────────┘              └───────┬───────┘                    └───────┬───────┘
                                              │                                    │
                                              ▼                                    ▼
                               ┌─────────────────────────────┐      ┌─────────────────────────────┐
                               │   Worker Node (Local / LAN) │      │   Worker Node (Remote Host) │
                               │   agent-agy adapter         │      │   agent-agy adapter         │
                               │   ↳ Real agy CLI process    │      │   ↳ Real agy CLI process    │
                               └─────────────────────────────┘      └─────────────────────────────┘
```

---

## 🚀 Key Features

- **🎯 Interactive Keyboard TUI:** Built with Ratatui, featuring 7 dedicated views:
  1. `Dashboard` — Live fleet activity, running tasks, and project velocity.
  2. `Project Input` — Create goals, select repositories, and trigger AI planning.
  3. `Plan Review` — Inspect task DAG dependencies, review diffs, edit descriptions, and approve/reject.
  4. `Fleet Inspector` — Real-time telemetry, worker heartbeats, and resource consumption.
  5. `Diagnostics` — IPC latency, queue depth, and database connection metrics.
  6. `Settings` — Runtime configuration, provider selection, and worker timeout tuning.
  7. `Doctor` — Non-destructive verification of system requirements, permissions, and network connectivity.
- **🔒 Git Worktree Concurrency:** Ensures parallel agents never modify the same working tree simultaneously. Pre-checks merge conflicts before task finalization.
- **⚡ Resilient JetStream Transport:** Workers register via cryptographically validated API keys. Tasks support automatic timeouts (`AGY_TIMEOUT_SECS`), exponential backoff retries, and reassignment if a worker dies.
- **🔍 Comprehensive Doctor Engine:** Run `coordinator --doctor` to verify PostgreSQL schemas, NATS streams, Git configurations, and external AI CLI binaries.

---

## 🛠️ Quick Start

### Prerequisites

- **Rust 1.80+** (`rustup default stable`)
- **Docker & Docker Compose** (for local PostgreSQL and NATS)
- **Google Antigravity CLI (`agy`)** or an **Anthropic API Key**

### 1. Start Infrastructure

```bash
# Clone the repository
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# Copy environment configuration
cp .env.example .env

# Start local PostgreSQL and NATS JetStream services
docker compose up -d --wait

# Compile all workspace binaries
cargo build --workspace --release
```

### 2. Launch the Coordinator

```bash
cargo run --bin coordinator -- --tui
```

### 3. Launch a Real Worker

In a separate terminal or remote host connected to the same network:

```bash
export NATS_URL="nats://localhost:4222"
export NATS_AUTH_TOKEN="agentmesh_nats_dev_token"
export AGY_AGENT_OWNER="Local Worker 01"
export AGY_AGENT_API_KEY="am_ak_YOUR_64_CHARACTER_HEX_KEY"

# Start the worker adapter
cargo run --bin agent-agy
```

---

## ⌨️ TUI Keyboard Controls

| Shortcut | Description |
| :--- | :--- |
| `1` – `7` | Direct jump to screen (Dashboard, Input, Review, Fleet, Diagnostics, Settings, Doctor) |
| `Tab` / `[` / `]` | Cycle active screens forward / backward |
| `?` | Open contextual keyboard shortcut modal |
| `:` | Open command palette |
| `I` / `N` | Initiate new project goal input |
| `D` | Submit built-in demo project through AI planner |
| `Y` / `N` | Approve (`Y`) or reject (`N`) focused review task |
| `Shift + A` | Bulk-approve all unblocked DAG tasks |
| `G` | Toggle Directed Acyclic Graph (DAG) tree visualization |
| `R` | Retry failed or cancelled task / re-run Doctor diagnostics |
| `C` | Cancel active task execution |
| `q` / `Ctrl + C` | Safely disconnect and quit |

---

## ⚙️ Environment Configuration

| Variable | Target | Purpose |
| :--- | :--- | :--- |
| `DATABASE_URL` | Coordinator | PostgreSQL connection URI |
| `NATS_URL` | Coordinator & Worker | NATS cluster connection address |
| `NATS_AUTH_TOKEN` | Coordinator & Worker | Shared authentication token |
| `AI_PROVIDER` | Coordinator | Planner engine: `agy` or `anthropic` (default: `agy`) |
| `AI_MODEL` | Coordinator | Specific model identifier passed to planner |
| `ANTHROPIC_API_KEY` | Coordinator | Required when `AI_PROVIDER=anthropic` |
| `AGY_AGENT_OWNER` | Worker | Human-readable worker identifier |
| `AGY_AGENT_API_KEY` | Worker | Worker registration authorization key |
| `AGY_BIN_PATH` | Worker | Explicit path to `agy` executable |
| `AGY_TIMEOUT_SECS` | Worker | Maximum task runtime before abort (default: 600s) |

---

## 🧪 Testing & Validation

```bash
# Code formatting check
cargo fmt --all -- --check

# Compiler check across all targets
cargo check --workspace --all-targets

# Execute unit and integration tests (requires PostgreSQL)
cargo test --workspace
```

---

## 📚 Architectural Documentation

- 📖 **[docs/USER_GUIDE.md](docs/USER_GUIDE.md)** — In-depth setup, TUI controls, and failure recovery.
- 📋 **[WORKFLOW.md](WORKFLOW.md)** — Production two-user workflow runbook.
- 🏛️ **[docs/architecture/architecture.md](docs/architecture/architecture.md)** — Core subsystem design and state machine invariants.
- 🔌 **[docs/protocols/agent-protocol.md](docs/protocols/agent-protocol.md)** — NATS JetStream subject taxonomy and wire schemas.
- 🌐 **[docs/deployment/multi-machine.md](docs/deployment/multi-machine.md)** — Multi-machine LAN/VPN deployment architecture.

---

## 📄 License

AgentMesh is open-source software licensed under the **[MIT License](LICENSE)**.
