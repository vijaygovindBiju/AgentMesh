# AgentMesh

> AI suggests → Humans decide → Agents execute

AgentMesh is an AI project coordinator for parallel coding agents. It sits above your repository workflow to decompose software projects, detect resource overlaps, propose dependency-ordered task plans across multiple AI coding agents, and enforce a strict human approval gate before any code executes.

---

> ### 📑 Navigation & Quick Index
>
> | 🚀 Getting Started | 📦 Installation & Setup | 🎮 How to Use | ⚡ Command Cheat Sheet | 🏗️ Architecture & Internals |
> | :--- | :--- | :--- | :--- | :--- |
> | • [Overview](#agentmesh)<br>• [Problem Solved](#the-problem-agentmesh-solves)<br>• [What You Need](#what-you-need-prerequisites--system-checklist)<br>• [Quick Start](#quick-start)<br>• [Features](#features) | • [**How to Install**](#how-to-install)<br>&nbsp;&nbsp;▫ [Automated Install (`install.sh`)](#option-a-automated-installation-recommended)<br>&nbsp;&nbsp;▫ [Manual Installation](#option-b-manual-step-by-step-installation)<br>&nbsp;&nbsp;▫ [Clone Repository](#2-clone-repository)<br>&nbsp;&nbsp;▫ [Environment Config](#3-environment-configuration)<br>&nbsp;&nbsp;▫ [Start Infrastructure](#4-start-infrastructure-services)<br>&nbsp;&nbsp;▫ [Build from Source](#5-build-binaries-from-source)<br>&nbsp;&nbsp;▫ [Verify Installation](#6-verify-installation) | • [**How to Use**](#how-to-use)<br>&nbsp;&nbsp;▫ [Operating Modes](#operating-modes)<br>&nbsp;&nbsp;▫ [Launch Coordinator](#step-1-launch-the-coordinator)<br>&nbsp;&nbsp;▫ [Launch Mock Agents](#step-2-launch-mock-agent-workers)<br>&nbsp;&nbsp;▫ [TUI Walkthrough](#step-3-step-by-step-tui-walkthrough)<br>&nbsp;&nbsp;▫ [4-Terminal Walkthrough](#step-4-complete-local-test-walkthrough)<br>&nbsp;&nbsp;▫ [TUI Keybindings](#step-5-tui-keyboard-controls)<br>&nbsp;&nbsp;▫ [AI Provider Setup](#step-6-ai-provider-configuration) | • [**Complete Command Reference**](#complete-command-reference--cheat-sheet)<br>&nbsp;&nbsp;▫ [Setup & Install Commands](#1-setup--installation-commands)<br>&nbsp;&nbsp;▫ [Docker & Service Commands](#2-infrastructure--docker-commands)<br>&nbsp;&nbsp;▫ [Coordinator Commands](#3-coordinator-execution-commands)<br>&nbsp;&nbsp;▫ [Agent Worker Commands](#4-agent-worker-execution-commands)<br>&nbsp;&nbsp;▫ [Inspection Commands](#5-inspection--health-check-commands)<br>&nbsp;&nbsp;▫ [Testing Commands](#6-testing--verification-commands) | • [Architecture](#architecture)<br>• [Core Workflow](#core-workflow)<br>• [Project Structure](#project-structure)<br>• [Task State Machine](#task-state-machine)<br>• [DAG Validation](#dependencies--dag-validation)<br>• [Overlap Detection](#overlap-detection--gating)<br>• [Agent Protocol](#agent-registration--protocol)<br>• [Remote Deployment](#multi-device--remote-agent-setup)<br>• [Troubleshooting](#troubleshooting) |

---

## The Problem AgentMesh Solves

When multiple developers deploy AI coding agents on the same codebase simultaneously, coordination breaks down:

- **Resource Conflicts:** Multiple agents modify shared models, schema migrations, or interfaces concurrently, leading to merge collisions and architectural drift.
- **Dependency Inversion:** Downstream tasks are executed before prerequisite schema migrations or core libraries are completed.
- **Lack of Verification:** Unchecked AI agents make autonomous code modifications without human sign-off.
- **Fragmented Visibility:** Human leads have no single pane of glass to observe fleet health, task progress, and blocking issues across remote agent runtimes.

AgentMesh solves this by treating the AI as an advisor rather than an autonomous actor. Authoritative state is locked in PostgreSQL, task delivery is guaranteed via NATS JetStream, and execution is gated behind human review.

---

## What You Need (Prerequisites & System Checklist)

> 💡 **Core Concepts in 30 Seconds:**
> - **Coordinator:** The central control center. It takes your project ideas, asks AI to break them into tasks, checks for file conflicts, and renders the interactive terminal UI.
> - **Human Approval Gate:** The safety shield. AI proposes tasks, but **you** approve, edit, or reject them. No agent can run code without your explicit sign-off.
> - **Agent Workers:** The worker programs (e.g., Alice and Bob) that receive approved tasks and simulate or execute code.
> - **NATS JetStream:** A super-fast messaging system that delivers tasks to agents reliably, even across multiple machines or during network reconnections.
> - **PostgreSQL 16:** The database that stores all projects, tasks, approval records, and execution logs permanently on disk.

Before installing and running AgentMesh, verify that you have the following requirements:

### 1. System Requirements & Software
| Component | Minimum Version | Required For | How to Install / Verify |
| :--- | :--- | :--- | :--- |
| **Operating System** | Linux, macOS, or Windows WSL2 | All components | Standard ANSI / UTF-8 compatible terminal |
| **Rust & Cargo** | Stable **1.75+** (2021 edition) | Building Coordinator & Agents | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| **Docker Engine** | **v20.10+** | Running PostgreSQL 16 & NATS | Verify with `docker --version` |
| **Docker Compose** | **v2.0+** (Plugin or Standalone) | Starting backing services | Verify with `docker compose version` |
| **Git** | Any modern version | Cloning and tracking codebase | Verify with `git --version` |
| **Curl** | Any modern version | Downloading toolchains & health checks | Verify with `curl --version` |

### 2. Network Ports
AgentMesh uses the following local ports (configurable via `.env`):
- **`5432`**: PostgreSQL 16 database (relational state, transactional locks).
- **`4222`**: NATS client protocol (pub/sub, request-reply registration).
- **`8222`**: NATS HTTP monitoring and healthcheck endpoint (`/healthz`).

### 3. API Keys & Credentials
- **Local Testing / Mock Mode (Default):** **No API keys or internet connection required!** AgentMesh defaults to `AI_PROVIDER=mock`, which uses an internal deterministic task planning and DAG generation engine. You can run and test everything completely free.
- **Anthropic Claude (Optional):** If you want real AI planning with Claude 3.5 Sonnet, provide an Anthropic API key (`ANTHROPIC_API_KEY=sk-ant-...`) with `AI_PROVIDER=anthropic` in your `.env`.

---

## Quick Start

For developers who want to get the stack running immediately:

```bash
# ------------------------------------------------------------------------------
# 1. Clone repository and navigate into the project directory
# ------------------------------------------------------------------------------
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

# ------------------------------------------------------------------------------
# 2. Copy the example configuration to create your local .env
#    (Default settings work out-of-the-box with free local mock AI planning)
# ------------------------------------------------------------------------------
cp .env.example .env

# ------------------------------------------------------------------------------
# 3. Start PostgreSQL 16 and NATS 2.10 JetStream in the background
#    (-d runs containers in detached mode)
# ------------------------------------------------------------------------------
docker compose up -d

# ------------------------------------------------------------------------------
# 4. Launch the Coordinator TUI
#    (Automatically creates database tables and starts background listeners)
# ------------------------------------------------------------------------------
cargo run --bin coordinator

# ------------------------------------------------------------------------------
# 5. In separate terminal windows, start worker agents:
#    Terminal 2 (Backend specialist):
# ------------------------------------------------------------------------------
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock

#    Terminal 3 (Infrastructure specialist):
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock
```

---

## How to Install

You can install and set up AgentMesh either automatically using the included setup script or manually step-by-step.

### Option A: Automated Installation (Recommended)

AgentMesh includes an automated installation script [`install.sh`](install.sh) that handles the entire setup in one go:
1. Validates system dependencies (Git, Curl, Rust, Cargo, Docker).
2. Generates your local `.env` configuration file if missing.
3. Starts the PostgreSQL 16 and NATS JetStream Docker containers.
4. Waits for container health checks to report healthy.
5. Compiles the workspace binaries (`coordinator` and `agent-mock`).

```bash
# Step 1: Make the script executable
chmod +x install.sh

# Step 2: Run the automated setup
./install.sh
```

#### Automated Script Options:
```bash
# Build optimized release binaries for faster execution:
./install.sh --release

# Build binaries only (skip starting Docker containers if already running):
./install.sh --skip-docker

# Verify prerequisites and start Docker services without compiling Rust code:
./install.sh --skip-build

# Non-interactive mode (automatically accepts all prompts with defaults):
./install.sh -y

# Display all available options and help:
./install.sh --help
```

---

### Option B: Manual Step-by-Step Installation

If you prefer to configure your environment step-by-step, follow these instructions:

#### 1. Install System Dependencies

<details>
<summary><b>Click to expand instructions for your operating system</b></summary>

##### Ubuntu / Debian
```bash
# 1. Update package repository index
sudo apt update

# 2. Install essential build tools, git, curl, and OpenSSL libraries
sudo apt install -y curl git build-essential pkg-config libssl-dev

# 3. Install the Rust compiler and Cargo toolchain via Rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# 4. Configure your current shell to load Cargo into PATH
source "$HOME/.cargo/env"
```

##### macOS (Homebrew)
```bash
# 1. Install git and curl using Homebrew
brew install curl git

# 2. Install Rust toolchain via Rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# 3. Configure your current shell
source "$HOME/.cargo/env"
```

##### Arch Linux
```bash
# 1. Install base build packages, curl, git, rustup, and docker
sudo pacman -S --needed base-devel curl git rustup docker docker-compose

# 2. Set default Rust toolchain to stable
rustup default stable
```

##### Windows (WSL2 Ubuntu)
Inside your WSL2 terminal, follow the Ubuntu instructions above. Ensure Docker Desktop for Windows has WSL2 integration enabled (*Settings → Resources → WSL Integration*).

</details>

#### 2. Clone Repository

```bash
# Clone the repository from GitHub
git clone https://github.com/vijaygovindBiju/AgentMesh.git

# Enter the project folder
cd AgentMesh
```

#### 3. Environment Configuration

Copy the template environment file to create your local `.env`:

```bash
# Copy template to .env
cp .env.example .env
```

> [!TIP]
> **No changes required for local testing!** The default `.env` is already configured to use the local PostgreSQL database, local NATS server, and the built-in deterministic AI mock planner.

##### Environment Variables Reference

If you want to customize your setup, edit `.env`:

| Variable | Default Value | Description / Purpose |
| :--- | :--- | :--- |
| `DATABASE_URL` | `postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh` | PostgreSQL connection string for coordinator state |
| `POSTGRES_PASSWORD` | `agentmesh_dev` | Superuser password used by Docker Compose |
| `NATS_URL` | `nats://localhost:4222` | NATS message broker connection URL |
| `NATS_AUTH_TOKEN` | `agentmesh_dev_token` | Client authentication token for NATS connections |
| `AI_PROVIDER` | `mock` | Active planning provider: `mock` (deterministic, free) or `anthropic` |
| `AI_MODEL` | `claude-3-5-sonnet-20241022` | Model identifier when `AI_PROVIDER=anthropic` |
| `ANTHROPIC_API_KEY` | *(empty)* | Anthropic API key required only when `AI_PROVIDER=anthropic` |
| `RUST_LOG` | `agentmesh=debug,coordinator=debug,info` | Tracing log filter directive (written to stderr) |
| `MOCK_TASK_DELAY_MS` | `1000` | Delay in ms between progress stages in `agent-mock` |
| `MOCK_AGENT_OWNER` | `MockDev` | Default human owner label for mock agent worker |
| `MOCK_AGENT_ID` | *(auto-generated UUID)* | Explicit UUID to assign to the mock agent |
| `MOCK_AGENT_API_KEY` | `agentmesh_mock_key` | Secret key sent by mock agent during registration |

> [!IMPORTANT]
> The `.env` file is excluded from version control via [`.gitignore`](.gitignore). Never commit secret API keys or production database passwords.

#### 4. Start Infrastructure Services

Start the PostgreSQL 16 database and NATS 2.10 JetStream message broker containers:

```bash
# Start background services (-d = detached / background mode)
docker compose up -d
```

##### Verify Service Health
Confirm that both containers are running and report healthy status:

```bash
# Check container status
docker compose ps
```

Expected output:
```text
NAME                   IMAGE                COMMAND                  SERVICE    STATUS
agentmesh-nats-1       nats:2.10-alpine     "/nats-server -js -a…"   nats       Up (healthy)
agentmesh-postgres-1   postgres:16-alpine   "docker-entrypoint.s…"   postgres   Up (healthy)
```

You can test connectivity to the endpoints directly:
```bash
# Check NATS HTTP monitoring server (should return HTTP 200 OK)
curl -i http://localhost:8222/healthz

# Check PostgreSQL port (should report Connection to localhost 5432 port succeeded)
nc -zv localhost 5432
```

> [!TIP]
> If you already run PostgreSQL and NATS natively on your machine, you do not need Docker. Simply update `DATABASE_URL` and `NATS_URL` in `.env` to point to your existing instances.

#### 5. Build Binaries from Source

Compile the Rust workspace crates:

```bash
# Fast debug build (ideal for development and testing)
cargo build --workspace

# Optimized release build (faster execution for production or benchmarking)
cargo build --release --workspace
```

Binaries will be placed in `target/debug/` (or `target/release/`):
- `coordinator`: Primary coordination daemon and interactive Ratatui TUI.
- `agent-mock`: Standalone simulated agent worker for local demonstrations and testing.

*(Optional)* Install binaries into your `$HOME/.cargo/bin` path to run them from any directory:
```bash
cargo install --path crates/coordinator
cargo install --path crates/agent-mock
```

#### 6. Verify Installation

Run the complete test suite to verify that all components compile and pass all domain invariants:

```bash
# Run all unit, repository, and phase integration tests
cargo test --workspace
```

All 119 tests across `agent-protocol`, `agent-mock`, `coordinator`, and all phase integration suites will execute and pass.

---

## How to Use

AgentMesh provides a keyboard-driven terminal user interface (TUI) backed by an asynchronous coordination engine. This section guides you through operating modes, launching the fleet, reviewing AI plans, and observing real-time task execution.

### Operating Modes

The Coordinator binary automatically adapts to the environment it detects:

1. **Full-Stack Mode (Default & Recommended):**
   - Active when PostgreSQL and NATS JetStream are running (`docker compose up -d`).
   - Automatically executes database migrations (`migrations/`).
   - Automatically provisions JetStream streams (`TASK_ASSIGNMENTS`, `AGENT_EVENTS`).
   - Launches background registration listeners, heartbeat monitors (with 30s offline detection), and telemetry ingestion loops.
   - Saves all project definitions, task plans, approvals, and execution logs persistently in PostgreSQL.

2. **Standalone Demo / Sandbox Mode (Zero-Infrastructure):**
   - Automatically triggered if PostgreSQL or NATS are offline or unreachable.
   - Loads a self-contained in-memory demo dataset.
   - Allows immediate exploration of the Ratatui TUI, screen navigation, keybindings, and task review workflow without running Docker or databases.

---

### Step 1: Launch the Coordinator

In your primary terminal, launch the Coordinator:

```bash
# Launch interactive TUI coordinator
cargo run --bin coordinator

# Or if you built with --release:
./target/release/coordinator
```

On startup in an interactive terminal, the Ratatui TUI opens immediately on **Screen 1: Project Input**.

> [!NOTE]
> Database migrations and stream creation run automatically on startup. There is no need to run separate database migration commands.

---

### Step 2: Launch Mock Agent Workers

To execute tasks and stream telemetry, launch one or more agent worker instances in separate terminal windows. Each agent acts as an independent developer worker in your fleet:

```bash
# ------------------------------------------------------------------------------
# Terminal 2: Start Mock Agent Alice (Backend specialist)
# ------------------------------------------------------------------------------
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock

# ------------------------------------------------------------------------------
# Terminal 3: Start Mock Agent Bob (Infrastructure specialist)
# ------------------------------------------------------------------------------
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock
```

#### What Happens When an Agent Starts:
1. **Registration:** Connects to NATS on `coordinator.agents.register` and registers its ID, human owner, capabilities, and adapter type.
2. **Heartbeat Loop:** Spawns a background task publishing a heartbeat to `coordinator.agents.heartbeat.{agent_id}` every 5 seconds.
3. **Consumer Creation:** Sets up a durable pull consumer on JetStream listening for task assignments on `tasks.{agent_id}.assigned`.
4. **Execution Simulation:** Upon receiving an assignment, the agent sends transport ACKs, publishes `TaskStarted`, emits periodic `ProgressUpdate` (50%), and reports `Completed` when finished.

---

### Step 3: Step-by-Step TUI Walkthrough

The Ratatui TUI consists of three primary screens:

#### Screen 1: Project Initiation (`ProjectInput`)
*Where you describe what software feature or project you want built.*

1. Press `[ i ]` or `[ Enter ]` to enter text input mode (`EnteringProject`).
2. Type a **Project Name** (e.g., `Order Ingress Engine`).
3. Press `[ Tab ]` to navigate to the **Project Description** field.
4. Type a natural language project description, for example:
   ```text
   Build order ingestion pipeline, database schema migration for orders and items, Stripe payment webhooks, and idempotent delivery tracking.
   ```
5. Press `[ Enter ]` to submit the project for AI planning.
6. The coordinator submits the request to the configured `LlmProvider` (`mock` or `anthropic`), stores the generated DAG in PostgreSQL, computes resource overlap reachability, and automatically transitions to **Screen 2**.

#### Screen 2: Interactive Plan Review (`PlanReview`)
*Where the Human Approval Gate enforces developer oversight before code executes.*

AI plans cannot execute until a human operator inspects and approves them.
1. Use `[ ↑ ]` / `[ ↓ ]` (or `[ k ]` / `[ j ]`) to navigate through proposed tasks.
2. Press `[ Enter ]` to toggle the detailed task panel, viewing:
   - **Target Assignee & Required Capabilities** (e.g., Alice for backend, Bob for infra)
   - **Affected File & API Resources** (e.g., `src/db/orders.rs`)
   - **Dependency Relationships** (`Blocks`, `RelatesTo`)
3. **Edit Task Descriptions:** Press `[ e ]` to open an inline editor buffer. Modify the instructions, then press `[ Enter ]` to confirm and approve, or `[ Esc ]` to cancel.
4. **Resolve Overlap Warnings:** If tasks concurrently touch the same resource, an `OverlapWarning` banner appears.
   - Attempting to approve a task with an unacknowledged critical overlap displays:
     ```text
     APPROVAL BLOCKED: Critical conflict on [src/db/orders.rs]. Press [a] to acknowledge first.
     ```
   - Press `[ a ]` to acknowledge the conflict and unblock approval.
5. **Approve or Reject Tasks:**
   - Press `[ y ]` to **Approve** the selected task.
   - Press `[ n ]` to **Reject** the task (marks it Rejected and stops execution).
6. As soon as a task is approved and its blocking dependencies are satisfied, the coordinator assignment engine immediately assigns it to an available agent over NATS JetStream!

#### Screen 3: Live Fleet Dashboard (`Dashboard`)
*Where you monitor real-time agent execution, telemetry, and progress.*

Press `[ Tab ]` or `[ 3 ]` to switch to the Live Dashboard:
1. **Fleet Status Table:** View all registered agents, their human owners, current status (`Idle`, `Busy`, `Offline`), and currently executing task.
2. **Active Overlap Warnings:** Scroll through system-wide overlap warnings with `[ ↑ ]` / `[ ↓ ]` and acknowledge them with `[ a ]` or `[ Enter ]`.
3. **Execution Telemetry Stream:** Monitor live progress percentages (0% → 50% → 100%) and state transitions emitted by agents.
4. **Automatic Dependency Resolution:** When Agent A completes Task 1, the dependency engine unblocks dependent downstream tasks (e.g., Task 3) and automatically dispatches them to the next idle agent.
5. **Completion:** When all approved tasks finish, the coordinator status advances to `Done`.

---

### Step 4: Complete Local Test Walkthrough

To experience the full lifecycle, open 4 terminal windows side-by-side:

```text
┌─────────────────────────────┐  ┌─────────────────────────────┐
│         TERMINAL 1          │  │         TERMINAL 2          │
│      docker compose up      │  │   cargo run coordinator     │
│   (PostgreSQL 16 + NATS)    │  │       (Ratatui TUI)         │
└─────────────────────────────┘  └─────────────────────────────┘
┌─────────────────────────────┐  ┌─────────────────────────────┐
│         TERMINAL 3          │  │         TERMINAL 4          │
│    Mock Agent A (Alice)     │  │     Mock Agent B (Bob)      │
│  MOCK_AGENT_OWNER="Alice"   │  │   MOCK_AGENT_OWNER="Bob"    │
└─────────────────────────────┘  └─────────────────────────────┘
```

1. **Terminal 1:** Run `docker compose up -d`. Ensure `docker compose ps` shows both services healthy.
2. **Terminal 3 & 4:** Start Alice and Bob in separate terminals using the commands in Step 2. Watch their registration logs confirming connection.
3. **Terminal 2:** Run `cargo run --bin coordinator`.
4. **Input Project:** Press `[ i ]`, type `Order Ingress Engine`, press `[ Tab ]`, type `Build order processing pipeline`, and press `[ Enter ]`.
5. **Inspect & Acknowledge Conflicts:** In Screen 2, inspect Task 1 and Task 2. If a critical conflict is flagged on a shared file, press `[ a ]` to acknowledge.
6. **Approve Plan:** Press `[ y ]` on Task 1. Watch Alice immediately pick up Task 1 in Terminal 3!
7. **Observe Execution:** Watch the live progress update to 50% in the TUI, then complete.
8. **Observe Dependency Cascading:** As soon as Task 1 finishes, dependent tasks automatically dispatch to Bob in Terminal 4.
9. **Review Dashboard:** Press `[ Tab ]` to switch to Screen 3 and view the completed fleet summary.

---

### Step 5: TUI Keyboard Controls

#### Global Navigation
| Key | Action |
| :--- | :--- |
| `Tab` | Cycle screens forward: Project Input → Plan Review → Dashboard |
| `1` | Jump directly to Screen 1: Project Input |
| `2` | Jump directly to Screen 2: Plan Review |
| `3` | Jump directly to Screen 3: Live Dashboard |
| `r` / `R` | Refresh current state from PostgreSQL |
| `q` / `Q` | Quit AgentMesh Coordinator |

#### Screen 1: Project Input
| Key | Action |
| :--- | :--- |
| `Enter` or `i` | Enter text editing mode (`EnteringProject`) |
| `Tab` / `BackTab` | Switch focus between Project Name and Description fields |
| `Enter` | Submit project for AI planning and decomposition |
| `Esc` | Cancel editing mode and revert input |

#### Screen 2: Plan Review
| Key | Action |
| :--- | :--- |
| `↑` / `k` | Navigate up through proposed tasks |
| `↓` / `j` | Navigate down through proposed tasks |
| `Enter` | Toggle task details and dependency panel |
| `y` / `Y` | **Approve Task** (blocked if unacknowledged critical overlap exists) |
| `n` / `N` | **Reject Task** (marks task Rejected) |
| `e` / `E` | **Edit Description** (opens inline buffer; `Enter` confirms & approves, `Esc` cancels) |
| `a` / `A` | **Acknowledge Overlap** on the selected task (unblocks approval) |

#### Screen 3: Live Dashboard
| Key | Action |
| :--- | :--- |
| `↑` / `k` | Navigate up through active overlap warnings |
| `↓` / `j` | Navigate down through active overlap warnings |
| `a` / `A` / `Enter` | Acknowledge selected overlap warning |

---

### Step 6: AI Provider Configuration

AgentMesh supports multiple AI task decomposition backends via the `LlmProvider` trait:

#### 1. Local Mock Planner (Default, Zero API Keys)
In `.env`:
```bash
# Uses built-in deterministic planning engine (no external API calls or keys required)
AI_PROVIDER=mock
```
Generates realistic, deterministic task graphs with dependency relationships and resource estimations. Ideal for development, testing, and offline demonstrations.

#### 2. Anthropic Claude 3.5 Sonnet
In `.env`:
```bash
# Use Anthropic Claude 3.5 Sonnet for real task decomposition
AI_PROVIDER=anthropic
AI_MODEL=claude-3-5-sonnet-20241022
ANTHROPIC_API_KEY=sk-ant-api03-your-key-here
```
Uses Claude 3.5 Sonnet to decompose real-world software specifications into task graphs, capability assignments, and file paths. Prompts enforce strict JSON adherence validated before saving.

---

## Complete Command Reference / Cheat Sheet

Here is a quick reference of every command you may need while working with AgentMesh:

### 1. Setup & Installation Commands
```bash
# Make automated installer executable and run full setup
chmod +x install.sh
./install.sh                     # Full setup (checks tools, creates .env, starts Docker, builds debug)
./install.sh --release           # Build optimized release binaries for faster execution
./install.sh --skip-docker       # Build binaries only (if native DB/NATS are already running)
./install.sh --help              # View all script options

# Manual setup commands
cp .env.example .env             # Create environment configuration file from template
cargo build --workspace          # Compile all crates in debug mode (fast compilation)
cargo build --release --workspace# Compile all crates in release mode (optimized)
cargo install --path crates/coordinator # Install coordinator globally to ~/.cargo/bin
cargo install --path crates/agent-mock  # Install mock agent globally to ~/.cargo/bin
```

### 2. Infrastructure & Docker Commands
```bash
# Docker service lifecycle commands
docker compose up -d             # Start PostgreSQL and NATS in the background
docker compose ps                # Check container health and running status
docker compose logs -f           # Follow combined logs of all backing services
docker compose logs -f postgres  # Follow PostgreSQL logs specifically
docker compose logs -f nats      # Follow NATS JetStream logs specifically
docker compose restart           # Restart backing services
docker compose down              # Stop containers (preserves database volumes)
docker compose down -v           # Stop containers and WIPE all database & JetStream volumes
```

### 3. Coordinator Execution Commands
```bash
# Standard interactive TUI mode (auto-runs migrations and provisions streams)
cargo run --bin coordinator

# Run using the compiled release binary
./target/release/coordinator

# Run with debug tracing redirected to a log file (prevents TUI screen distortion)
RUST_LOG=agentmesh=debug,coordinator=debug cargo run --bin coordinator 2> coordinator_debug.log

# View the debug log stream in real time in another terminal window
tail -f coordinator_debug.log
```

### 4. Agent Worker Execution Commands
```bash
# Start standard mock agents with custom human owner labels
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock

# Tune execution delay (e.g. 500ms for fast tests or 5000ms for realistic pacing)
MOCK_TASK_DELAY_MS=500 MOCK_AGENT_OWNER="Fast Worker" cargo run --bin agent-mock

# Connect agent to a remote coordinator / NATS server across LAN, VPN, or Tailscale
NATS_URL="nats://192.168.1.50:4222" NATS_AUTH_TOKEN="agentmesh_dev_token" MOCK_AGENT_OWNER="Remote Dev" cargo run --bin agent-mock

# Start pre-compiled release agent binary
MOCK_AGENT_OWNER="Alice" ./target/release/agent-mock
```

### 5. Inspection & Health Check Commands
```bash
# Verify NATS HTTP monitoring server (returns OK)
curl -i http://localhost:8222/healthz

# Inspect NATS server health status and JetStream memory metrics in JSON format
curl -s http://localhost:8222/varz | grep -o '"jetstream":{[^}]*}'

# Connect to PostgreSQL directly via psql inside the Docker container
docker compose exec postgres psql -U agentmesh -d agentmesh

# Useful SQL inspection queries to run inside psql:
# \dt                            # List all tables (projects, tasks, task_deliveries, agents, etc.)
# SELECT id, name, status FROM projects;
# SELECT short_id, title, status, assignee_id FROM tasks;
# SELECT id, human_owner, status, capabilities FROM agents;
# SELECT id, task_id, agent_id, status, attempt FROM task_deliveries;
```

### 6. Testing & Verification Commands
```bash
# Run the entire test suite across all crates (119 total tests)
cargo test --workspace

# Run tests for a specific crate
cargo test -p coordinator        # Coordinator core, state machine, repositories, and TUI tests
cargo test -p agent-protocol     # Protocol serialization and specification tests
cargo test -p agent-mock         # Mock agent worker tests

# Run end-to-end multi-agent integration test specifically
cargo test --test phase7_e2e_demo

# Run code style formatting and linter checks
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Architecture

AgentMesh strictly separates presentation, orchestration, transport, and authoritative persistence:

```text
                     HUMAN OPERATOR
                           │
                           ▼
                      RATATUI TUI
              (Presentation & Intent Layer)
                           │
                           ▼
                    COORDINATOR CORE
       ┌───────────────────┼───────────────────┐
       ▼                   ▼                   ▼
  AI Planning         Validation &        Assignment &
  (LlmProvider)     Overlap Detection     Gating Logic
       │                   │                   │
       └───────────────────┼───────────────────┘
                           │
            ┌──────────────┴──────────────┐
            ▼                             ▼
       POSTGRESQL 16                NATS 2.10 + JETSTREAM
   (Authoritative State)             (Transport Layer)
   • Projects & Tasks                • coordinator.tasks.assign.*
   • TaskApprovals (Hard Gate)       • agents.*.events
   • TaskDeliveries                  • coordinator.agents.register
   • OverlapWarnings                 • coordinator.agents.heartbeat.*
                                          │
                      ┌───────────────────┴───────────────────┐
                      ▼                                       ▼
                 AGENT ADAPTER                           AGENT ADAPTER
                  (agent-mock)                            (agent-mock)
                 Alice (Backend)                          Bob (Infra)
```

### Component Responsibilities

- **Human Operator:** Final authority over task approval, rejection, inline description editing, and critical overlap resolution. No task executes without recorded human sign-off.
- **Ratatui TUI:** Terminal user interface. Renders screens and emits `TuiAction` events. Completely decoupled from database queries, network I/O, or business logic.
- **Coordinator Core:** Central state machine engine. Dispatches operator commands, runs assignment cycles with dependency checks, performs concurrency locking (`FOR UPDATE SKIP LOCKED`), and reconciles deliveries.
- **AI Planner:** Decomposes natural language project descriptions into structured tasks, suggested agent mappings, affected resource paths, and directed dependencies (`Blocks`, `RelatesTo`).
- **PostgreSQL 16:** The authoritative source of truth. Stores all projects, tasks, dependency DAGs, human approval audits, delivery records, agent registries, and overlap warnings.
- **NATS 2.10 + JetStream:** Low-latency distributed message broker. Handles request-reply registration, pub/sub heartbeats, durable WorkQueue task delivery, and ordered agent lifecycle telemetry.
- **Agent Protocol:** Transport-agnostic serde message contract shared by the coordinator and all agent adapters.
- **Agent Adapters:** Worker bridges that translate coordinator task specifications into agent actions, and report lifecycle events (`TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, `Failed`, `Heartbeat`) back to the coordinator.

