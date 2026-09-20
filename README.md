# AgentMesh

> **AI suggests → Humans decide → Agents execute → Coordinator enforces**

AgentMesh is a high-reliability coordination and orchestration layer for multiple AI coding agents working concurrently on the same software project.

When multiple autonomous agents edit code simultaneously, coordination quickly collapses: agents overwrite each other's changes, violate architectural dependencies, duplicate effort, and fail to recover when a process crashes or loses network connectivity. AgentMesh solves this by treating the LLM strictly as an advisor, keeping the human operator in control, and using isolated Git worktrees and a transactional state machine to guarantee project-wide integrity.

PostgreSQL 16 is the authoritative single source of truth; NATS 2.10 JetStream provides durable, transport-independent work distribution across local processes or physical machines; and the Agent Protocol keeps agent runtimes interchangeable (mock agents for deterministic testing or real Antigravity `agy` CLI instances).

---

## Table of Contents

- [The Problem](#the-problem)
- [The Solution](#the-solution)
- [Core Philosophy](#core-philosophy)
- [Architecture](#architecture)
- [Key Features](#key-features)
- [Tech Stack](#tech-stack)
- [System Requirements](#system-requirements)
- [Installation](#installation)
  - [Option A — Automated Setup & Build (`./install.sh`)](#option-a--automated-setup--build-installsh)
  - [Option B — Production Binary Installer (`./scripts/install.sh`)](#option-b--production-binary-installer-scriptsinstallsh)
  - [Option C — Manual Source Build (`cargo build`)](#option-c--manual-source-build-cargo-build)
  - [Notice on Remote Curl Installation](#notice-on-remote-curl-installation)
- [Install on Another Linux Machine (Worker Host)](#install-on-another-linux-machine-worker-host)
- [Backing Infrastructure (Docker Compose)](#backing-infrastructure-docker-compose)
- [Configuration Reference](#configuration-reference)
- [Quick Start](#quick-start)
- [Running the Coordinator](#running-the-coordinator)
- [Running Mock Agents](#running-mock-agents)
- [Running AGY Agents](#running-agy-agents)
- [Multi-Machine Fleet Setup](#multi-machine-fleet-setup)
- [Real Repository Workflow](#real-repository-workflow)
- [Git Worktree & Isolation Model](#git-worktree--isolation-model)
- [Human Approval & Replanning Gate](#human-approval--replanning-gate)
- [Failure & Recovery Behavior](#failure--recovery-behavior)
- [Security Model & Boundaries](#security-model--boundaries)
- [Observability & Diagnostics](#observability--diagnostics)
- [Testing & Quality Assurance](#testing--quality-assurance)
- [Known External Dependencies](#known-external-dependencies)
- [Project Structure](#project-structure)
- [Documentation Index](#documentation-index)
- [Troubleshooting](#troubleshooting)
- [Contributing & License](#contributing--license)

---

## The Problem

When multiple autonomous coding agents are loosed on a repository without centralized coordination:

| Failure Mode | What Goes Wrong Without a Coordinator |
| :--- | :--- |
| **Workspace Collisions** | Two agents edit the same file or schema simultaneously, causing silent overwrites and Git index lock conflicts. |
| **Dependency Violations** | Downstream features are built before required database migrations, data models, or APIs exist. |
| **Silent Failures & Drift** | A crashed or disconnected agent leaves a task stranded indefinitely with no automated recovery. |
| **Uncontrolled Hallucination** | LLMs autonomously re-plan or execute destructive project-wide changes without human approval. |
| **Remote Fleet Fragmentation** | No unified protocol or visibility across agents running across different machines or environments. |
| **Context Blindness** | Agents lack awareness of what other agents are touching, creating severe merge conflicts at review time. |

---

## The Solution

AgentMesh provides the missing systems-level foundation for multi-agent software engineering:

```text
Human Operator
     ↓
AgentMesh Coordinator
     ↓
AI Planning (Repository Discovery + DAG Generation)
     ↓
Human Approval Gate (y / n / e / a / c)
     ↓
Validated Task DAG (PostgreSQL Source of Truth)
     ↓
Agent Capability Matching & Health Gating
     ↓
NATS JetStream WorkQueue Delivery
     ↓
Coding Agents (agent-mock / agent-agy)
     ↓
Isolated Git Worktrees (.agentmesh/worktrees/<short_id>)
     ↓
Lifecycle Streaming (Started, Progress, Blocked, Completed, Failed)
     ↓
Automatic Git Finalization (3-Way Merge Check, Staging, Commit)
     ↓
Automated Recovery (Stale Sweeper, Dependency Unblocking, Dynamic Replanning)
```

---

## Core Philosophy

- **AI suggests.** The LLM scans the repository and proposes a dependency-aware task decomposition. It can only produce `Proposed` tasks.
- **Humans decide.** Every task must be approved (`y`), rejected (`n`), or edited-and-approved (`e`) by the operator in the terminal UI. Critical overlaps require explicit acknowledgement (`a`). Tasks generated by dynamic replanning re-enter `HumanReview` and can never auto-execute.
- **Agents execute.** Agents run isolated in dedicated Git worktrees on separate branches, reporting progress via standard protocol messages over NATS JetStream.
- **Coordinator enforces.** The coordinator runtime validates the DAG, isolates workspaces, detects merge collisions, manages delivery timeouts, recovers crashed tasks, and records auditable metrics.

---

## Architecture

```text
                           HUMAN OPERATOR
                                 │
                                 ▼  (Keyboard: y/n/e/a/c, Tab, 1-4)
                           Ratatui TUI
                                 │
                                 ▼  (TuiAction / CommandHandler)
                        COORDINATOR ENGINE
             ┌───────────────────┼───────────────────┐
             │                   │                   │
             ▼                   ▼                   ▼
       AI Planner &       Task & Delivery      Git Manager &
       DAG Validator       State Machine       Worktrees
             │                   │                   │
             └───────────────────┼───────────────────┘
                                 │
                            PostgreSQL 16
                       (Authoritative Database)
                                 │
                                 ▼
                        NATS 2.10 JetStream
                     (WorkQueue & Event Streams)
                   ┌─────────────┴─────────────┐
                   ▼                           ▼
              agent-mock                   agent-agy
           (Simulated Work)             (Subprocess Mgr)
                                               │
                                               ▼
                                            agy CLI
                                               │
                                               ▼
                                      Coding Agent Process
```

### Subsystem Responsibilities

- **Ratatui TUI (`crates/coordinator/src/tui/`)**: Renders reactive terminal screens (Project Input, Plan Review, Fleet Dashboard, Diagnostics KPI). Emits user intent as `TuiAction`; contains zero database or network logic.
- **Coordinator Core (`crates/coordinator/src/coordinator/`)**: Encodes the task lifecycle state machine (`Proposed → HumanReview → Approved → Assigned → Executing → Completed/Failed/Blocked/Cancelled`). Enforces the human approval gate, coordinates transactional assignment, and detects resource overlaps.
- **AI Planning (`crates/coordinator/src/ai/`)**: Scans codebase structure via `RepositoryScanner`, prompts LLMs (`AnthropicProvider` or `MockLlmProvider`), validates DAG acyclicity via `PlanValidator`, matches agent capabilities, and generates corrective replans via `ReplanEngine`.
- **Git Coordination (`crates/coordinator/src/git/`)**: Automatically provisions isolated worktrees at `.agentmesh/worktrees/<short_id>` on `agentmesh/<short_id>` branches upon task dispatch. Audits modified resources against planned paths, simulates 3-way mergeability via `git merge-tree`, records completion commits, and removes worktrees upon task completion or cancellation.
- **Reliability Loops (`crates/coordinator/src/reliability/`)**: Runs crash recovery on coordinator boot, sweeps stale tasks from offline agents every 5 seconds, reconciles pending deliveries, and drives a 2-second periodic assignment loop for unblocked tasks.
- **Security & Audit (`crates/coordinator/src/security/`, `crates/agent-protocol/src/security.rs`)**: Constant-time SHA-256 API key authentication (`am_ak_*`), role-based permissions (`AgentRole`), path glob boundaries (`PermissionBoundary`), impersonation prevention (`TaskAuthorizer`), and credential redaction (`SecretRedactor`).
- **Observability (`crates/coordinator/src/observability/`)**: Records structured coordinator events, measures execution timelines, diagnoses merge failures with actionable remediation advice, and streams real-time metrics to the TUI.
- **Agent Protocol (`crates/agent-protocol/`)**: Pure, transport-agnostic Serde definitions for all coordinator-agent messages. Zero network dependencies.
- **Mock Agent (`crates/agent-mock/`)**: Protocol-compliant worker with configurable simulated execution delays; used for testing, demos, and CI.
- **AGY Adapter (`crates/agent-agy/`)**: Supervised runner that wraps the Antigravity `agy` CLI, parses real-time NDJSON event streams, dynamically resolves binary paths, handles process timeouts, redacts secrets, and gracefully surfaces upstream quota limits.

---

## Key Features

- **Strict Human Approval Gate**: Zero autonomous task execution without explicit human sign-off. Replanned tasks are strictly quarantined in `HumanReview`.
- **Deterministic DAG Validation**: Rejects cycles, missing dependencies, and self-referencing tasks before database insertion.
- **Git Worktree Isolation**: Zero merge conflicts or overwritten code during parallel execution. Every task runs in its own private checkout.
- **Capability-Based Matching**: Matches tasks to agents based on runtime platform, detected compilers/languages (Rust, Python, Node, Go, Dart), and toolchains (`git`, `docker`, `cargo`, `sqlx`).
- **Automated Startup & Crash Recovery**: Reconciles in-flight deliveries and sweeps orphaned tasks on coordinator startup without human intervention.
- **Autonomous Stale Task Sweeping**: Reclaims tasks from offline or crashed agents every 5s, safely unassigning the worker and queuing the task for reassignment.
- **Dependency Waiting & Unblocking**: Emits `WaitForDependency` notices when an agent is blocked; automatically unblocks and claims dependents when prerequisites complete.
- **Task Cancellation Protocol**: Cancels active work via `TaskCancelled`, frees the worker, marks deliveries terminal, and cleans up worktrees.
- **Multi-Machine Orchestration**: Orchestrates remote agents across distinct physical machines over secure NATS JetStream.
- **Live Diagnostics & KPI Dashboard**: Streams fleet health, delivery status, and structured coordinator events in real time.
- **Secret Redaction**: Automatically sanitizes API tokens (`am_ak_*`, `sk-*`), database connection passwords, and private keys from logs and protocol payloads.

---

## Tech Stack

Every technology listed below is actively used in the AgentMesh codebase:

| Subsystem | Technologies & Crates | Usage in AgentMesh |
| :--- | :--- | :--- |
| **Language & Runtime** | **Rust** (2021 edition, stable 1.75+), **Tokio** (v1, full features) | High-performance, memory-safe asynchronous runtime for the coordinator engine and agent adapters. |
| **Coordinator TUI** | **Ratatui** (v0.28), **Crossterm** (v0.27) | Cross-platform immediate-mode terminal user interface with keyboard navigation and reactive layout. |
| **Authoritative Storage** | **PostgreSQL 16**, **SQLx** (v0.8, runtime-tokio, tls-rustls, postgres) | Authoritative persistence with compile-time checked SQL queries, connection pooling, and embedded compile-time migrations (`sqlx::migrate!`). |
| **Transport & Messaging** | **NATS 2.10**, **async-nats** (v0.35) | High-throughput messaging with **JetStream** (`TASK_ASSIGNMENTS` WorkQueue stream, `AGENT_EVENTS` Limits stream). |
| **AI Planning** | `LlmProvider` trait, `MockLlmProvider`, `AnthropicProvider`, **Reqwest** (v0.12, rustls-tls, json) | Model-agnostic planning layer supporting deterministic offline mocks and Anthropic Claude REST API. |
| **Agents & Protocol** | `agent-protocol`, `agent-mock`, `agent-agy`, **Serde** (v1, derive), **serde_json** | Shared protocol types, NDJSON stream parsing, simulated mock workloads, and `rand` (v0.8) simulation jitter. |
| **Git Coordination** | **Git CLI** (`git worktree`, `git merge-tree`, `git rev-parse`, `git status`) | Workspace isolation, automatic task branch creation (`agentmesh/<short-id>`), 3-way conflict simulation, and clean cleanup. |
| **Security & Auth** | **sha2** (v0.10), **hex** (v0.4), **subtle** (v2.5) | Constant-time SHA-256 API key hashing, `PermissionBoundary` path glob enforcement, and `SecretRedactor` regex sanitization. |
| **Observability** | **tracing** (v0.1), **tracing-subscriber** (v0.3, env-filter, fmt), **chrono** (v0.4), **uuid** (v1, v4) | Structured contextual logging with trace spans, microsecond event timestamps, and UUID correlation IDs. |
| **Infrastructure** | **Docker**, **Docker Compose v2** (`postgres:16-alpine`, `nats:2.10-alpine`) | Standardized backing infrastructure with automated health checks and persistent data volumes. |

---

## System Requirements

### General Requirements
- **Operating System**: Linux (Ubuntu 20.04+, Debian 11+, Arch, Fedora) or macOS (12+). Windows is supported via WSL2.
- **Rust Toolchain**: Stable Rust 1.75+ (Edition 2021) with `cargo` and `rustc`.
- **Git**: Git 2.38+ (supports `git merge-tree --write-tree` for conflict simulation).
- **Docker**: Docker Engine 20.10+ with the Docker Compose v2 plugin (`docker compose`).
- **PostgreSQL & NATS**: Provided automatically via `docker-compose.yml`, or externally via `DATABASE_URL` and `NATS_URL`.

### AGY Agent Requirements (for `agent-agy` only)
- **Antigravity CLI (`agy`)**: The CLI must be installed and authenticated on the agent host.
- **Dynamic Binary Discovery**: AgentMesh dynamically resolves `agy` using the following priority:
  1. `AGY_BIN_PATH` environment variable (if explicitly set);
  2. `$HOME/.local/bin/agy` (if the file exists);
  3. `agy` located on the system `PATH`.
- Verify your local CLI with: `agy --version`.

### AI Provider Requirements (for Coordinator Planner)
- By default, `AI_PROVIDER=mock` requires **no API keys** and runs completely offline.
- When `AI_PROVIDER=anthropic`, an `ANTHROPIC_API_KEY` is required.

---

## Installation

AgentMesh provides two installation scripts depending on your goal:
1. `./install.sh` (Repository Root) — An end-to-end environment bootstrapper that checks tools, generates configuration, starts backing Docker services (PostgreSQL + NATS), compiles the workspace, and optionally installs binaries.
2. `./scripts/install.sh` — A dedicated binary installer and uninstaller that compiles optimized release binaries and installs them into `~/.local/bin` (or a custom `--prefix`).

---

### Option A — Automated Setup & Build (`./install.sh`)

Use `./install.sh` to set up a complete local development or production environment from the repository root:

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# Run the standard environment setup (checks prerequisites, starts Docker, builds debug binaries)
./install.sh

# Or run optimized production setup and install binaries to ~/.local/bin:
./install.sh --release -p ~/.local/bin
```

#### What `./install.sh` Actually Does
1. **Verifies Prerequisites**: Checks for `git`, `curl`, the Rust toolchain (`cargo` and `rustc`), and Docker with the Docker Compose v2 plugin (`docker compose`). If Rust is missing, it offers interactive installation via `rustup`.
2. **Configures Environment**: Safely copies `.env.example` to `.env` if `.env` does not already exist, preserving your existing configuration if present.
3. **Starts Infrastructure Services**: Automatically executes `docker compose up -d` to launch PostgreSQL 16 (`postgres:16-alpine`, port 5432) and NATS 2.10 (`nats:2.10-alpine`, client port 4222, monitoring port 8222) with JetStream enabled, and waits up to 15 seconds for healthy container status. (Can be bypassed with `--skip-docker`).
4. **Compiles Workspace Binaries**: Executes `cargo build --workspace` (or `cargo build --release --workspace` if `--release` / `-r` is passed), building all three workspace binaries:
   - `coordinator` (`crates/coordinator/src/main.rs`)
   - `agent-mock` (`crates/agent-mock/src/main.rs`)
   - `agent-agy` (`crates/agent-agy/src/main.rs`)
5. **Installs Binaries (Optional)**: If `-p, --prefix <DIR>` or `--install` is provided, it installs the three compiled executables into the target directory (`install -m 0755`) and checks whether the directory is in your `PATH`.

#### `./install.sh` Flags and Options
```text
Usage:
  ./install.sh [options]

Options:
  -r, --release          Build optimized release binaries (cargo build --release)
  -p, --prefix <DIR>     Install compiled binaries to target directory (default: ~/.local/bin)
  --install              Install binaries to ~/.local/bin after building
  --skip-docker          Do not start Docker Compose containers
  --skip-build           Skip cargo workspace build
  -y, --yes              Non-interactive mode (accept all prompts)
  -h, --help             Display this help message and exit
```

---

### Option B — Production Binary Installer (`./scripts/install.sh`)

If you want an auditable, lightweight installer dedicated strictly to building and placing release binaries into `~/.local/bin` (e.g. for worker nodes where Docker is not required):

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# Build release binaries and install to ~/.local/bin
./scripts/install.sh
```

#### Installer Features & Options
```bash
./scripts/install.sh --help               # Display help
./scripts/install.sh --prefix /usr/local/bin  # Install system-wide (requires sudo)
./scripts/install.sh --debug              # Build debug binaries instead of release
./scripts/install.sh --skip-build         # Install existing build artifacts without compiling
./scripts/install.sh --skip-env           # Do not create or touch .env
./scripts/install.sh -y                   # Non-interactive mode
./scripts/install.sh --uninstall          # Cleanly remove installed binaries from prefix
```

---

### Option C — Manual Source Build (`cargo build`)

You can compile AgentMesh manually using standard Cargo commands:

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# Build all binaries in release mode
cargo build --release --workspace
```

This compiles three executables in `target/release/`:
- `target/release/coordinator` — Central coordinator TUI, state engine, and Git manager.
- `target/release/agent-mock` — Simulated test worker agent.
- `target/release/agent-agy` — Antigravity CLI agent adapter.

Copy them to your path if desired:
```bash
cp target/release/coordinator target/release/agent-mock target/release/agent-agy ~/.local/bin/
```

---

### Notice on Remote Curl Installation

> [!NOTE]
> A remote one-liner installation (such as `curl -fsSL <url> | bash`) without cloning the repository is **not supported**.
> 
> AgentMesh compiles from source using the Rust toolchain and sqlx compile-time query verification against the embedded migration schemas. It does not currently distribute pre-compiled binary tarballs via GitHub Releases. To install AgentMesh, clone the repository first, then run `./install.sh` or `./scripts/install.sh`.

---

## Install on Another Linux Machine (Worker Host)

When running a multi-machine fleet (coordinator on Machine A, remote agents on Machine B), the worker host **only needs to run the agent adapter**. Worker machines do **not** require PostgreSQL, Docker, or the coordinator binary.

```text
Machine A (Coordinator Host)                       Machine B (Worker Host)
─────────────────────────────                      ───────────────────────
• Coordinator Runtime                              • agent-agy or agent-mock
• PostgreSQL 16 (port 5432)                        • Antigravity agy CLI (installed separately)
• NATS 2.10 (port 4222) ◄────── TCP Connection ────┘ (no Docker or PostgreSQL needed!)
```

### Step 1: System Prerequisites on Worker Host
- **Git** (`sudo apt install git` or `sudo dnf install git`)
- **Rust Toolchain** (if compiling on the worker):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
  ```
- **Antigravity CLI (`agy`)**:
  > [!IMPORTANT]
  > **AgentMesh does NOT install `agy` itself.** You must install and authenticate the Antigravity CLI independently on the worker machine before starting `agent-agy`.
  > Verify with:
  > ```bash
  > agy --version
  > ```
  > Ensure `agy` is authenticated with your AI provider.

### Step 2: Clone and Install Worker Binaries on Worker Host
```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# Compile and install only the agent binaries into ~/.local/bin
cargo build --release --bin agent-agy --bin agent-mock
mkdir -p ~/.local/bin
cp target/release/agent-agy target/release/agent-mock ~/.local/bin/
```
*(Or use `./install.sh --release --skip-docker -p ~/.local/bin`)*

### Step 3: Configure and Launch Worker
On the worker machine, set the environment variables to point across the network to Machine A:

```bash
# Point to Machine A's IP address (LAN or WireGuard/Tailscale VPN)
export NATS_URL="nats://192.168.1.50:4222"
export NATS_AUTH_TOKEN="agentmesh_dev_token"  # Must match Machine A's NATS_AUTH_TOKEN

# Worker configuration
export AGY_AGENT_OWNER="Worker-Linux-01"
export AGY_AGENT_API_KEY="am_ak_$(openssl rand -hex 32)"
export AGY_EFFORT="medium"
export AGY_TIMEOUT_SECS=600

# Start the agent
agent-agy
```

### Step 4: Verify Remote Fleet Registration
On Machine A, open the Coordinator TUI and press `3` to open the Fleet Dashboard. You will see `Worker-Linux-01` registered, displaying its detected toolchains, idle status, and live 5-second heartbeats.

---

## Backing Infrastructure (Docker Compose)

AgentMesh uses PostgreSQL 16 for state persistence and NATS 2.10 for durable JetStream messaging.

Launch the infrastructure:
```bash
docker compose up -d
```

Verify service health:
```bash
docker compose ps
```
Both containers should report `Up (healthy)`:
```text
NAME                 IMAGE                STATUS                    PORTS
agentmesh-nats-1     nats:2.10-alpine     Up (healthy)              0.0.0.0:4222->4222/tcp, 0.0.0.0:8222->8222/tcp
agentmesh-postgres-1 postgres:16-alpine   Up (healthy)              0.0.0.0:5432->5432/tcp
```

### Infrastructure Details
- **PostgreSQL 16**: Port `5432`. User: `agentmesh`, Password: `agentmesh_dev`, Database: `agentmesh`. Data volume: `postgres_data`.
- **NATS 2.10**: Port `4222` (client TCP). Monitoring HTTP: `http://localhost:8222` (`/healthz`, `/varz`, `/jsz`). Token: `agentmesh_dev_token`. Data volume: `nats_data`.

To stop the infrastructure:
```bash
docker compose down
```

To completely reset the database and message streams:
```bash
docker compose down -v
```

---

## Configuration Reference

The **coordinator** automatically loads environment variables from `.env` via `dotenvy`. **Agents (`agent-mock`, `agent-agy`) do not read `.env`**—they read variables from the process environment (export them or prepend them on the command line).

Copy the template:
```bash
cp .env.example .env
```

### Complete Environment Variable Table

| Variable | Required | Default | Used By | Description |
| :--- | :---: | :--- | :--- | :--- |
| `DATABASE_URL` | Yes | `postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh` | Coordinator | PostgreSQL connection string. |
| `POSTGRES_PASSWORD` | Yes | `agentmesh_dev` | Docker Compose | Password for the PostgreSQL container (must match `DATABASE_URL`). |
| `NATS_URL` | Yes | `nats://localhost:4222` | Coordinator, Agents | NATS broker URL. Remote agents replace `localhost` with the host IP. |
| `NATS_AUTH_TOKEN` | Yes | `agentmesh_dev_token` | Coordinator, Agents | Token required to connect to NATS (`-auth` flag in Compose). |
| `AI_PROVIDER` | No | `mock` | Coordinator | AI planner provider: `mock` (built-in offline) or `anthropic` (Claude). |
| `AI_MODEL` | No | `claude-3-5-sonnet-20241022` | Coordinator | Model name used when `AI_PROVIDER=anthropic`. |
| `ANTHROPIC_API_KEY` | Conditional | *(empty)* | Coordinator | Required if `AI_PROVIDER=anthropic`. Never logged or stored in DB. |
| `RUST_LOG` | No | `agentmesh=debug,coordinator=debug,info` | All Binaries | Tracing filter directive. Coordinator logs to stderr. |
| `MOCK_AGENT_OWNER` | No | `MockDev` | `agent-mock` | Human owner label displayed in the fleet dashboard. |
| `MOCK_AGENT_ID` | No | *(generated UUID)* | `agent-mock` | Fixed UUID. Must be a valid UUIDv4 string if provided. |
| `MOCK_AGENT_API_KEY` | No | `agentmesh_mock_key` | `agent-mock` | API key presented during registration (`am_ak_*` supported). |
| `MOCK_TASK_DELAY_MS` | No | `1000` | `agent-mock` | Simulated execution delay per progress stage in milliseconds. |
| `AGY_AGENT_OWNER` | No | `AgyDev` | `agent-agy` | Human owner label displayed in the fleet dashboard. |
| `AGY_AGENT_ID` | No | *(generated UUID)* | `agent-agy` | Fixed UUID. Keeps registration row stable across restarts. |
| `AGY_AGENT_API_KEY` | No | `agentmesh_agy_key` | `agent-agy` | API key presented during registration (`am_ak_*` format recommended). |
| `AGY_BIN_PATH` | No | *(auto-detected)* | `agent-agy` | Explicit path to `agy` binary. Defaults to `~/.local/bin/agy` or `PATH`. |
| `AGY_MODEL` | No | *(agy default)* | `agent-agy` | Model flag passed directly to `agy` (`--model`). |
| `AGY_EFFORT` | No | `medium` | `agent-agy` | Effort flag passed to `agy`: `low`, `medium`, or `high` (`--effort`). |
| `AGY_WORKSPACE_DIR` | No | *(none)* | `agent-agy` | Fallback working directory; overridden per task by `TaskSpec.repo_path`. |
| `AGY_TIMEOUT_SECS` | No | `600` | `agent-agy` | Timeout in seconds before terminating an unresponsive `agy` process. |

---

## Quick Start

Experience a complete coordinated multi-agent workflow in under 2 minutes:

### 1. Start Backing Services
```bash
docker compose up -d
```

### 2. Start Coordinator TUI
```bash
cargo run --bin coordinator
```
*(On boot, the coordinator connects to PostgreSQL, runs SQL migrations automatically, creates JetStream streams, performs startup recovery, and launches background workers).*

### 3. Start Two Mock Workers (in separate terminals)
```bash
# Terminal 2: Alice
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock

# Terminal 3: Bob
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock
```

### 4. Create and Approve a Project
1. In the Coordinator TUI, press `i` to enter Project Input mode.
2. Enter a project name (e.g. `User Auth Service`), press `Tab`, type a description (e.g. `Implement JWT auth and database schema`), and press `Enter`.
3. The AI Planner decomposes the project into a dependency-ordered task graph.
4. On the **Plan Review** screen, inspect tasks using `↑`/`↓`. Press `Enter` to expand details and view dependencies.
5. Press `y` to approve tasks. If a critical overlap warning appears, press `a` to acknowledge, then `y`.
6. Watch the workers claim tasks, report progress (`TaskStarted` → `ProgressUpdate` → `Completed`), and update the TUI live.
7. Press `3` to view the Fleet Dashboard or `4` for Diagnostics. Press `q` to quit.

---

## Running the Coordinator

```bash
# Interactive TUI mode (normal development)
cargo run --bin coordinator

# Capture debug logs to file without corrupting TUI rendering
cargo run --bin coordinator 2> coordinator.log

# Run as a headless daemon (non-interactive, e.g. in CI or systemd)
cargo run --bin coordinator > /dev/null 2>&1
```

### TUI Keybindings

| Key | Context | Action |
| :---: | :--- | :--- |
| `Tab` | Global | Cycle forward through screens: Input → Review → Dashboard → Diagnostics. |
| `1`, `2`, `3`, `4` | Global | Jump directly to Screen 1 (Input), 2 (Review), 3 (Dashboard), or 4 (Diagnostics). |
| `r` | Global | Manually reload tasks, agents, and metrics from PostgreSQL. |
| `q` / `Ctrl+C` | Global | Cleanly shut down coordinator. |
| `i` / `Enter` | Project Input | Enter edit mode for project name/description. |
| `Tab` / `Esc` | Project Input | Switch between fields or cancel input. |
| `Enter` (when ready) | Project Input | Submit project description to AI planning service. |
| `↑` / `↓` or `k` / `j` | Plan Review | Navigate between tasks in the proposal. |
| `Enter` | Plan Review | Toggle details pane (shows full description, dependencies, overlap warnings). |
| `y` | Plan Review | **Approve** selected task. Dispatches immediately if agent available. |
| `n` | Plan Review | **Reject** selected task. |
| `e` | Plan Review | **Edit** task description inline. Pressing `Enter` saves **and approves**. |
| `a` | Plan Review / Dashboard | **Acknowledge** selected critical resource overlap warning. |
| `c` | Plan Review / Dashboard | **Cancel** selected task (frees agent, terminates delivery, cleans worktree). |
| `↑` / `↓` | Diagnostics | Scroll through the live coordinator event stream. |

---

## Running Mock Agents

`agent-mock` simulates execution without invoking third-party LLMs or external CLI tools:

```bash
# Start a basic mock worker
cargo run --bin agent-mock

# Start with custom owner and execution speed
MOCK_AGENT_OWNER="Backend Worker" MOCK_TASK_DELAY_MS=500 cargo run --bin agent-mock

# Pin a persistent agent ID across restarts
MOCK_AGENT_ID="11111111-1111-4111-8111-111111111111" cargo run --bin agent-mock
```

---

## Running AGY Agents

`agent-agy` bridges AgentMesh to the real Antigravity `agy` CLI:

```text
Coordinator ──► NATS JetStream ──► agent-agy ──► agy CLI subprocess ──► Git Worktree
```

### 1. Verify `agy` Installation
```bash
agy --version
```
Ensure that `agy` is authenticated and functional on your machine.

### 2. Launch `agent-agy`
```bash
NATS_URL="nats://localhost:4222" \
NATS_AUTH_TOKEN="agentmesh_dev_token" \
AGY_AGENT_OWNER="Dev Lead" \
AGY_AGENT_API_KEY="am_ak_$(openssl rand -hex 32)" \
AGY_EFFORT="medium" \
AGY_TIMEOUT_SECS=600 \
cargo run --bin agent-agy
```

### How the Adapter Works
1. **Host Discovery**: Scans local environment for languages (Rust, Python, Node, Go, C), tools (`git`, `docker`, `cargo`), and OS attributes.
2. **Registration**: Authenticates with coordinator via `coordinator.agents.register`, storing its capability profile in PostgreSQL.
3. **Heartbeat Loop**: Sends heartbeat pings every 5 seconds to `coordinator.agents.heartbeat.<agent_id>`.
4. **Task Execution**: Pulls `TaskAssignment` from JetStream, executes `agy -p "<prompt>" --output-format stream-json --dangerously-skip-permissions` inside the assigned Git worktree.
5. **Stream Parsing**: Translates `agy` NDJSON output (`step_update`, `result`) into `TaskStarted`, `ProgressUpdate`, `Completed`, or `Failed` events.
6. **Error Redaction**: Masks API keys, passwords, and private keys in failure messages before sending them to the coordinator.

---

## Multi-Machine Fleet Setup

Run the coordinator on Machine A and remote agents on Machine B (via LAN, WireGuard, or Tailscale):

```text
     Machine A (Coordinator Host)                       Machine B (Worker Host)
┌───────────────────────────────────────┐       ┌───────────────────────────────────────┐
│ Coordinator Runtime                   │       │ agent-agy or agent-mock               │
│ PostgreSQL 16 (:5432, local only)     │       │ agy CLI                               │
│ NATS 2.10 (:4222, exposed to network) │◄──────┼─ NATS TCP Connection                  │
└───────────────────────────────────────┘       └───────────────────────────────────────┘
```

### Step 1: Configure Machine A (Coordinator Host)
1. Find Machine A's network IP (e.g. `192.168.1.50` on LAN or `100.x.y.z` on Tailscale):
   ```bash
   ip addr show  # or: tailscale ip -4
   ```
2. Set a strong `NATS_AUTH_TOKEN` in `.env`:
   ```bash
   NATS_AUTH_TOKEN="sec_prod_token_9f823a"
   ```
3. Restart containers with the new token:
   ```bash
   docker compose up -d
   ```
4. Allow TCP traffic on port 4222 from Machine B's IP in your firewall:
   ```bash
   # Linux UFW example:
   sudo ufw allow from 192.168.1.0/24 to any port 4222 proto tcp
   ```
5. Launch the coordinator:
   ```bash
   cargo run --bin coordinator
   ```

### Step 2: Configure Machine B (Remote Worker)
1. On Machine B, install `agent-agy` (or `agent-mock`):
   ```bash
   git clone https://github.com/vijaygovindBiju/AgentMesh.git
   cd AgentMesh
   ./scripts/install.sh --prefix ~/.local/bin
   ```
2. Point the agent to Machine A:
   ```bash
   export NATS_URL="nats://192.168.1.50:4222"
   export NATS_AUTH_TOKEN="sec_prod_token_9f823a"
   export AGY_AGENT_OWNER="Remote Worker 1"
   export AGY_AGENT_API_KEY="am_ak_$(openssl rand -hex 32)"

   agent-agy
   ```

### Step 3: Verify Connection
On Machine A, inspect registered agents:
```bash
docker compose exec postgres psql -U agentmesh -d agentmesh \
  -c "SELECT id, human_owner, adapter_type, status, last_seen FROM agents;"
```
Or switch to Screen 3 (**Dashboard**) in the coordinator TUI.

> [!CAUTION]
> **Never expose port 4222 to the public internet without TLS and firewall filtering.** Always connect across an encrypted VPN (WireGuard/Tailscale) or an SSH tunnel.

---

## Real Repository Workflow

When coordinating work on an actual Git codebase, AgentMesh executes the following end-to-end lifecycle:

```text
 1. Human enters project description in TUI.
 2. RepositoryScanner analyzes root workspace (Cargo, npm, Python, Go, directory tree, README).
 3. AI Planner generates DAG with affected resources, dependencies, and capability tags.
 4. PlanValidator verifies graph is an acyclic DAG.
 5. Human operator reviews and approves tasks individually (y/n/e/a).
 6. Periodic Assignment Worker claims ready tasks whose dependencies are Completed.
 7. GitCoordinator creates isolated worktree (.agentmesh/worktrees/<short_id>) on agentmesh/<short_id> branch.
 8. Coordinator dispatches TaskAssignment with worktree repo_path via JetStream.
 9. Agent executes task inside the private worktree.
10. Agent streams TaskStarted, ProgressUpdate, Blocked, or Completed events.
11. On Completed, GitCoordinator commits uncommitted changes, verifies 3-way mergeability against base branch, and removes worktree.
12. Coordinator marks task Completed in PostgreSQL and emits coordinator.events.
13. Periodic worker detects dependent tasks are now unblocked and dispatches them automatically.
```

---

## Git Worktree & Isolation Model

To guarantee that agents never corrupt each other's files or collide on `.git/index.lock`, AgentMesh isolates every executing task in a dedicated Git worktree:

```text
<repository-root>/
├── .git/
├── .agentmesh/
│   └── worktrees/
│       ├── task-a1b2/    ◄── agentmesh/task-a1b2 branch (Agent 1)
│       └── task-c3d4/    ◄── agentmesh/task-c3d4 branch (Agent 2)
├── src/
└── Cargo.toml
```

- **Branch Naming**: Each task creates a sanitized branch: `agentmesh/<short_id>` (e.g. `agentmesh/plan-1`).
- **Worktree Directory**: Stored under `.agentmesh/worktrees/<short_id>` (automatically git-ignored).
- **Zero Collision**: Agents have completely private working trees and indices while sharing the parent repository object database.
- **Resource Auditing**: Upon completion, `ResourceTracker` compares files modified against the task's declared `affected_resources`. Unexpected modifications are logged to `unexpected_resource_changes`.
- **Mergeability Check**: Before closing a task, `CompletionManager` executes a 3-way `git merge-tree` simulation against the base branch. If merge conflicts exist, they are recorded in `git_conflicts` with failure diagnostics.
- **Automatic Cleanup**: Worktrees are safely removed upon task completion or cancellation.

---

## Human Approval & Replanning Gate

AgentMesh enforces a strict human-in-the-loop security boundary:

```text
                 AI Planner
                     │
                     ▼
          Deterministic DAG Validation
                     │
                     ▼
            TaskStatus::HumanReview
                     │
           ┌─────────┴─────────┐
           ▼                   ▼
    Human Approves (y)   Human Rejects (n)
           │                   │
           ▼                   ▼
      Task Approved      Task Rejected
           │
           ▼
    Agent Assignment
```

### Replanning Safety Invariant
When a task fails or a merge conflict arises, `ReplanEngine` gathers context (completed work, error messages, conflict diffs) and prompts the LLM for corrective remediation tasks.
- **Strict Invariant**: All replanned tasks are placed into `TaskStatus::HumanReview`.
- **Zero Auto-Execution**: No replanned task can bypass human review. The operator must explicitly review and approve replanned tasks in the TUI.

---

## Failure & Recovery Behavior

AgentMesh handles runtime failures gracefully:

| Failure Scenario | Coordinator Runtime Behavior |
| :--- | :--- |
| **Agent Process Crash** | Heartbeat monitor marks agent `offline` after 30s of silence. |
| **Stale Task Sweeper** | Background loop sweeps orphaned tasks every 5s, unassigns worker, increments attempt count, and reclaims task to `Approved`. |
| **Repeated Failures (≥ 3)** | After 3 failed delivery attempts, task is quarantined to `TaskStatus::HumanReview` for operator investigation. |
| **Coordinator Restart** | `CoordinatorRecoveryService` runs on boot, reconciling pending deliveries and sweeping stale tasks before accepting events. |
| **Delivery Timeout** | Unacknowledged deliveries expire after 60s (`task_deliveries.expires_at`) and are marked `Terminal`. |
| **Task Blocked on Prerequisite** | Coordinator sends `WaitForDependency`; automatically unblocks task to `Approved` when prerequisite reports `Completed`. |
| **Task Cancellation** | Operator `c` command marks delivery `Terminal`, frees agent to `Idle`, cleans up worktree, and notifies worker via `TaskCancelled`. |
| **Git Merge Conflict** | Conflict detector flags colliding branches, logs details in `git_conflicts`, and provides remediation advice. |
| **Upstream 429 Quota Exhaustion** | Adapter captures `RESOURCE_EXHAUSTED` error, preserves exit code, redacts secrets, and reports failure cleanly without crashing. |

---

## Security Model & Boundaries

AgentMesh implements defense-in-depth across authentication, permissions, and credentials:

- **Authentication**: Agents present an API key (`am_ak_*`). The coordinator hashes keys using SHA-256 and validates them via constant-time comparison (`subtle::ConstantTimeEq`).
- **Authorization & Roles**: `AgentRole` (`Worker`, `Reviewer`, `ReadOnly`, `Admin`) restricts available commands.
- **Permission Boundaries**: `PermissionBoundary` configures allowed and denied glob path patterns (e.g. denying `.env`, `.git/*`, production credentials), enforced before dispatch.
- **Task Impersonation Prevention**: `TaskAuthorizer` verifies that incoming lifecycle events originate solely from the agent assigned to that task; unauthorized events are rejected and audit-logged.
- **Secret Redaction**: `SecretRedactor` masks API keys (`am_ak_*`, `sk-ant-*`, `sk-*`), connection string passwords (`postgres://user:pass@host`), and private key blocks across all logs and payloads.
- **Audit Logging**: Sensitive operations (registrations, revocations, impersonation attempts, permission violations) are permanently written to PostgreSQL in `audit_logs`.

> [!NOTE]
> **Development Status**: In the default development setup, NATS token authentication is shared (`NATS_AUTH_TOKEN`) and registration auto-approves valid keys. For production environments, utilize TLS client certificates, network isolation, and rotated `am_ak_*` keys.

---

## Observability & Diagnostics

AgentMesh provides deep runtime visibility across the entire fleet:

### Structured Coordinator Events
All major coordination events are recorded in the `coordinator_events` table and streamed live to the TUI Diagnostics screen:
- `task.started` — Worker acknowledged assignment and commenced execution.
- `task.progress` — Milestone progress percentage and status update.
- `task.blocked` — Task execution blocked on prerequisite or external dependency.
- `task.completed` — Task completed successfully with audited Git commit.
- `task.failed` — Task execution failed (retaining exit code and redacted error).
- `task.cancelled` — Operator cancelled task; worktree pruned.
- `task.unblocked` — Prerequisite completed; dependent task restored to `Approved`.
- `task.reclaimed` — Stale task reclaimed from offline agent for reassignment.
- `project.replanned` — Corrective proposal generated following failure or conflict.

### Useful SQL Inspection Queries
Run queries directly against the PostgreSQL container:
```bash
# Inspect task status and worktree branches
docker compose exec postgres psql -U agentmesh -d agentmesh \
  -c "SELECT short_id, status, assigned_agent_id, task_branch FROM tasks ORDER BY created_at;"

# Inspect active fleet health and last seen timestamps
docker compose exec postgres psql -U agentmesh -d agentmesh \
  -c "SELECT id, human_owner, adapter_type, status, health_status, last_seen FROM agents;"

# Inspect coordinator event stream
docker compose exec postgres psql -U agentmesh -d agentmesh \
  -c "SELECT event_type, task_id, message, created_at FROM coordinator_events ORDER BY created_at DESC LIMIT 15;"

# Inspect security audit trail
docker compose exec postgres psql -U agentmesh -d agentmesh \
  -c "SELECT timestamp, actor_id, action, status, details FROM audit_logs ORDER BY timestamp DESC LIMIT 10;"
```

---

## Testing & Quality Assurance

AgentMesh maintains rigorous test coverage with **207 passing automated tests** across the entire workspace:

```text
test result: ok. 207 passed; 0 failed; 0 ignored
```

### Test Suite Breakdown

| Crate / Integration Suite | Tests | Purpose & Verification Scope |
| :--- | :---: | :--- |
| **`agent-protocol`** | 13 | Serde serialization round-trips, message schemas, capability models, health state transitions, glob matching, and secret redaction. |
| **`agent-mock`** | 4 | Registration flow, simulated delay handling, heartbeat loop, and JetStream message consumption. |
| **`agent-agy`** | 10 | NDJSON stream parser, subprocess lifecycle supervision, timeout management, dynamic binary lookup, and error propagation. |
| **`coordinator (lib)`** | 101 | Unit tests for domain models, state machine transitions, SQL repositories, DAG acyclicity validation, capability matching, and overlap detection. |
| **`phase2_integration`** | 3 | JetStream durable consumers, redelivery on NAK, and mock lifecycle event delivery. |
| **`phase3_planning`** | 3 | AI planning service workflows, provider error handling, and DAG cycle rejection. |
| **`phase4_tui`** | 2 | Headless TUI rendering across all modes, keybindings, and action emission. |
| **`phase5_coordinator`** | 6 | Approval gating, dependency tracking, concurrency locking, and human reassignment. |
| **`phase6_dashboard_overlap`** | 5 | Resource overlap detection, warning banners, and real-time TUI dashboard event forwarding. |
| **`phase7_e2e_demo`** | 1 | Complete end-to-end integration demo with multiple mock agents. |
| **`phase8_agy_integration`** | 6 | AGY adapter registration, execution lifecycle, timeout/crash recovery, parallel execution, and quota handling. |
| **`phase9_git_integration`** | 6 | Git repository discovery, worktree isolation, unexpected change detection, 3-way merge conflict detection, and parallel JetStream workflows. |
| **`phase10_intelligent_planning`** | 6 | Repository scanner, architecture discovery, complexity estimation, and dynamic replanning context generation. |
| **`phase11_capabilities`** | 6 | Host environment detection, capability profiling, availability gating, and score-based matching. |
| **`phase12_security`** | 8 | Constant-time API key hashing, role boundaries, denied path globs, task impersonation prevention, secret redaction, and audit logging. |
| **`phase13_observability`** | 7 | Execution timelines, delivery diagnostics, automated failure remediation advice, and system KPI metrics. |
| **`phase14_reliability`** | 8 | Startup crash recovery, agent reconnect discovery, stale task sweeping, duplicate event deduplication, and network partitions. |
| **`phase15_v1_validation`** | 10 | Full v1.0 end-to-end multi-agent validation suite across physical machine boundaries and parallel execution. |
| **`phase16_runtime_integration`** | 10 | Comprehensive runtime integration test suite verifying that all subsystems are wired into the active coordinator binary. |

### Running the Tests

```bash
# Run the complete workspace test suite
cargo test --workspace --no-fail-fast

# Run Phase 16 Runtime Integration suite specifically
cargo test -p coordinator --test phase16_runtime_integration

# Run clippy checks with strict warning enforcement
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Verify formatting
cargo fmt --all -- --check
```

---

## Known External Dependencies

### Upstream LLM Quota Limits (HTTP 429)
When executing real `agy` CLI binary instances against external frontier models (e.g. Gemini, Claude), tasks may encounter upstream HTTP 429 rate limits:
```text
RESOURCE_EXHAUSTED (code 429): Individual quota reached
```
- **AgentMesh Handling**: `agent-agy` explicitly preserves the subprocess exit code, captures the error string, redacts any sensitive tokens, and reports a clean `TaskStatus::Failed` event to the coordinator.
- **Reporting**: AgentMesh surfaces upstream quota limits honestly rather than misrepresenting them as an internal coordination or protocol failure.

---

## Project Structure

```text
AgentMesh/
├── Cargo.toml                       # Workspace definition & shared dependency versions
├── Cargo.lock                       # Dependency lockfile
├── docker-compose.yml               # Backing PostgreSQL 16 & NATS 2.10 services
├── .env.example                     # Documented configuration template
├── README.md                        # Master project documentation
├── TODO.md                          # Authoritative development tracking & state
├── install.sh                       # Quick-bootstrap development helper
├── scripts/
│   ├── install.sh                   # Production & local binary installer script
│   └── test_two_agy_instances.sh    # Multi-agent verification script
├── migrations/                      # Compile-time embedded PostgreSQL SQL migrations
│   ├── 001_initial_schema.sql
│   ├── 002_git_coordination.sql
│   ├── 003_agent_capabilities_and_health.sql
│   ├── 004_security_and_audit.sql
│   └── 005_observability_and_events.sql
├── docs/                            # In-depth architectural & protocol documentation
│   ├── README.md                    # Documentation index
│   ├── architecture/                # Architecture overview & domain models
│   ├── protocols/                   # Wire protocol schemas & NATS subject maps
│   ├── deployment/                  # Multi-machine setup & network guides
│   ├── development/                 # Phase validation & testing reports
│   └── decisions/                   # Architecture Decision Records (ADRs 001–012)
└── crates/
    ├── agent-protocol/              # Shared data types, message enums, security & capabilities
    │   └── src/
    ├── agent-mock/                  # Simulated test agent binary
    │   └── src/
    ├── agent-agy/                   # Antigravity agy CLI adapter library & binary
    │   └── src/
    └── coordinator/                 # Central coordinator orchestration engine & binary
        ├── src/
        │   ├── ai/                  # LLM providers, DAG validator, repo scanner, replanner
        │   ├── coordinator/         # State machine, assignment service, commands, engine
        │   ├── db/                  # SQLx connection pool & entity repositories
        │   ├── domain/              # Core domain entities (Task, Agent, Project, Delivery)
        │   ├── git/                 # Worktree manager, branch strategy, conflict detector
        │   ├── messaging/           # NATS JetStream client, publisher, subscriber
        │   ├── observability/       # Events, execution timelines, failure diagnostics, metrics
        │   ├── reliability/         # Startup recovery, stale task sweeper, deduplication
        │   ├── security/            # API key auth, permission boundaries, secret redactor
        │   └── tui/                 # Ratatui screens (Input, Review, Dashboard, Diagnostics)
        └── tests/                   # Phase 2 through Phase 16 integration test suites
```

---

## Documentation Index

For in-depth architectural specifications and deployment walkthroughs, consult the [`docs/`](docs/) directory:

- [`docs/README.md`](docs/README.md) — Master index of all project documentation.
- [`docs/architecture/architecture.md`](docs/architecture/architecture.md) — Detailed coordinator engine architecture and subsystem design.
- [`docs/architecture/domain-model.md`](docs/architecture/domain-model.md) — Entity relationship models, state machine transitions, and invariants.
- [`docs/protocols/agent-protocol.md`](docs/protocols/agent-protocol.md) — Complete NATS subjects, JetStream stream configurations, and message schemas.
- [`docs/deployment/multi-machine.md`](docs/deployment/multi-machine.md) — Step-by-step walkthrough for deploying across physical networks and VPNs.
- [`docs/development/v1-validation.md`](docs/development/v1-validation.md) — Phase 15 & 16 validation report and verification matrix.
- [`docs/decisions/`](docs/decisions/) — Architecture Decision Records (ADRs 001–012).

---

## Troubleshooting

### PostgreSQL Connection Failures
- **Symptom**: Coordinator logs `Failed to connect to PostgreSQL database` and enters Standalone Demo Mode.
- **Resolution**: Check `docker compose ps` to ensure the container is healthy. Verify `DATABASE_URL` in `.env`. If you changed the password in `.env` after the first launch, clear the volume: `docker compose down -v && docker compose up -d`.

### NATS Connection Failures
- **Symptom**: `Failed to connect to NATS at nats://localhost:4222`.
- **Resolution**: Verify port 4222 is listening via `curl -I http://localhost:8222/healthz`. Ensure `NATS_AUTH_TOKEN` in `.env` matches the token provided to the NATS container.

### `agy` Command Not Found
- **Symptom**: `Failed to spawn agy process: No such file or directory`.
- **Resolution**: Ensure `agy` is installed. Set `export AGY_BIN_PATH="$(which agy)"` or place the binary in `~/.local/bin/agy`.

### Tasks Stuck in `Approved` Status
- **Symptom**: Tasks are approved in the TUI but do not transition to `Assigned`.
- **Resolution**:
  1. Check if a `Blocks` dependency is still incomplete. Dependent tasks will remain held until the blocker reaches `Completed`.
  2. Ensure at least one registered agent is `Idle` and healthy. Press `3` to inspect agent availability in the Fleet Dashboard.

### Stale Git Worktrees
- **Symptom**: Git reports `fatal: '.agentmesh/worktrees/<id>' already exists`.
- **Resolution**: Clean up stale worktree records manually using `git worktree prune`, or remove `.agentmesh/worktrees/`. AgentMesh also force-prunes conflicting paths on assignment retry.

---

## Contributing & License

AgentMesh is an open-source systems project designed for reliable, scalable multi-agent coding coordination.

### Contributing
1. Fork the repository and create a feature branch (`feat/your-feature`).
2. Adhere to the established architecture principles: **authoritative PostgreSQL storage, strict human approval gate, and isolated Git worktrees**.
3. Ensure all tests pass: `cargo test --workspace --no-fail-fast`.
4. Ensure clippy passes cleanly: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
5. Ensure formatting is verified: `cargo fmt --all -- --check`.
6. Submit a pull request with a clear explanation of what and why.

### License
AgentMesh is distributed under the open-source **MIT License**.
See the package manifests in [`Cargo.toml`](Cargo.toml) for details.
