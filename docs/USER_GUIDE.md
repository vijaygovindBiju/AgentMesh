# AgentMesh User Guide

This guide describes the current AgentMesh coordinator, the real `agent-agy`
worker, the keyboard-only terminal UI, and the local or multi-machine
deployment supported by the repository. The source code is authoritative if
this guide and an older document disagree.

## 1. Introduction and key concepts

AgentMesh coordinates coding work; it does not itself provide an AI coding
model. A typical run is:

1. An operator submits a project name and development goal.
2. The coordinator scans its repository and asks the configured planner for a
   proposed task breakdown.
3. The coordinator validates task IDs, dependencies, resource paths, and
   cycles.
4. The operator reviews the proposal and explicitly approves or rejects it.
5. Approved, unblocked tasks are assigned to eligible registered workers.
6. A worker runs the task in a task-specific Git worktree and sends lifecycle
   events back to the coordinator.

Important terms:

- **Project**: A development goal and its generated plan.
- **Task**: A unit of work with a status, dependencies, suggested resources,
  and optional assigned agent.
- **DAG**: The directed acyclic graph of task prerequisites. A task is not
  eligible while one of its prerequisites is incomplete.
- **Agent**: A registered worker process. The supported executable is
  `agent-agy`, an adapter around an authenticated `agy` CLI.
- **Proposal**: Planner output before it becomes an approved task plan.
- **Delivery**: The durable assignment record connecting a task to a worker
  and an attempt.
- **Worktree**: The isolated Git checkout prepared for a dispatched task.

## 2. Architecture and component roles

```text
Operator
  │ keyboard
  ▼
Coordinator TUI / command handler
  ├── planner and DAG validator
  ├── task assignment and recovery loops
  ├── Git worktree coordinator
  ├── PostgreSQL state
  └── NATS JetStream transport
                         │
                         ▼
             agent-agy → agy CLI → coding process
```

- **Coordinator** (`crates/coordinator`): Owns planning, state transitions,
  assignment, recovery, security checks, observability, and the TUI.
- **PostgreSQL 16**: Authoritative storage for projects, tasks, dependencies,
  agents, deliveries, proposals, approvals, events, overlaps, and audit
  records. Migrations run at coordinator startup.
- **NATS 2.10 JetStream**: Durable transport for task assignments and agent
  events. It is not the source of truth.
- **Agent protocol** (`crates/agent-protocol`): Transport-independent message
  types and capability/security definitions.
- **`agent-agy`** (`crates/agent-agy`): Registers, heartbeats, consumes
  assignments, runs `agy`, and publishes progress/completion/failure events.
- **Git coordinator**: Creates `agentmesh/<short-id>` branches and
  `.agentmesh/worktrees/<short-id>` worktrees when a repository is available,
  then audits and finalizes completed work.

## 3. Prerequisites and supported environments

Install:

- Rust and Cargo.
- Git.
- Docker Engine and the Docker Compose plugin for the bundled services.
- An installed, authenticated `agy` CLI for the default `agy` planner and
  worker execution, or an Anthropic API key for the `anthropic` planner.

Linux and macOS are documented. Other operating systems are unverified.
For remote operation, use a trusted LAN, WireGuard, Tailscale, or an SSH
tunnel. Do not expose the bundled unauthenticated-by-TLS NATS endpoint directly
to the public internet.

