# AgentMesh

> **AI suggests → Humans decide → Agents execute**

AgentMesh is an AI project coordinator for parallel coding agents. A human describes a project, an LLM proposes a dependency-ordered task plan, the human reviews and approves each task, and the coordinator dispatches approved tasks over NATS JetStream to coding agents (a mock agent for testing, or real [`agy`](#7-real-agy-setup) CLI instances) running on one or many machines. PostgreSQL is the single source of truth; NATS only transports work.

This README documents **AgentMesh v1.0** as it is actually implemented in this repository. Where a capability exists as a library module but is not yet wired into the interactive binary, that is stated explicitly (see [Current Status](#20-current-status)).

---

## Table of Contents

1. [What is AgentMesh?](#1-what-is-agentmesh)
2. [Architecture](#2-architecture)
3. [End-to-End Workflow](#3-end-to-end-workflow)
4. [Installation](#4-installation)
5. [Quick Start](#5-quick-start)
6. [Mock Agent Testing](#6-mock-agent-testing)
7. [Real AGY Setup](#7-real-agy-setup)
8. [Multi-Machine Setup](#8-multi-machine-setup)
9. [Agent Protocol](#9-agent-protocol)
10. [Agent Capability System](#10-agent-capability-system)
11. [Git / Worktree System](#11-git--worktree-system)
12. [Dependencies and Overlaps](#12-dependencies-and-overlaps)
13. [Failure Recovery](#13-failure-recovery)
14. [Dynamic Replanning](#14-dynamic-replanning)
15. [Security](#15-security)
16. [Observability](#16-observability)
17. [Testing](#17-testing)
18. [Project Structure](#18-project-structure)
19. [Troubleshooting](#19-troubleshooting)
20. [Current Status](#20-current-status)
21. [Documentation](#21-documentation)

---

## 1. What is AgentMesh?

When several AI coding agents work on the same codebase at the same time, coordination breaks down:

| Problem | What goes wrong without a coordinator |
| :--- | :--- |
| **Task overlap** | Two agents edit the same file or schema and silently overwrite each other. |
| **Dependency ordering** | A downstream task starts before the migration or interface it depends on exists. |
| **Agent coordination** | Nobody knows which agent is doing what, or whether it is still alive. |
| **Remote agents** | Agents on other machines need a reliable, authenticated way to receive work and report back. |
| **Failure recovery** | A crashed agent leaves a task stuck; a crashed coordinator loses in-flight assignments. |
| **Human control** | Autonomous agents change code with no explicit human sign-off. |

AgentMesh addresses these by treating the LLM strictly as an advisor:

- **AI suggests** — the planner decomposes the project into tasks, dependencies, affected resources, and suggested agents. It can only ever create `Proposed` tasks.
- **Humans decide** — every task must be approved (`y`), edited-and-approved (`e`), or rejected (`n`) in the terminal UI. Unacknowledged critical resource overlaps block approval.
- **Agents execute** — approved, unblocked tasks are delivered over NATS JetStream to registered agents, which report `TaskStarted`, `ProgressUpdate`, `Blocked`, `Completed`, or `Failed`.

---

## 2. Architecture

```text
                         HUMAN
                           │  keyboard (y / n / e / a / Tab / 1-4)
                           ▼
                    ┌─────────────┐
                    │   Ratatui   │  4 screens: Project Input, Plan Review,
                    │     TUI     │  Dashboard, Diagnostics
                    └──────┬──────┘
                           │  TuiAction
                           ▼
                  ┌─────────────────┐
                  │   COORDINATOR   │  crates/coordinator
                  │                 │
                  │ AI Planning     │  LlmProvider (mock | anthropic) + RepositoryScanner
                  │ Task Graph      │  PlanValidator (DAG, cycles, references)
                  │ Validation      │  OverlapDetector (resource reachability)
                  │ Human Gate      │  task_approvals row required before assignment
                  │ Assignment      │  FOR UPDATE SKIP LOCKED + TaskDelivery outbox
                  └───────┬─────────┘
                          │
             ┌────────────┴────────────┐
             ▼                         ▼
       PostgreSQL 16              NATS 2.10 JetStream
       Source of Truth            Transport only
       projects, tasks,           TASK_ASSIGNMENTS (WorkQueue)
       approvals, deliveries,     AGENT_EVENTS (Limits)
       agents, events,            coordinator.agents.register (req/reply)
       audit_logs, git_conflicts  coordinator.agents.heartbeat.* (core NATS)
                                        │
                         ┌──────────────┼──────────────┐
                         ▼              ▼              ▼
                      Agent A        Agent B        Agent C
                    agent-agy      agent-agy      agent-mock
                         │              │              │
                       agy CLI        agy CLI      simulated work
```

### Components

| Component | Location | Responsibility |
| :--- | :--- | :--- |
| **Ratatui TUI** | `crates/coordinator/src/tui/` | Renders screens, emits `TuiAction`s. No DB or network access of its own. |
| **Coordinator core** | `crates/coordinator/src/coordinator/` | State machine (`Idle → ProjectInput → Planning → HumanReview → Assigning → Executing → Done`), approval gate, assignment cycle, overlap detection. |
| **AI planning** | `crates/coordinator/src/ai/` | `LlmProvider` trait, `MockLlmProvider`, `AnthropicProvider`, `PlanValidator`, `RepositoryScanner`, `AgentCapabilityMatcher`, `ComplexityEstimator`, `ReplanEngine`. |
| **Persistence** | `crates/coordinator/src/db/`, `migrations/` | sqlx repositories with compile-time checked SQL; 5 migrations applied automatically at startup. |
| **Messaging** | `crates/coordinator/src/messaging/` | NATS connection, stream provisioning, registration handler, heartbeat monitor, task publisher, event subscriber. |
| **Git coordination** | `crates/coordinator/src/git/` | Repository identity, `agentmesh/<short-id>` branches, per-task worktrees, resource tracking, merge-conflict detection. |
| **Security** | `crates/coordinator/src/security/`, `agent-protocol/src/security.rs` | API key hashing, roles, permission boundaries, task authorization, NATS subject authorization, secret redaction, audit log. |
| **Observability** | `crates/coordinator/src/observability/` | Task/agent timelines, delivery visibility, failure diagnostics, system metrics, coordinator events. |
| **Reliability** | `crates/coordinator/src/reliability/` | Startup recovery, stale task sweeper, event deduplication, reconnect/retry helpers. |
| **Agent protocol** | `crates/agent-protocol/` | Pure serde message types shared by coordinator and every adapter. Zero I/O dependencies. |
| **Mock agent** | `crates/agent-mock/` | Full protocol implementation with simulated work; used for local demos and tests. |
| **agy adapter** | `crates/agent-agy/` | Spawns the `agy` CLI as a subprocess, parses its NDJSON stream, translates it into protocol events. |

---

## 3. End-to-End Workflow

The following steps are implemented and exercised in the integration tests. Steps marked **(TUI)** happen in the interactive coordinator binary; steps marked **(library)** are implemented and tested but are not yet triggered automatically by the binary.

```text
Project                    (TUI)   Screen 1: name + description, Enter
   ↓
Repository Discovery       (TUI)   RepositoryScanner scans the coordinator's working directory
   ↓                               (ecosystems, languages, crates, file tree, README summary)
AI Planning                (TUI)   LlmProvider returns tasks, dependencies, affected resources,
   ↓                               suggested agents (AgentCapabilityMatcher), complexity (XS–XL)
DAG Validation             (TUI)   PlanValidator rejects cycles, self-deps, unknown refs, dup IDs
   ↓
Human Review               (TUI)   Screen 2: y approve / n reject / e edit+approve / a acknowledge
   ↓
Overlap / Dependency Checks(TUI)   Critical overlap blocks approval until acknowledged;
   ↓                               blocked tasks are not claimable until blockers are Completed
Capability Matching        (TUI)   Suggested agent preferred if idle+healthy, else any eligible agent
   ↓
Task Assignment            (TUI)   Transaction: task→Assigned, agent→Busy, TaskDelivery(Pending),
   ↓                               then publish TaskAssignment to coordinator.tasks.assign.{agent_id}
Git Worktree               (library) GitCoordinator creates agentmesh/<short-id> branch + worktree
   ↓
Agent Execution            agent   agent-mock simulates; agent-agy runs `agy -p <prompt>`
   ↓
Progress / Events          agent   TaskStarted, ProgressUpdate, Blocked, Completed, Failed → agents.{id}.events
   ↓
Completion / Failure       (TUI)   EventSubscriber persists events, updates task + agent state
   ↓
Metrics / Timeline / Diagnostics (library) TimelineService, MetricsCollector, FailureDiagnostics
   ↓
Replanning                 (library) ReplanEngine builds a new proposal → back to Human Review
```

> **Important:** in v1.0 the assignment cycle runs when a human approves or edits a task in the TUI. When a task completes, its dependents become *claimable* in PostgreSQL immediately, but they are dispatched on the next assignment cycle (i.e. the next approve/edit action). There is no background assignment timer in the binary yet.

---

## 4. Installation

### Requirements

| Requirement | Version used in this repo | Purpose |
| :--- | :--- | :--- |
| **Rust toolchain** | stable, edition 2021 (developed and tested with rustc 1.97) | Build `coordinator`, `agent-mock`, `agent-agy` |
| **Docker Engine + Compose v2** | any recent (`docker compose` plugin) | Run PostgreSQL and NATS via `docker-compose.yml` |
| **PostgreSQL** | 16 (`postgres:16-alpine`) | Source of truth. Provided by Compose, or use your own instance via `DATABASE_URL` |
| **NATS** | 2.10 with JetStream (`nats:2.10-alpine`) | Task delivery and events. Provided by Compose, or your own via `NATS_URL` |
| **Git** | any modern version | Repository scanning, worktrees, conflict detection (`git worktree`, `git merge-tree`) |
| **`agy` CLI** | must support `-p`, `--output-format stream-json`, `--dangerously-skip-permissions`, `--model`, `--effort`, `--add-dir` | Only for `agent-agy`. Not needed for mock agents or tests. |
| **Anthropic API key** | optional | Only when `AI_PROVIDER=anthropic`. Default `mock` planner needs no key. |

Linux and macOS are supported. Windows via WSL2 should work but has not been validated.

### Ports (Docker Compose defaults)

| Port | Service |
| :--- | :--- |
| `5432` | PostgreSQL |
| `4222` | NATS client connections (agents and coordinator connect here) |
| `8222` | NATS HTTP monitoring (`/healthz`, `/varz`) |

### Option A — `install.sh`

```bash
chmod +x install.sh
./install.sh              # checks tools, creates .env, starts Docker, builds debug binaries
./install.sh --release    # build optimized binaries
./install.sh --skip-docker
./install.sh --skip-build
./install.sh -y           # non-interactive
```

### Option B — Manual

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Debian/Ubuntu build deps (macOS: brew install git curl; Arch: pacman -S base-devel rustup docker docker-compose)
sudo apt install -y curl git build-essential pkg-config libssl-dev

git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cp .env.example .env
docker compose up -d
docker compose ps           # both services should be "Up (healthy)"
cargo build --workspace     # produces target/debug/{coordinator,agent-mock,agent-agy}
```

### Environment variables

The **coordinator** loads `.env` via `dotenvy`. The **agents (`agent-mock`, `agent-agy`) read only the process environment** — export variables or prefix them on the command line.

| Variable | Default | Used by | Purpose |
| :--- | :--- | :--- | :--- |
| `DATABASE_URL` | `postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh` | coordinator, tests | PostgreSQL connection |
| `POSTGRES_PASSWORD` | `agentmesh_dev` | docker-compose | Password for the Postgres container (must match `DATABASE_URL`) |
| `NATS_URL` | `nats://localhost:4222` | all | NATS server |
| `NATS_AUTH_TOKEN` | `agentmesh_dev_token` | all | NATS token; must match the `-auth` value passed to the NATS container |
| `AI_PROVIDER` | `mock` | coordinator | `mock` or `anthropic` (only these two are implemented) |
| `AI_MODEL` | `claude-3-5-sonnet-20241022` | coordinator | Model name for the Anthropic provider |
| `ANTHROPIC_API_KEY` | *(empty)* | coordinator | Required only if `AI_PROVIDER=anthropic` |
| `RUST_LOG` | `info` | all | Tracing filter (coordinator logs to stderr) |
| `MOCK_AGENT_OWNER` | `MockDev` | agent-mock | Human owner label |
| `MOCK_AGENT_ID` | random UUID | agent-mock | Fixed UUID (must be a valid UUID, otherwise ignored) |
| `MOCK_AGENT_API_KEY` | `agentmesh_mock_key` | agent-mock | Key presented at registration |
| `MOCK_TASK_DELAY_MS` | `1000` | agent-mock | Simulated work delay per stage |
| `AGY_AGENT_OWNER` | `AgyDev` | agent-agy | Human owner label |
| `AGY_AGENT_ID` | random UUID | agent-agy | Fixed UUID (must be valid, otherwise ignored) |
| `AGY_AGENT_API_KEY` | `agentmesh_agy_key` | agent-agy | Key presented at registration |
| `AGY_BIN_PATH` | `~/.local/bin/agy` if it exists, else `agy` on `PATH` | agent-agy | Path to the `agy` binary |
| `AGY_MODEL` | *(agy default)* | agent-agy | Passed as `--model` |
| `AGY_EFFORT` | `medium` | agent-agy | Passed as `--effort` (`low`, `medium`, `high`) |
| `AGY_WORKSPACE_DIR` | *(none)* | agent-agy | Default working directory; overridden per task by `TaskSpec.repo_path` |
| `AGY_TIMEOUT_SECS` | `600` | agent-agy | Kill the `agy` subprocess after this many seconds |

> If `AI_PROVIDER=anthropic` is set but `ANTHROPIC_API_KEY` is **unset**, provider creation fails and the coordinator silently falls back to the mock planner (see `handle_tui_action` in `crates/coordinator/src/main.rs`). If the key is set but empty or invalid, the Anthropic call fails and `AI Planning error: …` is shown in the status bar.

---

## 5. Quick Start

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh

cp .env.example .env          # defaults work out of the box (mock planner, local Docker services)

docker compose up -d          # PostgreSQL 16 + NATS 2.10 JetStream

cargo test --workspace        # see "Testing" for what requires the Docker services

cargo run --bin coordinator   # opens the TUI; runs migrations and creates JetStream streams on startup
```

In two more terminals:

```bash
MOCK_AGENT_OWNER="Alice (Backend Lead)" cargo run --bin agent-mock
MOCK_AGENT_OWNER="Bob (Infra Lead)"    cargo run --bin agent-mock
```

Then in the TUI:

1. Press `i`, type a project name, `Tab`, type a description, `Enter`.
2. On **Plan Review**, use `↑/↓`, `Enter` for details, `a` to acknowledge a critical overlap, `y` to approve. The approved task is dispatched immediately if an eligible agent is idle.
3. `Tab` or `3` for the **Dashboard** (agents, tasks, overlaps); `4` for **Diagnostics**; `r` to refresh from PostgreSQL; `q` to quit.

If PostgreSQL or NATS are unreachable, the coordinator starts in **standalone demo mode** with in-memory sample data so you can explore the TUI. When stdout is not a TTY (e.g. `cargo run --bin coordinator > coordinator.log`), it runs as a headless daemon that only serves registration, heartbeats, and event ingestion.

### TUI keys

| Key | Where | Action |
| :--- | :--- | :--- |
| `Tab` | global | Cycle screens: Project Input → Plan Review → Dashboard → Diagnostics |
| `1` `2` `3` `4` | global | Jump to a screen |
| `r` | global | Reload agents, tasks and overlaps from PostgreSQL |
| `q`, `Ctrl+C` | global | Quit |
| `i` / `Enter`, `Tab`, `Enter`, `Esc` | Project Input | Start editing, switch field, submit, cancel |
| `↑/↓` or `k/j` | Plan Review | Move between tasks |
| `Enter` | Plan Review | Toggle details pane (description, dependencies, overlap warnings) |
| `y` / `n` | Plan Review | Approve / Reject selected task |
| `e` then `Enter` / `Esc` | Plan Review | Edit description inline; `Enter` saves **and approves** |
| `a` | Plan Review / Dashboard | Acknowledge the selected overlap warning |
| `↑/↓` | Diagnostics | Scroll the event list |

---

## 6. Mock Agent Testing

`agent-mock` is a real protocol implementation with simulated work. It registers, heartbeats every 5 s, consumes assignments from a durable JetStream consumer (`agent-<id>`, ack wait 60 s, max 5 deliveries), and publishes `TaskStarted → ProgressUpdate(50%) → Completed` with `MOCK_TASK_DELAY_MS` between stages.

```bash
# Local, two agents on the same machine as the coordinator
MOCK_AGENT_OWNER="Alice" cargo run --bin agent-mock
MOCK_AGENT_OWNER="Bob"   MOCK_TASK_DELAY_MS=500 cargo run --bin agent-mock

# Pin a stable identity so the agent keeps the same DB record across restarts
MOCK_AGENT_ID=11111111-1111-4111-8111-111111111111 MOCK_AGENT_OWNER="Alice" cargo run --bin agent-mock

# Release binary
MOCK_AGENT_OWNER="Alice" ./target/release/agent-mock
```

A mock agent on another machine only needs to reach NATS:

```text
Machine A
├── Coordinator
├── PostgreSQL
└── NATS  (port 4222)

Machine A
└── agent-mock                 NATS_URL=nats://localhost:4222

Machine B
└── agent-mock                 NATS_URL=nats://<Machine A IP>:4222
       │                       NATS_AUTH_TOKEN=<same token as Machine A>
       └── NATS → Machine A
```

```bash
# On Machine B
NATS_URL="nats://192.168.1.50:4222" NATS_AUTH_TOKEN="agentmesh_dev_token" \
MOCK_AGENT_OWNER="Remote Dev" cargo run --bin agent-mock
```

Any network that carries TCP to port 4222 works: LAN, a WireGuard/Tailscale VPN IP, or an SSH tunnel. Nothing in the agent is network-specific; it only uses `NATS_URL`. See [Multi-Machine Setup](#8-multi-machine-setup).

> On first start with an empty `agents` table, the coordinator seeds two placeholder `Mock` agents ("Alice (Backend Lead)", "Bob (Infra Lead)") so the planner has assignees. They are DB rows only — no process is listening for them. Real `agent-mock`/`agent-agy` processes register their own rows. If a task is assigned to a seeded placeholder, nothing will execute it; delete those rows (`DELETE FROM agents WHERE api_key_hash IN ('seed_alice','seed_bob');`) before real use.

---

## 7. Real AGY Setup

```text
AgentMesh coordinator
    ↓  TaskAssignment (JetStream: coordinator.tasks.assign.{agent_id})
NATS
    ↓
agent-agy  (crates/agent-agy)
    ↓  spawns: agy -p "<prompt>" --output-format stream-json [--dangerously-skip-permissions] [--model M] [--effort E] [--add-dir DIR]
agy CLI
    ↓  NDJSON on stdout: {"event":"init"...} {"event":"step_update"...} {"event":"result"...}
Coding task executed in the task's working directory
```

### Installing / locating `agy`

`agent-agy` does not install `agy`. It looks for the binary in this order:

1. `AGY_BIN_PATH` if set;
2. `$HOME/.local/bin/agy` if it exists;
3. `agy` on `PATH`.

Verify with `which agy` or `ls -l ~/.local/bin/agy`. The `agy` install must already be authenticated/configured on the agent machine; AgentMesh does not manage `agy` credentials.

### Running an agy agent

```bash
cargo build --bin agent-agy

NATS_URL="nats://localhost:4222" \
NATS_AUTH_TOKEN="agentmesh_dev_token" \
AGY_AGENT_OWNER="Alice (Backend Lead)" \
AGY_AGENT_API_KEY="choose-a-secret" \
AGY_EFFORT="low" \
AGY_TIMEOUT_SECS=900 \
./target/debug/agent-agy
```

Optional: `AGY_AGENT_ID=<uuid>` for a stable identity, `AGY_MODEL=<model>`, `AGY_BIN_PATH=/path/to/agy`, `AGY_WORKSPACE_DIR=/path/to/repo`.

### What the adapter does

1. **Capability detection** — `CapabilityDetector::detect_all` inspects the host (OS, CPU arch, cores; Rust/Python/Node/Go/Dart/C toolchains; tools such as `git`, `docker`, `cargo`, `agy`). Tags always include `agy` and `general-coding`.
2. **Registration** — sends `AgentMessage::Register { adapter_type: "Agy", capabilities, profile, api_key }` as a NATS request to `coordinator.agents.register` and expects `RegisterResponse { status: "ok", nats_subject }`.
3. **Heartbeat** — publishes `Heartbeat { status: Idle }` to `coordinator.agents.heartbeat.{agent_id}` every 5 s. (The heartbeat loop always reports `Idle`; the coordinator learns `Busy` from `TaskStarted`.)
4. **Receiving tasks** — creates a durable pull consumer `agent-agy-<id>` on `TASK_ASSIGNMENTS` filtered to `coordinator.tasks.assign.{agent_id}`, ack wait 600 s, max 5 deliveries. Each message is transport-ACKed on receipt.
5. **Execution** — builds a prompt from the `TaskSpec` (identifier, title, description, affected resources, task/base branch, repository path) and spawns `agy`. If `TaskSpec.repo_path` is set, that directory becomes both `--add-dir` and the working directory.
6. **Progress reporting**
   - `init` or first `step_update` → `TaskStarted { idempotency_key }`
   - `step_update` → `ProgressUpdate { message: "Step N: …", percent: min(10 + 15·N, 90) }`
   - `step_update` indicating a blocker → `Blocked { reason }`
   - `result` with `status == "SUCCESS"` → `Completed { summary }`; any other status → `Failed`
   - exit code ≠ 0, spawn failure, or timeout (`AGY_TIMEOUT_SECS`, process killed) → `Failed { error }`
   - process ends without a result → `Failed("Subprocess exited prematurely…")`
7. **Timeout / crash** — handled by `AgyProcess`; the subprocess is killed on timeout.

### Limits

- `--dangerously-skip-permissions` is passed by default (`dangerously_skip_permissions: true`); there is no environment variable to disable it. Treat the agent host as a sandbox.
- `TaskCancelled` and `WaitForDependency` messages are logged but not acted upon.
- The agent does not commit or push Git changes; it leaves the working tree modified. Commit/merge handling lives in the coordinator's `CompletionManager` (library, see §11).
- Tasks are processed sequentially per agent process (`max_concurrency` = 1 at registration).

`scripts/test_two_agy_instances.sh` starts a coordinator (if none is running) and two `agent-agy` processes with fixed IDs, then checks both registered. Use `--nats-url` / `--auth-token` to point it at a remote coordinator.

---

## 8. Multi-Machine Setup

```text
                 NETWORK (LAN / WireGuard / Tailscale)
                    │
        ┌───────────┴───────────┐
        │                       │
        ▼                       ▼
   YOUR COMPUTER           FRIEND COMPUTER
   ─────────────           ────────────────
   Coordinator             agent-agy   (NATS_URL=nats://<your IP>:4222)
   PostgreSQL   :5432
   NATS         :4222
   agent-agy
```

1. **Coordinator host** — one machine runs `docker compose up -d` and `cargo run --bin coordinator`. PostgreSQL never needs to be reachable from other machines; only the coordinator talks to it.
2. **NATS port** — `4222/tcp`. Open it on the host firewall for the remote machine (or VPN subnet) only. `8222` (monitoring) should stay local.
3. **Address** — the remote machine uses the coordinator host's LAN IP (`ip addr`, `hostname -I`) or its Tailscale/WireGuard IP (`tailscale ip -4`). Nothing else is exchanged; there is no discovery protocol.
4. **Authentication** — NATS is started with `-auth ${NATS_AUTH_TOKEN}`; every client must present the same `NATS_AUTH_TOKEN`. Separately, each agent presents its own `*_AGENT_API_KEY` at registration (see [Security](#15-security)). Change `NATS_AUTH_TOKEN` in `.env` from the default before exposing the port, then `docker compose up -d` again to recreate the NATS container.
5. **Remote registration** — the remote agent sends `Register` to `coordinator.agents.register`; the coordinator creates (or re-authenticates) the `agents` row and replies with the agent's event subject.
6. **Verify** — on the coordinator host:
   ```bash
   docker compose exec postgres psql -U agentmesh -d agentmesh \
     -c "SELECT id, human_owner, adapter_type, status, last_seen FROM agents ORDER BY last_seen DESC;"
   ```
   `status` should be `idle` and `last_seen` should advance every ~5 s. The Dashboard screen (`3`) shows the same fleet table; press `r` to refresh. Agents with no heartbeat for 30 s are marked `offline`.
7. **Task delivery** — `TaskAssignment` is published to `coordinator.tasks.assign.{agent_id}` on the `TASK_ASSIGNMENTS` WorkQueue stream; only that agent's durable consumer receives it. Messages persist in JetStream (file storage) until ACKed, so an agent that starts late still receives its assignment.
8. **Events back** — the agent publishes to `agents.{agent_id}.events`, captured by the `AGENT_EVENTS` stream and consumed by the coordinator's durable `coordinator-events` consumer, which updates PostgreSQL and the TUI.

```bash
# Friend computer
NATS_URL="nats://100.101.102.103:4222" NATS_AUTH_TOKEN="<shared token>" \
AGY_AGENT_OWNER="Friend" AGY_AGENT_API_KEY="<friend's key>" ./target/debug/agent-agy
```

**Security requirements for anything beyond a trusted LAN:** the Compose stack speaks plain TCP with a static token. Do not expose 4222 to the public internet. Use a VPN (WireGuard/Tailscale) or SSH tunnel. The coordinator library supports TLS/mTLS and user/password NATS auth via `NatsSecurityConfig` + `connect_secure`, but the shipped binaries only use `NATS_URL` + `NATS_AUTH_TOKEN`; enabling TLS today requires code changes and a TLS-configured NATS server. A detailed walkthrough is in [`docs/deployment/multi-machine.md`](docs/deployment/multi-machine.md).

---

## 9. Agent Protocol

Defined in `crates/agent-protocol` (`messages.rs`, `spec.rs`, `status.rs`, `capabilities.rs`, `security.rs`). The protocol exists independently of `agy` so that any runtime — mock, `agy`, or a future adapter — is interchangeable from the coordinator's point of view. Messages are JSON with a `"type"` tag. Full schema: [`docs/protocols/agent-protocol.md`](docs/protocols/agent-protocol.md).

### NATS subjects

| Subject | Direction | Transport |
| :--- | :--- | :--- |
| `coordinator.agents.register` | Agent → Coordinator | core NATS request/reply |
| `coordinator.agents.heartbeat.{agent_id}` | Agent → Coordinator | core NATS publish |
| `coordinator.tasks.assign.{agent_id}` | Coordinator → Agent | JetStream `TASK_ASSIGNMENTS` (WorkQueue, file) |
| `agents.{agent_id}.events` | Agent → Coordinator | JetStream `AGENT_EVENTS` (Limits: 100k msgs / 24 h, file) |

### Coordinator → Agent (`CoordinatorMessage`)

| Message | Fields | Implemented behaviour |
| :--- | :--- | :--- |
| `TaskAssignment` | flattened `TaskSpec`: `task_id`, `short_id`, `title`, `description`, `affected_resources[]`, `depends_on[]`, `idempotency_key` (`{task_id}:{attempt}`), `assigned_at`, optional `task_branch`, `base_branch`, `repo_path` | Published by `AssignmentService`; consumed and executed by both adapters |
| `RegisterResponse` | `status` (`"ok"`/`"error"`), `nats_subject?`, `error?` | Reply to `Register` |
| `TaskCancelled` | `task_id`, `reason`, `timestamp` | Type defined; adapters log it but do not act; coordinator does not currently publish it |
| `WaitForDependency` | `task_id`, `blocking_task_id`, `message`, `timestamp` | Type defined; adapters log it; coordinator does not currently publish it |

### Agent → Coordinator (`AgentMessage`)

| Message | Fields | Coordinator effect |
| :--- | :--- | :--- |
| `Register` | `agent_id`, `human_owner`, `adapter_type` (`"Mock"`/`"Agy"`), `capabilities[]`, `profile?` (`AgentCapabilities`), `api_key` | Create agent row or verify key; audit-logged |
| `UpdateCapabilities` | `agent_id`, `profile`, `timestamp` | Updates `agents.capability_profile` |
| `TaskStarted` | `agent_id`, `task_id`, `idempotency_key`, `timestamp` | Delivery → `Acknowledged`, task → `Executing`, agent → `Busy` |
| `ProgressUpdate` | `agent_id`, `task_id`, `message`, `percent`, `timestamp` | Recorded in `agent_events` |
| `Blocked` | `agent_id`, `task_id`, `reason`, `blocking_task_id?`, `timestamp` | Task → `Blocked`, agent → `Blocked` |
| `Completed` | `agent_id`, `task_id`, `summary`, `timestamp` | Task → `Completed`, agent → `Idle`, completion metrics |
| `Failed` | `agent_id`, `task_id`, `error`, `timestamp` | Task → `Failed`, agent → `Error`, failure metrics |
| `Heartbeat` | `agent_id`, `status`, `current_task_id?`, `health?` (`AgentHealth`), `timestamp` | Updates `last_seen`, status, latency and health |

Lifecycle events are authorized (`TaskAuthorizer`: the reporting agent must be the task's assignee) and deduplicated (`EventDeduplicator`, keyed by agent/task/type/percent) before they change state.

---

## 10. Agent Capability System

**Capabilities** (`AgentCapabilities` in `agent-protocol/src/capabilities.rs`):

- `runtime` — OS, architecture, adapter type, CPU count, optional memory and `agy` version
- `languages[]` — name, version, frameworks (detected: Rust, Python, Node/TypeScript, Go, Dart/Flutter, C/C++)
- `tools[]` — name, version, path (detected from `PATH`: git, docker, cargo, sqlx, agy, …)
- `tags[]` — free-form skill tags (`rust`, `backend`, `frontend`, …)

`agent-agy` detects its profile automatically at startup. `agent-mock` registers a flat tag list. Profiles are stored in `agents.capability_profile` (JSONB) and can be refreshed with `UpdateCapabilities`.

**Health** (`HealthStatus`: `Healthy | Degraded | Unhealthy | Offline`) is tracked per agent from heartbeat latency, consecutive task failures and reported `AgentHealth`. **Availability** combines `status = idle`, `is_draining = false`, `is_revoked = false`, non-expired API key, and `active_tasks_count < max_concurrency`.

**Task requirements** (`TaskRequirements`) are inferred by `AgentCapabilityMatcher::infer_task_requirements` from the task title, description and affected file extensions (e.g. `.rs` → language `rust`, tool `cargo`; `.dart` → `dart`/`flutter`).

**Matching** (`rank_candidates`): unavailable or `Unhealthy`/`Offline` agents are ineligible; required OS (+15), required languages (+40 each), required tools (+25 each) and required tags (+20 each) are hard requirements; preferred tags add +15; healthy agents get +10. The top eligible candidate with a positive score becomes the task's `suggested_agent_id`.

**Assignment decision** (`AssignmentService::assign_ready_tasks`): if the task has a suggested agent and that agent is idle, healthy/degraded, not draining/revoked and under its concurrency limit, it is chosen; otherwise the most recently seen eligible agent that has no other active task is chosen. Role and `PermissionBoundary` checks run before the delivery is created.

```text
Task:      Implement Rust backend authentication
Required:  rust (from ".rs" files / text), backend, security

Agent A:   rust, backend, linux       → eligible, score > 0
Agent B:   flutter, dart, frontend    → missing required language "rust" → ineligible

Coordinator → Agent A
```

Matching happens at planning time (and during replanning); it is not re-run at dispatch. If the suggested agent is busy, any eligible agent may take the task.

---

## 11. Git / Worktree System

Module: `crates/coordinator/src/git/`. Schema: `migrations/002_git_coordination.sql`.

| Concept | Implementation |
| :--- | :--- |
| **Repository identity** | `RepositoryIdentity` discovers the repo root, base branch, HEAD SHA and remote URL, and checks for a clean tree. |
| **Task branches** | `BranchStrategy::task_branch_name("TASK-001")` → `agentmesh/task-001` (sanitized, lower-case). Stored in `tasks.task_branch`. |
| **Worktrees** | `AgentWorkspace::create` runs `git worktree add -B <branch> <path> <base>`; default path `<repo>/.agentmesh/worktrees/<short-id>`. `base_commit_sha` is recorded on the task. |
| **Isolation** | Each task has its own index, HEAD and working directory; agents cannot collide on the index lock or overwrite each other's checkouts. `TaskSpec.repo_path` points the agent at its worktree. |
| **Parallel execution** | Verified in `phase9_git_integration.rs` and `phase15` (two tasks, two worktrees, simultaneous JetStream delivery). |
| **Resource tracking** | `ResourceTracker` diffs untracked/uncommitted/committed changes against the base commit and records files not in `affected_resources` in `unexpected_resource_changes`. |
| **Conflict detection** | `ConflictDetector::detect_cross_agent_conflicts` runs a 3-way `git merge-tree` between task branches; results go to `git_conflicts`. `check_concurrency_safety` flags overlapping resource footprints before execution. |
| **Completion** | `CompletionManager::finalize_task_git_state` commits uncommitted agent work on the task branch, records `completion_commit_sha`, checks mergeability to the base branch and removes the worktree. |
| **Diagnostics** | `FailureDiagnostics` surfaces merge collisions and unexpected resources with `RemediationAdvice`. |

Why worktrees: they share one object store while giving each agent a private checkout, so N agents can work on N branches of the same clone without cloning N times or fighting over `.git/index`.

**Wiring status:** `AssignmentService` already forwards `task_branch`/`base_branch`/`repo_path` to agents when present in the database, and `agent-agy` executes in that directory. However, the interactive coordinator binary does not yet call `GitCoordinator::prepare_task_workspace` or `finalize_task_git_state` automatically; these are invoked from the integration tests and are available as library APIs.

---

## 12. Dependencies and Overlaps

- **Dependency kinds:** `Blocks` (gates assignment) and `RelatesTo` (informational). Stored in `task_dependencies`.
- **DAG validation:** `PlanValidator` rejects cycles (direct and transitive), self-dependencies, references to unknown tasks, and duplicate short IDs before anything is written to PostgreSQL.
- **Blocker enforcement:** the assignment query only claims `approved` tasks with an approval row and *no* `blocks` dependency whose blocker is not `completed`.
- **Resource overlap:** `OverlapDetector` computes reachability over `Blocks` edges. Two tasks touching the same resource with a path between them → `Info`; with no ordering between them → `Critical`.
- **Human approval:** `CommandHandler::execute_approve_task` refuses to approve a task with an unacknowledged `Critical` overlap (`ApprovalGateError::BlockedByCriticalOverlap`). Press `a` to acknowledge, then `y`.
- **Automatic unblocking:** when the blocker reports `Completed`, dependents satisfy the claim query immediately. They are dispatched on the next assignment cycle (triggered by the next approve/edit action in v1.0).

---

## 13. Failure Recovery

| Scenario | v1.0 behaviour | Wired in binary? |
| :--- | :--- | :--- |
| **Agent heartbeat failure** | `HeartbeatMonitor::check_timeouts` marks agents with no heartbeat for 30 s as `offline` (checked every 5 s). | Yes |
| **Stale tasks** | `StaleTaskSweeper::sweep` finds `assigned`/`executing` tasks whose agent is offline or stale, unassigns them, reverts to `Approved` (or `HumanReview` after 3 delivery attempts), increments the agent's failure count, records a `task.reclaimed` event, and marks expired unacknowledged deliveries. | Library / tests (`CoordinatorCore::run_stale_sweep`) |
| **Reassignment** | A reclaimed `Approved` task is picked up by the next assignment cycle with `attempt + 1` and a new idempotency key. `CommandHandler::execute_reassign_task` lets a human move a `Failed` task back to `Approved` for a specific agent. | Assignment: yes. Reassign: library (no TUI key) |
| **Duplicate events** | `EventDeduplicator` (in-memory TTL) drops repeated lifecycle events; delivery ACKs are idempotent by `idempotency_key`. | Yes |
| **Coordinator restart** | `CoordinatorRecoveryService::recover_on_startup` republishes `pending` deliveries, sweeps stale tasks, and reports orphans. `AssignmentService::reconcile_pending_deliveries` republishes deliveries stuck in `pending` > 5 s (terminal after 3 attempts). | Library / tests |
| **NATS publish failure** | In-band compensation: delivery → `terminal`, task → `Approved`, agent → `Idle`. | Yes |
| **NATS/DB reconnect** | `ResilientConnection` provides reconnect callbacks and exponential-backoff retry helpers. | Library |
| **Execution failure** | `Failed` → task `Failed`, agent `Error`, failure metrics; **requires a human** to reassign (no automatic retry storms). | Yes |
| **Blocked** | `Blocked` → task `Blocked`, agent `Blocked`; the agent may later resume and report `Completed`. | Yes |

Automatic recovery is limited to what is marked "Yes" above. Anything else needs either a human action in the TUI or a caller of the library APIs.

---

## 14. Dynamic Replanning

`ReplanEngine` (`crates/coordinator/src/ai/replan.rs`), exercised by `phase10_intelligent_planning.rs`:

```text
Current project state      TaskRepository (all tasks + statuses)
        +
Completed tasks            preserved; never re-proposed
        +
Failed tasks               error text fed to the planner for remediation
        +
Blocked tasks              blocker reasons included
        +
Resource changes           unexpected_resource_changes
        +
Git conflicts              git_conflicts
        ↓
ReplanEngine::gather_replan_context → ReplanRequest → LlmProvider (ReplanPrompt)
        ↓
ReplanEngine::execute_replan → validated → new Proposal + Proposed tasks (suggested agents re-matched)
        ↓
Human review               tasks enter HumanReview exactly like an initial plan
```

Replanning never bypasses the approval gate: it only creates `Proposed` tasks, which a human must approve. It is available through `PlanningService::generate_replan`; the TUI does not yet have a key to trigger it.

---

## 15. Security

Implemented in `crates/coordinator/src/security/` and `agent-protocol/src/security.rs`, schema in `migrations/004_security_and_audit.sql`.

- **Agent authentication.** `ApiKeyManager` generates keys as `am_ak_<64 hex>` and stores SHA-256 hashes; verification is constant-time (`subtle`). On **first** registration of an unknown `agent_id`, the presented key is accepted and stored — hashed if it starts with `am_ak_`, otherwise stored as-is (legacy/test path). On **re**-registration the presented key must match. Revoked (`is_revoked`) or expired (`api_key_expires_at`) agents are refused. Rotation/revocation: `AgentRepository::rotate_api_key`, `revoke_agent`.
- **Authorization.** `AgentRole` (`Worker`, `Reviewer`, `ReadOnly`, `Admin`) and `PermissionBoundary` (allowed/denied path globs, `allow_code_modification`, `allow_command_execution`) are enforced by `PermissionEnforcer::validate_task_assignment` before delivery. New agents get role `worker` and a default boundary.
- **Task authorization.** `TaskAuthorizer` rejects lifecycle events from an agent that is not the task's assignee (impersonation) and logs them.
- **NATS.** Token auth (`-auth`) in Compose. `NatsSubjectAuthorizer` validates that an agent only publishes/subscribes to its own subjects (library). `NatsSecurityConfig` supports user/password, `require_tls`, root CA and client certificates via `connect_secure` (library; binaries use token only).
- **Audit logging.** `AuditLogger` writes to `audit_logs`: registrations (success/denied), auth failures, impersonation attempts, revoked-agent actions.
- **Secret handling.** `.env` is git-ignored; `SecretRedactor` masks API keys, passwords and private keys in text/JSON; `SecretScoper::sanitize_env_for_agent` filters environment variables handed to agents. Raw keys are never written to the database.
- **Remote deployment.** See §8. Rotate `NATS_AUTH_TOKEN`, use a VPN, and give each agent its own `am_ak_` key.

**Not production-ready as shipped:**

- Registration is open: any process that can reach NATS with the token can register a *new* agent ID with any key. There is no allow-list or admin approval step.
- Default `.env`/Compose credentials are well-known; TLS is off; `agent-agy` runs `agy --dangerously-skip-permissions`.
- Keys are sent in the registration payload in clear text over NATS; without TLS this is visible on the network.
- Subject authorization is enforced in the coordinator library, not via NATS server accounts/permissions — a malicious client can still publish to another agent's subject at the broker level.

---

## 16. Observability

Module: `crates/coordinator/src/observability/`; schema `migrations/005_observability_and_events.sql`.

| Capability | API | Where visible |
| :--- | :--- | :--- |
| **Task timeline** | `TimelineService::build_task_timeline` — milestones (proposed, approved, assigned, started, progress, completed/failed), elapsed durations | Library / tests |
| **Agent timeline** | `TimelineService::build_agent_timeline` — registration, deliveries, events | Library / tests |
| **Agent events** | `agent_events` table (every lifecycle message) | SQL; Dashboard reflects state changes live |
| **Coordinator events** | `coordinator_events` table (`task.reclaimed`, …) via `CoordinatorEventRepository` | SQL; Diagnostics screen list (populated by library callers / demo mode) |
| **Delivery visibility** | `DeliveryDiagnostics::inspect` — attempts, ACK state, expiry, redelivery | Library |
| **Failure diagnostics** | `FailureDiagnostics::diagnose_task` — merge collisions, unexpected resources, `RemediationAdvice` | Library |
| **System metrics** | `MetricsCollector::collect` → `SystemMetrics` (task/agent counts, delivery success, throughput) | Diagnostics screen (`4`) — shown when metrics are pushed to the TUI; the live binary does not yet push them |
| **Structured logs** | `TraceContext` spans with task/agent/project IDs; `RUST_LOG` filter, stderr | `RUST_LOG=debug cargo run --bin coordinator 2> coordinator_debug.log` |
| **Audit** | `audit_logs` table | SQL |
| **Execution integrity** | verified in `phase15_v1_validation.rs` (`test_phase15_9_…`) | Tests |

Useful SQL (inside `docker compose exec postgres psql -U agentmesh -d agentmesh`):

```sql
SELECT short_id, status, assigned_agent_id, task_branch FROM tasks ORDER BY created_at;
SELECT id, human_owner, adapter_type, status, health_status, last_seen FROM agents;
SELECT task_id, agent_id, attempt, status, idempotency_key FROM task_deliveries ORDER BY delivered_at DESC;
SELECT received_at, event_type, message FROM agent_events ORDER BY received_at DESC LIMIT 20;
SELECT timestamp, actor_id, action, status, details FROM audit_logs ORDER BY timestamp DESC LIMIT 20;
```

---

## 17. Testing

```bash
cargo test --workspace                     # everything
cargo test -p agent-protocol               # pure serde/logic tests, no services needed
cargo test -p agent-agy                    # subprocess/parser tests (uses temp shell scripts as fake agy)
cargo test -p coordinator --lib            # unit + repository tests
cargo test -p coordinator --test phase7_e2e_demo
cargo test -p coordinator --test phase15_v1_validation
cargo test --workspace --no-fail-fast      # keep going after a failing test binary
```

**Services:** most coordinator tests need the Docker Compose PostgreSQL and NATS (`docker compose up -d`). Tests that cannot reach them print `Skipping test: …` and return successfully, so a run without Docker passes but exercises far less. Tests read `DATABASE_URL`, `NATS_URL`, `NATS_AUTH_TOKEN` from `.env`/environment.

**Suite composition (verified on 2026-09-19 against this repository):**

| Binary | Tests |
| :--- | ---: |
| `agent-protocol` lib | 10 |
| `agent-agy` lib | 9 |
| `agent-mock` lib | 0 (covered via coordinator integration tests) |
| `coordinator` lib | 101 |
| `phase2_integration` … `phase7_e2e_demo` | 3, 3, 2, 6, 5, 1 |
| `phase8_agy_integration` | 6 |
| `phase9_git_integration` | 6 |
| `phase10` … `phase15` | 6, 6, 8, 7, 8, 10 |
| **Total** | **197** |

One test is environment-dependent: `phase8_agy_integration::test_one_real_agy_binary_instance` runs the **real** `agy` binary at the hard-coded path `/home/pirate/.local/bin/agy`. It is skipped if the file is absent; if present it needs a working, authenticated `agy` with network access and takes ~2–3 minutes. All other tests use deterministic fake-`agy` shell scripts and pass without external services beyond Docker. See the run results in [Current Status](#20-current-status).

Lint/format:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
```

---

## 18. Project Structure

```text
AgentMesh/
├── Cargo.toml                  # workspace (crates below), shared dependency pins
├── docker-compose.yml          # postgres:16-alpine + nats:2.10-alpine (JetStream, token auth)
├── .env.example                # documented environment variables (copy to .env)
├── install.sh                  # optional bootstrap: checks tools, writes .env, starts Docker, builds
├── README.md
├── migrations/                 # applied automatically by the coordinator at startup
│   ├── 001_initial_schema.sql
│   ├── 002_git_coordination.sql
│   ├── 003_agent_capabilities_and_health.sql
│   ├── 004_security_and_audit.sql
│   └── 005_observability_and_events.sql
├── scripts/
│   └── test_two_agy_instances.sh   # starts two agent-agy processes and checks registration
├── docs/                       # see §21
└── crates/
    ├── agent-protocol/         # shared message types (messages, spec, status, capabilities, security, discovery, adapter trait)
    ├── agent-mock/             # simulated agent: adapter.rs, runner.rs, main.rs
    ├── agent-agy/              # agy adapter: adapter.rs, process.rs (subprocess), parser.rs (NDJSON), runner.rs, main.rs
    └── coordinator/
        ├── src/
        │   ├── main.rs         # binary: env, DB+NATS connect, migrations, streams, listeners, TUI loop
        │   ├── ai/             # provider, anthropic, mock, prompts, schema, validator, service, repo_scanner, matcher, complexity, replan
        │   ├── coordinator/    # engine (CoordinatorCore), state, commands, assignment, overlap
        │   ├── db/             # pool + repositories (projects, tasks, agents, proposals, deliveries, events, overlaps, audit, git_conflicts, unexpected_resources)
        │   ├── domain/         # entities, enums, state machines
        │   ├── git/            # identity, branch, workspace, changes, conflict, completion, coordinator
        │   ├── messaging/      # client, streams, publisher, subscriber, registration, heartbeat
        │   ├── observability/  # logging, timeline, delivery_visibility, diagnostics, metrics, events
        │   ├── reliability/    # recovery, stale_sweeper, deduplication, reconnect
        │   ├── security/       # auth, permissions, task_auth, nats_security, secrets, audit
        │   └── tui/            # state, ui, screens/{project_input,plan_review,dashboard,diagnostics}, widgets
        └── tests/              # phase2 … phase15 integration suites
```

Workspace crate version is `0.1.0` (the coordinator logs `v0.1.0` at startup); "v1.0" refers to the completed roadmap milestone, not the Cargo version.

---

## 19. Troubleshooting

**PostgreSQL connection problems**
- Symptom: coordinator logs `PostgreSQL or NATS JetStream not reachable. Falling back to Standalone Mock Mode.`
- Check: `docker compose ps`, `docker compose logs postgres`, `nc -zv localhost 5432`.
- `DATABASE_URL` must match the Compose credentials (`agentmesh` / `POSTGRES_PASSWORD`). If you changed the password after the first start, the volume still holds the old one: `docker compose down -v` (destroys data) and `up -d` again.
- Port in use: `sudo lsof -i :5432` — stop a host PostgreSQL or change the published port in `docker-compose.yml` and `DATABASE_URL`.

**NATS connection problems**
- Check: `curl -i http://localhost:8222/healthz` (expect `200`), `docker compose logs nats`, `nc -zv <host> 4222`.
- `Authorization Violation` in agent/coordinator logs → `NATS_AUTH_TOKEN` differs from the token the container was started with. Remember agents do not read `.env`.
- `TASK_ASSIGNMENTS stream not found` on an agent → the coordinator has never connected to this NATS server; start the coordinator first (it creates the streams).

**Agent registration failures**
- `Registration request failed` / timeout → no coordinator is listening on `coordinator.agents.register`. Start `cargo run --bin coordinator` (it also works headless with stdout redirected).
- `Registration rejected: Invalid API key` → the `agent_id` already exists with a different key. Use the original key, use a new `*_AGENT_ID`, or rotate/delete the row.
- `Agent key is revoked` / `has expired` → `is_revoked` or `api_key_expires_at` on the `agents` row.
- `Unsupported adapter type` → only `mock` and `agy` are accepted.

**Authentication failures (events ignored)**
- Coordinator logs `Rejected unauthorized task event: task is assigned to another agent` → the reporting agent is not the assignee (`TaskAuthorizer`). Check `tasks.assigned_agent_id`.

**`agy` not found / `agy` fails**
- `Failed to spawn agy process: …` → set `AGY_BIN_PATH=/full/path/to/agy` or put `agy` on `PATH`. Confirm with `"$AGY_BIN_PATH" --help`.
- Task goes `Failed` with an **empty** error and progress steps of type `error_message` → `agy` itself returned `{"event":"result","result":{"status":"ERROR","response":"","error":"…"}}`. The adapter forwards `response` (empty here), not `error`, so run `agy` manually to read the message, e.g. `agy -p "Reply OK" --output-format stream-json --effort low`. A common cause is `RESOURCE_EXHAUSTED (code 429): Individual quota reached` on the `agy` account. `RUST_LOG=debug` on the agent also prints every stdout line.

**Agent heartbeat problems**
- Agent shows `offline` in the Dashboard → no heartbeat for 30 s. Check the agent process is alive and can reach NATS; check clock skew (heartbeat latency is derived from timestamps).
- Agent stuck `busy` after a crash → run a stale sweep from code/tests (`CoordinatorCore::run_stale_sweep`) or reset via SQL: `UPDATE agents SET status='idle', current_task_id=NULL WHERE id='…';`.

**Task stuck in a state**
- `approved` but never `assigned`: a `blocks` dependency is not `completed`; no agent is `idle` + healthy + under concurrency; the task's suggested agent is a seeded placeholder with no process; or you have not triggered an assignment cycle since the blocker completed (approve/edit any task, or restart and approve).
- `assigned` but agent never starts: check the agent's consumer exists (`curl -s localhost:8222/jsz?consumers=true`), that `agent_id` matches, and the agent log for `Received task assignment`.
- `executing` forever: the agent died mid-task; see heartbeat above. Deliveries expire after 60 s without `TaskStarted` (`task_deliveries.expires_at`) and are reclaimed by the sweeper.
- `failed`: needs human action — `CommandHandler::execute_reassign_task` (library) or SQL to set `approved` again; there is no TUI key yet.

**Git / worktree problems**
- `fatal: '<path>' already exists` or leftover `.agentmesh/worktrees/<id>` → `git worktree prune` and remove the directory; `AgentWorkspace::create` also force-removes stale worktrees on retry.
- Branch already exists: worktrees are created with `-B`, which resets the branch to the base commit — do not reuse `agentmesh/*` branches for manual work.
- Conflict detection uses `git merge-tree --write-tree` (Git ≥ 2.38) and falls back to the classic `git merge-tree <base> <a> <b>` on older Git.

**Remote machine connection problems**
- `nc -zv <coordinator-ip> 4222` from the remote machine. If it fails: firewall on the coordinator host, wrong IP, or Docker publishing only on a specific interface. Compose publishes `4222:4222` on all interfaces by default.
- Over Tailscale/WireGuard use the VPN IP, not the LAN IP.

**LLM / API configuration problems**
- Status bar `AI Planning error: …` with `AI_PROVIDER=anthropic` → check `ANTHROPIC_API_KEY`, network, and `AI_MODEL`. Malformed model output is rejected by `PlanValidator` and shown in the status bar; the coordinator does not crash.
- Plans always look generic → you are on the mock planner (`AI_PROVIDER` unset/`mock`, or missing key fallback).
- `Unsupported AI_PROVIDER` → only `mock` and `anthropic` exist; `openai`/`gemini` are not implemented.

---

## 20. Current Status

### Implemented (in this repository)

- Human-gated planning loop: project input → LLM plan (mock/Anthropic) → DAG validation → per-task approve/edit/reject → overlap acknowledgement.
- Transactional assignment with `FOR UPDATE SKIP LOCKED`, `TaskDelivery` outbox, idempotency keys, JetStream WorkQueue delivery, in-band rollback on publish failure.
- Full agent protocol with two adapters (`agent-mock`, `agent-agy`), registration with key verification, heartbeats, 30 s offline detection, event ingestion with authorization and deduplication.
- Repository-aware planning (`RepositoryScanner`), capability matching, complexity estimation, replanning engine.
- Git identity/branch/worktree/resource-tracking/merge-conflict/completion modules.
- Security: hashed `am_ak_` keys, roles, permission boundaries, task authorization, audit log, secret redaction, TLS-capable NATS client.
- Observability: timelines, delivery diagnostics, failure diagnostics, metrics, coordinator events, Diagnostics TUI screen.
- Reliability: startup recovery, stale task sweeper, delivery reconciliation, deduplication, reconnect helpers.
- 5 SQL migrations applied automatically; Docker Compose stack; 4-screen Ratatui TUI; headless daemon mode; standalone demo mode.

### Tested (automated)

- 197 tests across the workspace (see §17). On the maintainer machine on 2026-09-19 with Docker services running: **196 passed**, and `test_one_real_agy_binary_instance` (the only test that invokes the real `agy` binary) **failed**. Running `agy` by hand showed the cause: `result.status = "ERROR"` with `error: "API error … RESOURCE_EXHAUSTED (code 429): Individual quota reached"` — an `agy` account quota limit, not a coordinator defect. Earlier runs recorded by the maintainer (`docs/development/v1-validation.md`) report the full suite passing. Treat this test as environment-dependent.
- Integration suites cover: JetStream redelivery on NAK; approval gating; dependency gating; concurrency locking; overlap gating; worktree isolation with parallel delivery; capability matching and health gating; authentication, revocation, impersonation, permission boundaries, audit; timelines, diagnostics, metrics; restart recovery, stale reclamation, deduplication, crash/reassign cycles; a full lifecycle demo with two mock agents.

### Real-world validated

- One real `agy` instance executing a trivial task end-to-end through NATS (the `phase8` real-binary test, when it passes) and two `agent-agy` processes registering concurrently (`scripts/test_two_agy_instances.sh`), both on a single machine.
- Interactive TUI flow with two `agent-mock` processes on one machine.

### Experimental (not independently validated by the automated suite)

- Two physically separate machines: the code path is just `NATS_URL`, and `phase15_4` registers multiple agents with distinct identities, but the automated tests run on one host. Follow §8 and verify with the SQL query there.
- Real `agy` on non-trivial coding tasks inside coordinator-created worktrees.
- Anthropic planning quality on real repositories (mock planner is the default and the only provider used in tests).
- TLS/mTLS NATS, user/password auth (library only).

### Future / not implemented

- Background assignment/sweeper/recovery loops in the coordinator binary (today: assignment on approve; sweeper/recovery via library).
- Automatic worktree creation and completion handling from the interactive binary.
- TUI actions for reassign, cancel, replan, and live metrics streaming into the Diagnostics screen.
- Coordinator publishing `TaskCancelled` / `WaitForDependency`; adapters acting on them.
- OpenAI / Gemini providers; NATS server-side account permissions; admin approval of new agent registrations; a `coordinator` Docker image.

AgentMesh v1.0 is a complete, tested coordination core and a working `agy` adapter. It is **not** production-ready as shipped — see §15.

---

## 21. Documentation

```text
docs/
├── README.md                          # index
├── architecture/
│   ├── architecture.md                # system overview, design principles, v1.0 additions
│   └── domain-model.md                # entities, task/agent/delivery state machines, invariants
├── protocols/
│   └── agent-protocol.md              # NATS subjects, streams, message schemas, registration, idempotency
├── deployment/
│   └── multi-machine.md               # LAN / VPN deployment walkthrough and verification
├── development/
│   └── v1-validation.md               # Phase 15 validation report and test matrix
└── decisions/                         # ADR 001–012
```

- [`docs/README.md`](docs/README.md)
- [`docs/architecture/architecture.md`](docs/architecture/architecture.md)
- [`docs/architecture/domain-model.md`](docs/architecture/domain-model.md)
- [`docs/protocols/agent-protocol.md`](docs/protocols/agent-protocol.md)
- [`docs/deployment/multi-machine.md`](docs/deployment/multi-machine.md)
- [`docs/development/v1-validation.md`](docs/development/v1-validation.md)
- [`docs/decisions/`](docs/decisions/)

---

> **AI suggests. Humans decide. Agents execute.**
> AgentMesh coordinates. PostgreSQL is the source of truth. NATS transports the work. The agent protocol keeps runtimes interchangeable.