---

## Core Workflow

The end-to-end execution lifecycle follows a deterministic, human-gated pipeline:

```text
1. Human enters project description in TUI (Screen 1)
   │
2. Coordinator submits PlanningRequest to configured LlmProvider
   │
3. LLM returns ProposedTasks, Affected Resources, and ProposedDependencies
   │
4. PlanValidator checks proposal deterministically (acyclicity, unique IDs, agent existence)
   │
5. Proposal and Proposed tasks are persisted in PostgreSQL
   │
6. OverlapDetector runs Floyd-Warshall reachability on affected resources:
   - Concurrent tasks sharing a resource -> Critical overlap warning
   - Sequential tasks sharing a resource -> Info overlap warning
   │
7. TUI switches to Plan Review (Screen 2). Human reviews tasks one-by-one:
   - [Y] Approve: Permitted only if no unacknowledged critical overlaps exist
   - [A] Acknowledge: Clears critical conflict blocking condition
   - [E] Edit: Modifies task description inline, confirms, and approves
   - [N] Reject: Transitions task to terminal Rejected state
   │
8. Coordinator Assignment Cycle executes:
   - Queries Approved tasks whose blocking dependencies are all Completed
   - Locks task and available agent via SELECT FOR UPDATE SKIP LOCKED
   - Inserts TaskDelivery record in PostgreSQL (status = Pending)
   │
9. Coordinator publishes TaskAssignment to NATS JetStream (tasks.{agent_id}.assigned)
   - On publish success, TaskDelivery status -> Delivered
   │
10. Agent consumes assignment from JetStream and sends transport ACK
    │
11. Agent reports TaskStarted { idempotency_key, timestamp }
    - Coordinator sets TaskDelivery -> Acknowledged, Task -> Executing, Agent -> Busy
    │
12. Agent publishes periodic ProgressUpdate and Heartbeat events
    │
13. Agent reports completion:
    - If successful: AgentMessage::Completed -> Task status becomes Completed, Agent becomes Idle
    - If blocked: AgentMessage::Blocked -> Task status becomes Blocked
    - If failed: AgentMessage::Failed -> Task status becomes Failed
    │
14. Dependency DAG unblocks: Completed tasks trigger the next assignment cycle,
    allowing downstream dependent tasks to become ready for assignment
    │
15. TUI Live Dashboard (Screen 3) projects real-time state; Coordinator reaches Done
```

