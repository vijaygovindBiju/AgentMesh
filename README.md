# AgentMesh

> AI suggests → Humans decide → Agents execute

AgentMesh is an AI project coordinator for parallel coding agents. It sits above your repository workflow to decompose software projects, detect resource overlaps, propose dependency-ordered task plans across multiple AI coding agents, and enforce a strict human approval gate before any code executes.

---

## The Problem AgentMesh Solves

When multiple developers deploy AI coding agents on the same codebase simultaneously, coordination breaks down:

- **Resource Conflicts:** Multiple agents modify shared models, schema migrations, or interfaces concurrently, leading to merge collisions and architectural drift.
- **Dependency Inversion:** Downstream tasks are executed before prerequisite schema migrations or core libraries are completed.
- **Lack of Verification:** Unchecked AI agents make autonomous code modifications without human sign-off.
- **Fragmented Visibility:** Human leads have no single pane of glass to observe fleet health, task progress, and blocking issues across remote agent runtimes.

AgentMesh solves this by treating the AI as an advisor rather than an autonomous actor. Authoritative state is locked in PostgreSQL, task delivery is guaranteed via NATS JetStream, and execution is gated behind human review.

---

## Quick Start

For experienced developers who want to get the stack running immediately:

```bash
# 1. Clone and configure
git clone <repo-url>
cd AgentMesh
cp .env.example .env

# 2. Start PostgreSQL and NATS JetStream
docker compose up -d

# 3. Launch the Coordinator TUI (runs DB migrations automatically)
cargo run --bin coordinator

# 4. In separate terminals, launch mock agent workers (optional)
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock
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

## Requirements

### Required for Local Demo
- **Rust Toolchain:** Stable 1.75+ (2021 edition) with `cargo`
- **Docker & Docker Compose:** Docker Engine v20.10+ and Docker Compose v2.0+
- **Terminal:** Standard ANSI-compatible terminal (Linux, macOS, or WSL2)

*Note: No external API keys are required for the local demo. AgentMesh defaults to `AI_PROVIDER=mock` and includes full simulated agent execution via `agent-mock`.*

### Required for Real LLM Usage
- An active API key from **Anthropic** (`ANTHROPIC_API_KEY`) with `AI_PROVIDER=anthropic`.

### Required for Real Coding-Agent Execution
- A supported external coding agent runtime. The `agent-agy` adapter is currently a Phase 0 scaffold. For v0.1, agent execution is fully demonstrated and verified using `agent-mock`.

---

## Installation & Configuration

### 1. Clone Repository & Setup Environment
```bash
git clone <repo-url>
cd AgentMesh
cp .env.example .env
```

### 2. Environment Variables Reference

Edit `.env` to configure your environment. All supported variables are documented below:

| Variable | Default Value | Description |
|---|---|---|
| `DATABASE_URL` | `postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh` | PostgreSQL connection string for the coordinator |
| `POSTGRES_PASSWORD` | `agentmesh_dev` | PostgreSQL superuser password used by Docker Compose |
| `NATS_URL` | `nats://localhost:4222` | NATS server connection URL |
| `NATS_AUTH_TOKEN` | `agentmesh_dev_token` | Client authentication token for NATS connections |
| `AI_PROVIDER` | `mock` | Active planning provider: `mock` or `anthropic` |
| `AI_MODEL` | `claude-3-5-sonnet-20241022` | Model identifier when using a cloud provider |
| `ANTHROPIC_API_KEY` | *(empty)* | API key required when `AI_PROVIDER=anthropic` |
| `RUST_LOG` | `agentmesh=debug,coordinator=debug,info` | Tracing log filter directive |
| `MOCK_TASK_DELAY_MS` | `1000` | Delay in ms between progress stages in `agent-mock` |
| `MOCK_AGENT_OWNER` | `MockDev` | Human owner label for the mock agent |
| `MOCK_AGENT_ID` | *(auto-generated UUID)* | Specific UUID for the mock agent instance |
| `MOCK_AGENT_API_KEY` | `agentmesh_mock_key` | Secret key sent by mock agent during registration |

---

## Start Infrastructure

Start the background database and message broker services:

```bash
docker compose up -d
```

### Verify Service Health

Check that both containers are running and marked `healthy`:

```bash
docker compose ps
```

Expected output:
```text
NAME                   IMAGE                COMMAND                  SERVICE    STATUS
agentmesh-nats-1       nats:2.10-alpine     "/nats-server -js -a…"   nats       Up (healthy)
agentmesh-postgres-1   postgres:16-alpine   "docker-entrypoint.s…"   postgres   Up (healthy)
```

You can also verify the endpoints directly:
- **NATS Client Port:** `localhost:4222`
- **NATS HTTP Monitoring:** `http://localhost:8222` (or check health at `http://localhost:8222/healthz`)
- **PostgreSQL Port:** `localhost:5432`

---

## Run AgentMesh Locally

Launch the Coordinator:

```bash
cargo run --bin coordinator
```

On startup, the coordinator automatically applies all pending SQL migrations from `migrations/` and provisions the required JetStream streams. If launched in an interactive terminal, the Ratatui TUI opens immediately.

### TUI Keyboard Controls

#### Global Navigation
| Key | Action |
|---|---|
| `Tab` | Cycle through screens: Project Input → Plan Review → Dashboard |
| `1` | Jump directly to Screen 1: Project Input |
| `2` | Jump directly to Screen 2: Plan Review |
| `3` | Jump directly to Screen 3: Dashboard |
| `r` / `R` | Refresh current state from PostgreSQL |
| `q` / `Q` | Quit AgentMesh Coordinator |