## 4. Installation and initial setup

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cp .env.example .env
```

Edit `.env` before starting anything that will leave the local machine. At
minimum, choose strong values for `POSTGRES_PASSWORD` and `NATS_AUTH_TOKEN`
when the services are not strictly local. Do not commit `.env`.

Build the workspace:

```bash
cargo build --workspace
```

Alternatively:

```bash
./install.sh                 # checks prerequisites, starts Docker, builds
./install.sh --release       # optimized build
./scripts/install.sh         # installs release binaries into ~/.local/bin
./scripts/install.sh --help
```

The scripts are convenience wrappers. Review their output and use
`--skip-docker`, `--skip-build`, `--debug`, `--prefix`, or `--agent-only` as
appropriate for the host.

## 5. PostgreSQL and NATS configuration

The bundled `docker-compose.yml` starts:

- PostgreSQL 16 on `localhost:5432`.
- NATS 2.10 with JetStream on `localhost:4222`.
- NATS monitoring on `localhost:8222`.

Start and wait for health checks:

```bash
docker compose up -d --wait
docker compose ps
```

The local connection values are represented by:

```dotenv
DATABASE_URL=postgres://agentmesh:<password>@localhost:5432/agentmesh
POSTGRES_PASSWORD=<same password>
NATS_URL=nats://localhost:4222
NATS_AUTH_TOKEN=<shared token>
```

The coordinator loads `.env`. The worker does not, so export its variables or
prefix its command. After changing a Docker environment value, recreate the
affected service. If the PostgreSQL password was changed after first startup,
the local volume may need to be removed and recreated:

```bash
docker compose down -v
docker compose up -d --wait
```

That command permanently deletes local database and JetStream data.

## 6. Planner and real agent runtime setup

The planner is selected with `AI_PROVIDER`:

| Value | Requirement | Status |
| --- | --- | --- |
| `agy` | Authenticated `agy` CLI available to the coordinator | Implemented/default |
| `anthropic` | `ANTHROPIC_API_KEY` and a supported model | Implemented |

`AI_MODEL` selects the configured model name. OpenAI, Gemini, and other
providers are not implemented in this workspace.

For a real worker, install and authenticate `agy` separately, then verify the
binary:

```bash
command -v agy
agy --help
```

If it is not on `PATH`, set `AGY_BIN_PATH`. AgentMesh does not document or
manage the upstream `agy` login flow; follow that tool's own instructions.

## 7. Registering and configuring coding agents

Start the coordinator before a worker so it can create JetStream streams and
accept registration:

```bash
cargo run --bin coordinator -- --tui
```

In another terminal, configure one worker:

```bash
export NATS_URL="nats://localhost:4222"
export NATS_AUTH_TOKEN="<same NATS token>"
export AGY_AGENT_OWNER="Local Worker"
export AGY_AGENT_API_KEY="am_ak_<64-hex-characters>"
export AGY_BIN_PATH="/absolute/path/to/agy" # optional
export AGY_EFFORT="medium"                  # optional
export AGY_TIMEOUT_SECS=600                  # optional
cargo run --bin agent-agy
```

Optional `AGY_AGENT_ID` is a UUID. Keep the ID and API key stable across
restarts when the same registration should be reused. Use a different ID/key
for a distinct worker. `AGY_WORKSPACE_DIR` can identify the worker's local
repository base directory.

The worker registers, sends heartbeats approximately every five seconds, and
creates a durable assignment consumer. The Fleet screen should show the
registered worker; `last_seen` and health determine whether it can receive
work.

## 8. Launching and navigating the TUI

Interactive mode:

```bash
cargo run --bin coordinator -- --tui
```

Useful non-interactive commands:

```bash
cargo run --bin coordinator -- --help
cargo run --bin coordinator -- --headless
cargo run --bin coordinator -- --status
cargo run --bin coordinator -- --project "<UUID or project name>"
```

The TUI has Dashboard, Project Management/Input, Plan Review, Fleet
Inspector, Diagnostics, and Settings screens. It does not capture mouse input.

| Key | Result |
| --- | --- |
| `1`–`6` | Jump to a screen |
| `Tab` / `BackTab` / `[` / `]` | Cycle screens |
| `?` / `Esc` | Open or close help |
| `:` | Open command palette |
| `q` / `Ctrl-C` | Quit |
| `I`, `N` | Open project input |
| `D` | Submit the built-in demo project |
| `Enter` | Submit, load, toggle, or inspect according to focus |
| Arrow keys or `h/j/k/l` | Navigate lists and panes |
| `Y` | Approve focused task |
| `N` | Reject focused task or open a cancellation confirmation |
| `E` | Edit a task description |
| `R` | Retry failed/cancelled task |
| `C` | Cancel a task or inspect an agent, depending on screen |
| `Shift-A` / `Ctrl-A` | Start bulk approval |
| `G` | Toggle the plan DAG |
| `Space` | Select a task for selective bulk approval |

The help overlay and footer are the final authority for context-specific
shortcuts.

## 9. Creating a project and submitting a goal

1. Open Project Input with `I` or `N`.
2. Enter a project name.
3. Press `Tab` to move to the description.
4. Enter a concrete development goal, constraints, and important paths.
5. Press `Enter` to submit.

The coordinator persists the project and invokes the configured planner. The
planner receives repository context generated by the repository scanner. A
project submission can therefore read the coordinator's current repository;
review the selected repository and working directory before submitting.

The `D` shortcut submits the built-in demo project through the configured
planner. It is an example workflow, not a substitute for connecting a real
coding agent.

## 10. Repository inspection and plan generation

At planning time the repository scanner collects repository structure,
ecosystems, languages, crates, file paths, and README context. The planner
returns task titles/descriptions, dependencies, affected resources, complexity,
and suggested capabilities.

The coordinator then rejects invalid plans such as duplicate IDs, unknown
dependency references, self-dependencies, or cycles. Planning failures appear
as a TUI status message and in coordinator logs. No task should be considered
executable merely because the planner returned it.

## 11. DAG and dependencies

The Plan Review screen shows tasks and their prerequisite relationships. A
task remains blocked until all dependencies complete. The assignment loop
periodically looks for approved, unblocked tasks and eligible idle/healthy
agents.

Use `G` to view the ASCII DAG. Inspect the task details pane for dependencies,
affected resources, overlap warnings, and status. Dependency completion events
can automatically unblock a waiting task.

## 12. Review, edit, approve, reject, retry, and cancel

The normal lifecycle is:

```text
Proposed → HumanReview → Approved → Assigned → Executing → Completed
                                      └──────→ Failed/Blocked/Cancelled