> **Architectural Guarantee:** The LLM only proposes plans. It cannot transition tasks to `Assigned`, cannot publish directly to agents, and cannot bypass the human approval boundary.

---

## Features

- **AI Task Planning:** Generates dependency-aware task graphs with suggested agent mappings and resource estimations using Anthropic Claude 3.5 Sonnet or a deterministic local mock planner.
- **Deterministic DAG Validation:** Validates plans for graph acyclicity (cycle detection), task ID uniqueness, self-referential blocks, and valid agent references before database persistence.
- **Human Approval Gate:** Enforces that no task can advance from `Proposed` to `Assigned` without an explicit human approval record in PostgreSQL.
- **Per-Task Review & Inline Editing:** Terminal review interface supporting single-key approval (`y`), rejection (`n`), description editing (`e`), and detail pane expansion (`Enter`).
- **Critical Resource Overlap Detection:** Computes transitive reachability across tasks sharing files or API resources. Concurrent access raises a `Critical` overlap warning that strictly blocks task approval until explicitly acknowledged (`a`).
- **Transactional Assignment & Concurrency Locking:** Uses PostgreSQL `FOR UPDATE SKIP LOCKED` to guarantee that concurrent coordinator workers cannot double-assign a task or claim the same agent simultaneously.
- **NATS JetStream Task Delivery:** Publishes assignments over durable `WorkQueue` streams with idempotency keys (`{task_id}:{attempt}`) and ACK tracking.
- **Agent Lifecycle Monitoring:** Ingests `TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, `Failed`, and `Heartbeat` messages to update database and dashboard state in real time.
- **Heartbeat Timeout Detection:** Background monitor automatically marks agents `Offline` if no heartbeat is received within 30 seconds.
- **Delivery Reconciliation & Crash Safety:** Out-of-band reconciliation detects stale `Pending` or unacknowledged `Delivered` messages and handles reassignment safely.
- **Ratatui Terminal Dashboard:** 3-screen terminal UI featuring project initiation, interactive plan review, and a fleet overview with real-time telemetry streaming.
- **Docker Compose Stack:** Pre-configured, health-checked PostgreSQL 16 and NATS 2.10 JetStream services.
- **Comprehensive Test Suite:** 119 verified tests covering domain invariants, repository CRUD, JetStream delivery, AI planning validation, TUI action emission, and full end-to-end integration.

---

## Project Structure

```text
AgentMesh/
├── Cargo.toml                  # Workspace definition (resolver v2, shared pins)
├── docker-compose.yml          # PostgreSQL 16 + NATS 2.10 JetStream configuration
├── .env.example                # Documented template for all environment variables
├── .gitignore                  # Excludes target, secrets, logs, and build artifacts
├── TODO.md                     # Authoritative implementation task list
├── README.md                   # System documentation and developer guide
│
├── docs/                       # Architectural specifications
│   ├── architecture.md         # System architecture and success criteria
│   ├── domain-model.md         # Canonical state machines and relational models
│   ├── protocol.md             # Agent protocol and NATS topic specifications
│   └── decisions/              # Architecture Decision Records (ADRs 001–008)
│
├── migrations/                 # SQL migrations applied automatically by coordinator
│   └── 20260916000001_initial_schema.sql
│
└── crates/
    ├── coordinator/            # Primary coordinator binary and core library
    │   ├── src/
    │   │   ├── ai/             # LLM provider trait, mock/Anthropic clients, schema & DAG validator
    │   │   ├── coordinator/    # Coordinator engine, assignment service, overlap detector, command handler
    │   │   ├── db/             # PostgreSQL connection pool and repositories (sqlx)
    │   │   ├── domain/         # Pure domain entities, enums, and state machine invariants
    │   │   ├── messaging/      # NATS JetStream client, publisher, subscriber, registration & heartbeat
    │   │   ├── tui/            # Ratatui application, screens (ProjectInput, PlanReview, Dashboard)
    │   │   ├── lib.rs          # Library root
    │   │   └── main.rs         # Binary entry point, background service wiring, TUI event loop
    │   └── tests/              # Phase integration tests (phase2 through phase7_e2e_demo)
    │
    ├── agent-protocol/         # Pure serde message types and TaskSpec (zero I/O dependencies)
    │   └── src/
    │       ├── adapter.rs      # AgentAdapter trait definition
    │       ├── messages.rs     # AgentMessage and CoordinatorMessage enums
    │       ├── spec.rs         # TaskSpec data contract
    │       └── status.rs       # AgentStatus, DeliveryAck, AckKind
    │
    ├── agent-mock/             # Simulated agent runner for tests and local demonstrations
    │   └── src/
    │       ├── adapter.rs      # MockAgent struct with configurable delay, blocker, and failure simulation
    │       ├── runner.rs       # NATS registration, heartbeat loop, JetStream task execution
    │       └── main.rs         # Executable mock agent binary
    │
    └── agent-agy/              # Scaffold for Google Antigravity (agy) CLI adapter
        └── src/
            └── main.rs         # Subprocess adapter scaffold (planned for future phases)