#### Screen 1: Project Input
| Key | Action |
|---|---|
| `Enter` or `i` | Enter editing mode (`EnteringProject`) |
| `Tab` / `BackTab` | Switch focus between Project Name and Description fields |
| `Enter` | Submit project for AI planning and decomposition |
| `Esc` | Cancel editing mode and revert input |

#### Screen 2: Plan Review
| Key | Action |
|---|---|
| `↑` / `k` | Navigate up through proposed tasks |
| `↓` / `j` | Navigate down through proposed tasks |
| `Enter` | Toggle task details and dependency panel |
| `y` / `Y` | **Approve Task** (blocked if unacknowledged critical overlap exists) |
| `n` / `N` | **Reject Task** (marks task Rejected) |
| `e` / `E` | **Edit Description** (opens inline buffer; `Enter` confirms & approves, `Esc` cancels) |
| `a` / `A` | **Acknowledge Overlap** on the selected task (unblocks approval) |

#### Screen 3: Live Dashboard
| Key | Action |
|---|---|
| `↑` / `k` | Navigate up through active overlap warnings |
| `↓` / `j` | Navigate down through active overlap warnings |
| `a` / `A` / `Enter` | Acknowledge selected overlap warning |

---

## Run Mock Agents

`agent-mock` simulates an external coding agent connecting to the mesh, reporting health, receiving task assignments, and simulating execution phases.

Start mock agents in separate terminal windows:

```bash
# Terminal 3: Start Agent A
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock

# Terminal 4: Start Agent B
MOCK_AGENT_OWNER="Bob (Infra Lead)" cargo run --bin agent-mock
```

### What the Mock Agent Simulates
1. **Registration:** Connects to NATS and issues a request-reply `Register` message on `coordinator.agents.register`.
2. **Heartbeat:** Spawns a background task publishing `Heartbeat` messages to `coordinator.agents.heartbeat.{agent_id}` every 5 seconds.
3. **JetStream Consumer:** Creates a durable pull consumer on the `TASK_ASSIGNMENTS` stream filtering for `tasks.{agent_id}.assigned`.
4. **Execution Telemetry:** Upon receiving a task assignment:
   - ACKs the JetStream transport message.
   - Publishes `AgentMessage::TaskStarted` with the delivery idempotency key.
   - Waits half of `MOCK_TASK_DELAY_MS` and publishes `AgentMessage::ProgressUpdate` (50%).
   - Simulates blockers or failures if configured (`simulate_blocker`, `simulate_failure`).
   - Publishes `AgentMessage::Completed` upon finishing work.

---

## Complete Local Test Walkthrough

Follow this step-by-step test across 4 terminal windows to observe the complete system lifecycle:

```text
┌─────────────────────────┐  ┌─────────────────────────┐
│       TERMINAL 1        │  │       TERMINAL 2        │
│    docker compose up    │  │  cargo run coordinator  │
└─────────────────────────┘  └─────────────────────────┘
┌─────────────────────────┐  ┌─────────────────────────┐
│       TERMINAL 3        │  │       TERMINAL 4        │
│   Mock Agent A (Alice)  │  │    Mock Agent B (Bob)   │
└─────────────────────────┘  └─────────────────────────┘
```

1. **Terminal 1:** Run `docker compose up -d`. Ensure PostgreSQL and NATS are healthy.
2. **Terminal 3 & 4:** Start Mock Agent A and Mock Agent B using the commands above. Observe registration logs confirming they are connected.
3. **Terminal 2:** Launch `cargo run --bin coordinator`.
4. **Enter Project Details:** Press `i` to enter input mode. Type a project name (e.g., `Order Ingress Engine`) and a description. Press `Enter` to submit.
5. **Observe Plan Generation:** The coordinator invokes the planner, creates tasks in PostgreSQL, computes resource overlaps, and switches to Screen 2 (**Plan Review**).
6. **Inspect Dependencies & Overlaps:** Use `↑` / `↓` to select tasks. Press `Enter` to expand the details pane. Notice dependencies (e.g., Task 3 depends on Task 1) and any critical overlap warnings.
7. **Test Approval Gating:** Attempt to approve a task with a critical overlap by pressing `y`. The status bar displays: `APPROVAL BLOCKED: Critical conflict on [...]. Press [a] to acknowledge first.`
8. **Acknowledge Overlap:** Press `a`. The warning is acknowledged and the approval gate unblocks.
9. **Approve Tasks:** Press `y` on Task 1. Notice Task 1 advances to `Approved`, and the coordinator triggers an assignment cycle.
10. **Observe Task Delivery & Execution:**
    - Task 1 is published to JetStream and received by Agent A.
    - Agent A logs `Received task assignment` and reports `TaskStarted`.
    - TUI status bar and Dashboard update with real-time agent progress.
    - Task 3 remains in `Approved` because it is waiting for Task 1 to complete.
11. **Observe Dependency Unblocking:** When Agent A completes Task 1, the coordinator processes `AgentMessage::Completed`. The dependency engine detects that Task 3 is now unblocked and immediately assigns it to the next available agent.
12. **Project Completion:** Switch to Screen 3 (`Tab`). View fleet statuses, completed task counts, and observe the coordinator state machine transition to `Done`.

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