```

During Plan Review:

1. Move to a task with arrows or `j/k`.
2. Press `Y` to approve it.
3. Press `N` to reject it; destructive decisions use confirmation.
4. Press `E`, edit the description, then press `Enter` to submit the edited
   approval.
5. Use `Space` to select several tasks, then `Shift-A` or `Ctrl-A` for the
   guarded selective approval flow. With no selection, the bulk flow targets
   all pending tasks.
6. Use `R` only for failed or cancelled work. It resets the task to an
   approval-ready state; it does not silently execute it.
7. Use `C` to request cancellation of an active/assigned task and confirm it.

Critical resource overlaps require explicit acknowledgement before approval.
Approval is the human safety boundary: planner output and dynamic replans do
not bypass it.

## 13. Assignment, execution, and progress

After approval, the coordinator checks dependencies, agent health, capability
matching, permissions, and resource conflicts. It creates a delivery record,
prepares a worktree when possible, and publishes a `TaskAssignment` over
JetStream.

The worker:

1. Receives and transport-acknowledges the assignment.
2. Runs `agy` with the task specification and repository path.
3. Publishes started, progress, blocked, completed, or failed events.

The Dashboard and Diagnostics screens show task status, agent health, recent
events, delivery information, and timelines. A worker that stops heartbeating
can be marked stale; the recovery and stale-sweeper loops reconcile deliveries
and reclaim eligible work according to the persisted state.

## 14. Multi-agent and multi-device operation

Multiple workers can connect to one coordinator if they can reach NATS and
have distinct identities/keys. For remote workers, expose only NATS client
port 4222 through a trusted network path; keep PostgreSQL and monitoring local
to the coordinator host.

Remote operation does **not** synchronize source code. The repository and any
required local tools must exist on the worker host. The task's repository path
is interpreted by that worker. Use an identical clone layout, a shared
filesystem, or set `AGY_WORKSPACE_DIR` to the worker's local clone.

Supported network approaches documented in the repository:

- Same trusted LAN.
- WireGuard or Tailscale.
- SSH local port forwarding.

The bundled NATS configuration uses a shared token without TLS. NATS TLS/mTLS
helpers exist in library code, but the shipped coordinator and worker binaries
do not expose a TLS configuration workflow.

See [multi-machine deployment](deployment/multi-machine.md) for firewall and
verification details.

## 15. Approval, permissions, recovery, and troubleshooting

### Approval and permissions

Human approval is required before normal assignment. Registration and event
ingestion authenticate agent keys; task authorization prevents an agent from
acting on another agent's task. Permission boundaries restrict paths and
credentials are redacted from logs. Review affected resources and repository
paths before approving.

### Recovery

Coordinator startup runs crash recovery and delivery reconciliation. Background
loops process heartbeats, pending deliveries, dependency unblocking, and stale
tasks. Failed and cancelled tasks can be retried from the Plan Review screen,
but retry still returns them to an approval-ready state.

### Common problems

| Symptom | Check |
| --- | --- |
| PostgreSQL connection failure immediately after Docker start | Use `docker compose up -d --wait`; verify `DATABASE_URL` and password |
| NATS authorization violation | Ensure all clients use the exact `NATS_AUTH_TOKEN` used to start the container |
| Worker cannot register | Start the coordinator first, verify port 4222, owner/key variables, and worker logs |
| `agy` not found | Install/authenticate `agy` or set `AGY_BIN_PATH` |
| Worker is offline | Check its process, NATS reachability, token, and heartbeat logs |
| Approved task never runs | Check dependency blockers, worker health/capability, assignment state, and overlap acknowledgement |
| Task fails immediately | Verify the repository exists at the worker's path and that Git/`agy` permissions are available |
| TUI is not shown | Use a real terminal and `--tui`; stdout without a TTY selects headless mode |

## 16. Logs, diagnostics, and observability

The coordinator writes tracing logs to stderr. Keep them separate from the TUI:

```bash
cargo run --bin coordinator -- --tui 2> coordinator.log
RUST_LOG=coordinator=debug,agentmesh=debug cargo run --bin coordinator -- --headless
```

The Diagnostics screen exposes recent coordinator events, event streams,
severity/entity filters, task timelines, and payload drilldown. The
coordinator also persists lifecycle and security events in PostgreSQL.

Useful local checks:

```bash
docker compose ps
curl -fsS http://localhost:8222/healthz
curl -fsS http://localhost:8222/jsz
docker compose exec postgres psql -U agentmesh -d agentmesh -c \
  "SELECT id, human_owner, status, health_status, last_seen FROM agents;"