```

---

## Multi-Device / Remote Agent Setup

AgentMesh supports distributed setups where coding agents run on remote developer workstations or cloud VMs while connecting to a central coordinator:

```text
                            LAN / PRIVATE VPN / TAILSCALE
                                          │
                                          ▼
                             ┌─────────────────────────┐
                             │     AGENTMESH HOST      │
                             │                         │
                             │  Coordinator Binary     │
                             │  PostgreSQL 16 (:5432)  │
                             │  NATS JetStream (:4222) │
                             └────────────┬────────────┘
                                          │
                        NATS Connection (Token / TLS)
                                          │
                    ┌─────────────────────┴─────────────────────┐
                    ▼                                           ▼
          DEVELOPER WORKSTATION                       DEVELOPER WORKSTATION
          Agent A (Host IP:4222)                      Agent B (Host IP:4222)
          MOCK_AGENT_OWNER="Alice"                    MOCK_AGENT_OWNER="Bob"
```

### Network Configurations Supported in v0.1

1. **Same-Machine Setup (Default):**
   - Coordinator, PostgreSQL, NATS, and mock agents run on `localhost`.
   - Communication over `127.0.0.1:4222` and `127.0.0.1:5432`.

2. **LAN / Private Network / VPN (e.g., WireGuard, Tailscale):**
   - Coordinator and Docker stack run on a central workstation (e.g., IP `192.168.1.50` or Tailscale IP `100.x.y.z`).
   - Remote agents configure their environment:
     ```bash
     NATS_URL=nats://192.168.1.50:4222 NATS_AUTH_TOKEN=agentmesh_dev_token cargo run --bin agent-mock
     ```
   - No code modifications required; NATS handles socket connectivity across the subnet.

3. **Public Internet Setup:**
   - **Current Limitation:** The v0.1 Docker Compose configuration uses basic static token authentication (`agentmesh_dev_token`) over plain TCP.
   - **Do NOT expose port 4222 directly to the public internet without a TLS termination proxy or VPN.** Production multi-device deployment requires NATS NKey credentials and TLS encryption (see [ADR 002](docs/decisions/002-message-queue-nats-jetstream.md)).

---

## NATS Configuration

AgentMesh uses NATS 2.10 with JetStream enabled for message persistence and pub/sub messaging.

- **Client Port:** `4222` (TCP)
- **HTTP Monitoring Port:** `8222` (Web dashboard at `http://localhost:8222`)
- **Authentication:** Token-based via `NATS_AUTH_TOKEN` (configured via `-auth` flag in `docker-compose.yml`)