docker compose exec postgres psql -U agentmesh -d agentmesh -c \
  "SELECT short_id, status, assigned_agent_id FROM tasks ORDER BY created_at;"
```

Do not paste tokens, API keys, or sensitive repository content into issue
reports or logs.

## 17. Shutdown, cleanup, and repository safety

Quit the TUI with `q` or `Ctrl-C`. Stop a worker with `Ctrl-C`, allowing its
process to exit cleanly. Stop local services without deleting state:

```bash
docker compose down
```

To permanently remove local PostgreSQL and NATS state:

```bash
docker compose down -v
```

Task execution can modify the worker's repository worktree, run commands
through `agy`, create branches/worktrees, stage or commit changes during
finalization, and produce merge/conflict records. Inspect the target
repository, branch, worktree, and proposed affected resources before approval.
Do not point a worker at a valuable checkout without backups and a clear
cleanup policy.

## 18. Known limitations and further reading

Known limitations are:

- Real `agy` installation/authentication is external to this repository.
- Only `agy` and Anthropic planner providers are available.
- Remote repository synchronization is not provided.
- Bundled NATS security is token-based without TLS.
- Mouse interaction is intentionally unsupported.
- Settings and some advanced command-palette actions are available in the TUI,
  but this guide only documents behavior verified in the source.

Further reading:

- [README](../README.md)
- [Architecture](architecture/architecture.md)
- [Domain model](architecture/domain-model.md)
- [Agent protocol](protocols/agent-protocol.md)
- [Multi-machine deployment](deployment/multi-machine.md)
- [Development validation](development/v1-validation.md)
- [Architecture decisions](decisions/)