### JetStream Streams

| Stream Name | Storage | Retention Policy | Subject Filter | Purpose |
|---|---|---|---|---|
| `TASK_ASSIGNMENTS` | File | `WorkQueue` | `coordinator.tasks.assign.*` | Reliable, at-least-once task assignment delivery to agents |
| `AGENT_EVENTS` | File | `Limits` (Max 100k, 24h) | `agents.*.events` | Ingesting agent lifecycle telemetry into coordinator |

### Core Subjects

- `coordinator.agents.register`: Core NATS request-reply subject for agent registration.
- `coordinator.agents.heartbeat.{agent_id}`: Subject where agents publish 5-second heartbeats.
- `tasks.{agent_id}.assigned`: JetStream subject where the coordinator publishes task specs for a specific agent.
- `agents.{agent_id}.events`: JetStream subject where an agent publishes lifecycle events (`TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, `Failed`).

---

## Agent Registration & Protocol

### How an Agent Identifies Itself
When an agent starts, it issues a registration request containing:
- `agent_id`: Unique UUID representing the agent.
- `human_owner`: Human developer responsible for this agent (e.g., `Alice (Backend Lead)`).
- `adapter_type`: Adapter identifier (`mock`, `agy`).
- `capabilities`: Array of capability tags (e.g., `["backend", "rust", "sql"]`).
- `api_key`: Shared authentication key.

The coordinator validates the registration, records or updates the agent in PostgreSQL, and replies with a `RegisterResponse`.

### Agent Protocol Messages (`crates/agent-protocol`)

#### Agent → Coordinator (`AgentMessage`)
```rust
pub enum AgentMessage {
    Register { agent_id, human_owner, adapter_type, capabilities, api_key },
    TaskStarted { agent_id, task_id, idempotency_key, timestamp },
    ProgressUpdate { agent_id, task_id, message, percent, timestamp },
    Blocked { agent_id, task_id, reason, blocking_task_id, timestamp },
    Completed { agent_id, task_id, summary, timestamp },
    Failed { agent_id, task_id, error, timestamp },
    Heartbeat { agent_id, status, current_task_id, timestamp },
}
```

#### Coordinator → Agent (`CoordinatorMessage`)
```rust
pub enum CoordinatorMessage {
    TaskAssignment { spec: TaskSpec },
    TaskCancelled { task_id, reason, timestamp },
    WaitForDependency { task_id, blocking_task_id, message, timestamp },
    RegisterResponse { status, nats_subject, error },
}
```

---

## Agent Adapters

AgentMesh decouples coordination logic from agent execution via adapters:

- **`agent-mock` (Implemented & Verified):** A standalone worker that speaks the complete Agent Protocol over NATS JetStream, sends heartbeats, tracks idempotency keys, and simulates configurable execution delays, blockers, and failures.
- **`agent-agy` (Phase 0 Scaffold):** Designed to interface with the Google Antigravity (`agy`) CLI by wrapping `agy` in a subprocess, parsing stdout/JSON streams, and translating them into `AgentMessage` events. *Current Status: Crate structure scaffolded; live subprocess execution is not yet implemented.*

---

## AI Providers

The coordinator integrates LLMs through the `LlmProvider` trait:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn plan(&self, request: &PlanningRequest) -> Result<PlanningResponse>;
    fn model_name(&self) -> &str;
}
```

### Supported Providers
1. **Mock Provider (`AI_PROVIDER=mock`):** Built-in deterministic planning provider. Decomposes any project into realistic tasks, assignees, affected file paths, and dependencies without network calls or API keys. Default for all automated tests and local demos.
2. **Anthropic (`AI_PROVIDER=anthropic`):** Uses Claude 3.5 Sonnet (`claude-3-5-sonnet-20241022`) via raw REST API requests. Requires `ANTHROPIC_API_KEY`. Prompts enforce strict JSON output adhering to `PlanningResponse`.

---

## Task State Machine

The task lifecycle is strictly enforced by the domain model (`crates/coordinator/src/domain/task.rs`):

```text
                     ┌──────────────┐
                     │   Proposed   │
                     └──────┬───────┘
                            │ (Coordinator persists AI plan)
                            ▼
                     ┌──────────────┐
       ┌────────────►│ HumanReview  │◄────────────┐
       │             └──────┬───────┘             │ (Human reassigns failed task)
       │                    │                     │
       │       ┌────────────┴────────────┐        │
       │       ▼                         ▼        │
┌──────────────┐                  ┌──────────────┐│
│   Rejected   │ (Terminal)       │   Approved   ││
└──────────────┘                  └──────┬───────┘│
                                         │ (Dependencies Completed & Agent Idle)
                                         ▼        │
                                  ┌──────────────┐│
                                  │   Assigned   ││
                                  └──────┬───────┘│
                                         │ (Agent reports TaskStarted)
                                         ▼        │
                                  ┌──────────────┐│
                 ┌───────────────►│  Executing   ││
                 │                └──────┬───────┘│
  (Agent unblocked)                      │        │
                 │    ┌──────────────────┼────────┴───────┐
                 │    ▼                  ▼                ▼
          ┌──────────────┐        ┌──────────────┐ ┌──────────────┐
          │   Blocked    │        │  Completed   │ │    Failed    │
          └──────────────┘        └──────────────┘ └──────────────┘
                                     (Terminal)
```

### Legal Transitions
- `Proposed` → `HumanReview`
- `HumanReview` → `Approved` | `Rejected`
- `Approved` → `Assigned` | `Cancelled`
- `Assigned` → `Executing` | `Failed` | `Cancelled`
- `Executing` → `Blocked` | `Completed` | `Failed` | `Cancelled`
- `Blocked` → `Executing` | `Cancelled`
- `Failed` → `Approved` *(Allows human operator to reassign failed tasks)*

### Terminal States
`Completed`, `Rejected`, and `Cancelled` admit no further transitions.

---

## Dependencies & DAG Validation

AgentMesh supports two dependency relationships:
- **`Blocks`:** Downstream task cannot be assigned until the prerequisite task reaches `Completed`.
- **`RelatesTo`:** Informational link; does not gate assignment.

### Cycle & Graph Validation
Before any plan is accepted into PostgreSQL, [`PlanValidator`](crates/coordinator/src/ai/validator.rs) performs deterministic checks:
- **Acyclicity:** DFS-based topological sorting rejects direct cycles (`A → B → A`) and indirect transitive cycles (`A → B → C → A`).
- **Self-Dependencies:** Tasks cannot depend on themselves.
- **Reference Integrity:** Dependencies must reference tasks that exist in the proposal or project.
- **Short ID Uniqueness:** Duplicate task short identifiers within a plan are rejected.

---

## Overlap Detection & Gating

When tasks share resources (source files, database tables, or API contracts), uncoordinated execution leads to collisions:

```text
Task 1 (Agent A) touches "src/models/order.rs"
Task 2 (Agent B) touches "src/models/order.rs"
```

[`OverlapDetector`](crates/coordinator/src/coordinator/overlap.rs) evaluates these conflicts using Floyd-Warshall reachability:
- **Sequential Overlap (`Info`):** If a directed `Blocks` path exists between Task 1 and Task 2, one must finish before the other starts. Severity is marked `Info`.
- **Concurrent Overlap (`Critical`):** If no directed blocking relationship exists, both tasks could execute simultaneously. Severity is marked `Critical`.

> **Deterministic Approval Rule:** Any task associated with an unacknowledged `Critical` overlap is strictly blocked from approval (`ApprovalGateError::BlockedByCriticalOverlap`). The human operator must press `a` in the TUI to acknowledge the risk before the task can be approved.

---

## Reliability & Failure Handling

- **Authoritative PostgreSQL Ownership:** Task assignment state is owned by PostgreSQL `TaskDelivery` records, not NATS in-memory queues. If NATS restarts, PostgreSQL retains the authoritative delivery history.
- **Idempotency Guarantees:** Deliveries carry an idempotency key (`{task_id}:{attempt}`). Agents store processed keys and deduplicate re-delivered messages on reconnect.
- **Transport ACKs vs Lifecycle Events:** JetStream message ACKs acknowledge receipt of bytes. Execution tracking requires a separate `AgentMessage::TaskStarted` event.
- **Heartbeat Timeout Detection:** If an agent becomes unresponsive for 30 seconds, `HeartbeatMonitor` automatically marks the agent `Offline`.
- **Stale Delivery Recovery:** If a coordinator crashes after inserting a `Pending` delivery record, the reconciliation service (`AssignmentService::reconcile_pending_deliveries`) redelivers or fails expired deliveries.
- **No Automatic Retry Storms:** Failed tasks transition to `Failed` and require human review. The operator can reassign the task via the coordinator, which creates a new `TaskDelivery` with an incremented `attempt` counter.

---

## Testing

The workspace includes comprehensive unit, repository, integration, and end-to-end tests.

Run all tests across the workspace:

```bash
cargo test --workspace
```

### Verified Test Suite Breakdown (119 Total Tests)
- `agent-protocol`: 17 passed (serialization round-trips, spec validation)
- `agent-mock`: 13 passed (lifecycle states, simulated work runner)
- `coordinator` library: 69 passed (state machines, sqlx repositories, DAG validator, overlap detection, TUI state)
- `phase2_integration.rs`: 3 passed (JetStream redelivery on NAK, agent lifecycle over NATS)
- `phase3_planning.rs`: 3 passed (AI planning validation, cycle rejection, proposal persistence)
- `phase4_tui.rs`: 2 passed (TUI headless rendering, keybinding action emission)
- `phase5_coordinator.rs`: 6 passed (human approval gating, dependency gating, concurrency locking)
- `phase6_dashboard_overlap.rs`: 5 passed (overlap persistence, dashboard live telemetry streaming)
- `phase7_e2e_demo.rs`: 1 passed (full end-to-end multi-agent execution lifecycle)

---

## Troubleshooting

### PostgreSQL is Not Running / Connection Refused
- **Symptoms:** Coordinator panics on startup with `Failed to connect to PostgreSQL`.
- **Remedy:** Check container status with `docker compose ps`. If stopped, restart with `docker compose up -d postgres`. Inspect database logs with `docker compose logs postgres`.

### NATS is Not Running / Port 4222 Refused
- **Symptoms:** Coordinator logs `NATS not reachable` and falls back to standalone mock mode.
- **Remedy:** Ensure NATS container is healthy via `docker compose ps`. Verify port 4222 is open: `nc -zv localhost 4222`. Inspect NATS logs with `docker compose logs nats`.

### Port Already in Use (5432 or 4222)
- **Symptoms:** Docker Compose fails to bind port: `bind: address already in use`.
- **Remedy:** Identify the occupying process:
  ```bash
  sudo lsof -i :5432
  sudo lsof -i :4222
  ```
  Stop local PostgreSQL or NATS services running on the host system (`sudo systemctl stop postgresql`).

### Agent Does Not Register
- **Symptoms:** `agent-mock` outputs `Failed to register agent with coordinator`.
- **Remedy:**
  1. Verify the coordinator is running and listening on `coordinator.agents.register`.
  2. Check that `NATS_AUTH_TOKEN` in `.env` matches the token configured in `docker-compose.yml` (`agentmesh_dev_token`).

### Task is Not Delivered to Agent
- **Symptoms:** Task approved in TUI but remains unassigned.
- **Checklist:**
  1. **Dependencies:** Check if the task has prerequisite blocking dependencies that are not yet `Completed`.
  2. **Agent Availability:** Check Screen 3 (Dashboard). If all agents are `Busy` or `Offline`, the coordinator waits until an agent reports `Idle`.
  3. **Critical Overlaps:** Check if the task has unacknowledged critical overlaps blocking approval.

### LLM Planning Fails
- **Symptoms:** TUI displays `AI Planning error: ANTHROPIC_API_KEY environment variable must be set`.
- **Remedy:** If using cloud models, ensure `ANTHROPIC_API_KEY` is set in `.env`. For local development without API keys, set `AI_PROVIDER=mock`.

---

## Security Notes

- **Never Commit Secrets:** The `.env` file is excluded in [`.gitignore`](.gitignore). Only commit sanitized templates like [`.env.example`](.env.example).
- **No Secret Logging:** Coordinator tracing and database logs sanitize sensitive tokens and API keys.
- **NATS Authentication:** The v0.1 Docker Compose environment uses token-based authentication. In production deployments across untrusted networks, replace this with NKey authentication and TLS encryption (see [ADR 002](docs/decisions/002-message-queue-nats-jetstream.md)).
- **Database Access:** Database credentials in `docker-compose.yml` are intended for local development. Use managed secrets and restricted PostgreSQL user roles in production.

---

## Current Status

| Area | Status | Notes |
|---|---|---|
| **Core Architecture (v0.1)** | **Verified** | All 10 v0.1 success criteria met and validated in `phase7_e2e_demo.rs` |
| **Human Approval Boundary** | **Verified** | Strict domain gate preventing unapproved task assignment |
| **Overlap Detection** | **Verified** | Transitive reachability analysis; unacknowledged critical overlaps block approval |
| **PostgreSQL Persistence** | **Verified** | Full schema migrations and sqlx repositories with compile-time query checks |
| **NATS JetStream Transport** | **Verified** | WorkQueue assignments, Limits telemetry, request-reply registration |
| **Ratatui TUI** | **Verified** | 3-screen interactive terminal app with decoupled action emission |
| **Agent Mock Adapter** | **Demo / Mock** | Fully functional simulated agent for local demonstration and integration testing |
| **AI Planning (Anthropic)** | **Verified** | Structured task generation using Claude 3.5 Sonnet |
| **Agent Agy Adapter** | **Experimental** | Phase 0 crate scaffold; live subprocess execution planned for subsequent releases |
| **OpenAI / Gemini Providers** | **Future** | Architecture defined in ADR 006; provider implementations planned for future phases |

---

## Development & Contributing

### Build Workspace
```bash
cargo build --workspace
```

### Run Linter & Formatter
```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

### Inspecting Logs
To run the coordinator with verbose debug logging directed to a file (preventing TUI screen distortion):
```bash
RUST_LOG=debug cargo run --bin coordinator 2> coordinator_debug.log
```

### Adding a New Agent Adapter
1. Create a crate in `crates/` and add it to `Cargo.toml` workspace members.
2. Depend on `agent-protocol = { workspace = true }`.
3. Implement `AgentAdapter` from `agent_protocol::adapter::AgentAdapter`.
4. Connect to NATS, issue `AgentMessage::Register`, and subscribe to `tasks.{agent_id}.assigned`.
5. Report lifecycle events (`TaskStarted`, `ProgressUpdate`, `Completed`, `Failed`) to `agents.{agent_id}.events`.

### Adding a New LLM Provider
1. Add client dependencies to `crates/coordinator/Cargo.toml`.
2. Implement the `LlmProvider` trait in `crates/coordinator/src/ai/`.
3. Handle prompt formatting using `PlanningPrompt::system_prompt()` and parse JSON into `PlanningResponse`.
4. Register the new provider variant in `crates/coordinator/src/ai/mod.rs` (`create_provider_by_name`).

---

## Philosophy

> **AI suggests. Humans decide. Agents execute.**

AI models are exceptional at synthesis, pattern decomposition, and exploring solution spaces. However, production codebases demand deterministic accountability, explicit interface contracts, and unambiguous human intent. 

AgentMesh is built on the principle that the coordination plane must be structurally deterministic. By separating probabilistic AI reasoning from authoritative state management, AgentMesh enables developer teams to harness the speed of parallel coding agents without sacrificing software integrity.
