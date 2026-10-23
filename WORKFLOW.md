# AgentMesh: Normal Two-User Workflow

This document describes the normal development workflow for two people using
AgentMesh:

- **User 1 — coordinator operator:** runs PostgreSQL/NATS and the AgentMesh
  coordinator, creates projects, reviews plans, approves work, and monitors
  progress.
- **User 2 — coding-agent operator:** runs `agent-agy` and the authenticated
  `agy` CLI. This user owns the machine and repository clone where coding work
  executes.

AgentMesh does not provide the `agy` CLI or its authentication. Both users
must use trusted machines and must not place secrets in Git, chat, task
descriptions, logs, or this document.

## 1. Before starting

Both users need:

- Git.
- Rust and Cargo if running AgentMesh from source.
- An installed and authenticated `agy` CLI on the worker machine.
- A reachable NATS endpoint between the two machines.
- The target repository cloned on the worker machine.

The coordinator machine additionally needs Docker Engine and the Docker
Compose plugin. The bundled services are PostgreSQL 16 and NATS 2.10.

The worker repository path must be usable by the worker. AgentMesh sends a
repository path in the task assignment; it does not copy or synchronize a
repository between machines.

## 2. First-time setup

### User 1: coordinator machine

Clone and configure AgentMesh:

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cp .env.example .env
```

Edit `.env` and set private values. At minimum, verify:

```dotenv
DATABASE_URL=postgres://agentmesh:<database-password>@localhost:5432/agentmesh
POSTGRES_PASSWORD=<database-password>
NATS_URL=nats://localhost:4222
NATS_AUTH_TOKEN=<shared-nats-token>
AI_PROVIDER=agy
```

The default `agy` planner requires an authenticated `agy` CLI on the
coordinator machine too. Alternatively, set `AI_PROVIDER=anthropic` and
provide `ANTHROPIC_API_KEY` through the coordinator environment.

Start and wait for infrastructure:

```bash
docker compose up -d --wait
cargo build --workspace
```

Run safe diagnostics before inviting the worker:

```bash
cargo run --bin coordinator -- --doctor
```

A `Degraded` result with `No registered coding workers` is expected until User
2 starts the worker. Doctor does not run migrations, execute tasks, modify
repositories, restart services, or expose credentials. Normal coordinator
startup applies migrations and creates the required JetStream streams.

### User 2: worker machine

Clone the target repository at the path where work should execute. Clone
AgentMesh if building the worker from source:

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cargo build --release --bin agent-agy
```

Install and authenticate `agy` using that tool's own instructions. Verify it
without printing credentials:

```bash
command -v agy
agy --help
```

Set worker variables in the worker shell. `agent-agy` does **not** load the
coordinator `.env` file:

```bash
export NATS_URL="nats://<coordinator-host-or-vpn-ip>:4222"
export NATS_AUTH_TOKEN="<same-token-as-coordinator>"
export AGY_AGENT_OWNER="<user-2-name>"
export AGY_AGENT_API_KEY="am_ak_<64-hex-characters>"
export AGY_BIN_PATH="/absolute/path/to/agy" # omit when agy is on PATH
export AGY_WORKSPACE_DIR="/absolute/path/to/target-repository"
export AGY_EFFORT="medium"
export AGY_TIMEOUT_SECS=600
```

Use a unique `AGY_AGENT_API_KEY` for this worker. If a stable identity is
needed across restarts, set `AGY_AGENT_ID` to a UUID and keep the same ID/key
pair. Do not paste real values into documentation or commit them.

## 3. Start the normal two-user session

### User 1: start the coordinator

From the AgentMesh checkout:

```bash
docker compose up -d --wait
cargo run --bin coordinator -- --tui
```

Keep this terminal open. Coordinator logs go to stderr; if needed, capture
them without corrupting the TUI:

```bash
cargo run --bin coordinator -- --tui 2> coordinator.log
```

### User 2: start the worker

From the AgentMesh checkout, after exporting the variables:

```bash
cargo run --bin agent-agy
```

The worker registers with the coordinator, sends heartbeats approximately
every five seconds, waits for assignments, runs `agy` in the assigned
repository/worktree, and reports progress and completion or failure.

User 1 can confirm registration by opening Fleet Inspector with `4`, or by
running Doctor again with `7` and `R`. A registered worker is not proof that a
real coding task will succeed; the worker must also have a valid repository,
`agy` installation, authentication, model access, and permissions.

## 4. Submit and approve work

User 1 performs these steps in the coordinator TUI:

1. Press `I` or `N` to open project input.
2. Enter a project name.
3. Press `Tab` and enter a specific development goal.
4. Press `Enter` to submit the project to the configured planner.
5. Review the generated tasks on Plan Review.
6. Inspect dependencies and affected resources. Press `G` for the DAG.
7. Press `Y` to approve a task, or use the bulk approval flow only after
   reviewing the complete proposal.
8. Wait for approved and unblocked tasks to be assigned to an eligible worker.

The human approval gate is required in the normal flow. Do not use
`--auto-approve` unless the environment and repository are explicitly
controlled for unattended operation.

The planner may inspect the coordinator's current repository context. Confirm
that the intended repository and working directory are correct before
submitting a goal.

## 5. Monitor execution

### User 1

- `1` — Dashboard and live task summary.
- `3` — Plan Review and task details.
- `4` — Fleet Inspector and worker status.
- `5` — Existing task/event diagnostics.
- `7` — Doctor readiness checks; press `R` to rerun.
- `?` — Help and current keyboard shortcuts.
- `q` or `Ctrl-C` — Quit the coordinator.

Watch for these task states:

```text
Proposed -> HumanReview -> Approved -> Assigned -> Executing -> Completed
                                            \-> Failed/Blocked/Cancelled
```

Dependencies prevent a task from being assigned until its prerequisites
complete. If a worker becomes unavailable, the coordinator's recovery and
stale-task logic may reclaim eligible work according to the persisted
delivery state.

### User 2

Keep the worker terminal running and watch its logs for registration,
assignment, `agy` subprocess errors, repository-path errors, and completion.
Do not start two workers with the same stable identity. Stop the worker only
after its assigned task reaches a safe terminal state or User 1 has confirmed
that recovery is acceptable.

## 6. Normal troubleshooting

Run these checks in order:

```bash
docker compose ps
cargo run --bin coordinator -- --doctor
cargo run --bin coordinator -- --status
```

| Symptom | Check |
| --- | --- |
| PostgreSQL connection failure | Run `docker compose up -d --wait`; verify `DATABASE_URL` and the password. |
| NATS authorization violation | Ensure both users use the exact `NATS_AUTH_TOKEN` used when the NATS container started. |
| Worker does not appear | Verify User 2 can reach `NATS_URL`, the worker process is running, and its API key is valid and unique. |
| `agy` not found | Install/authenticate `agy` or set `AGY_BIN_PATH` to its absolute path. |
| Task fails immediately | Verify the target repository exists on the worker and `AGY_WORKSPACE_DIR` or the assigned path is correct. |
| No task is assigned | Confirm the task is approved, all dependencies are complete, and the worker is healthy and idle. |

For a remote worker, prefer a trusted LAN, WireGuard, Tailscale, or an SSH
tunnel. The bundled NATS setup uses a shared token but does not enable TLS.
Do not expose port 4222 directly to the public internet.

## 7. Stop and clean up

User 2 should stop `agent-agy` with `Ctrl-C` after work is complete. User 1
should stop the coordinator with `q` or `Ctrl-C`.

Stop infrastructure while retaining local data:

```bash
docker compose down
```

Delete all local PostgreSQL and NATS data only when explicitly intended:

```bash
docker compose down -v
```

The `-v` command permanently removes local projects, tasks, events, agent
registrations, deliveries, and JetStream messages.

## 8. Complete keyboard reference

| Key | Action |
| --- | --- |
| `1`–`7` | Open a screen |
| `Tab`, `BackTab`, `[`, `]` | Cycle screens |
| `?` / `Esc` | Open or close help |
| `:` | Open command palette |
| `q` / `Ctrl-C` | Quit |
| `I` / `N` | Project input |
| `D` | Submit the built-in demo project |
| `Enter` | Submit, load, toggle, or inspect based on focus |
| `Y` / `N` | Approve or reject the focused task |
| `E` | Edit task description |
| `R` | Retry a failed/cancelled task; rerun Doctor on Doctor screen |
| `C` | Cancel a task or inspect an agent, depending on screen |
| `Shift-A` / `Ctrl-A` | Bulk approval flow |
| `G` | Toggle the task DAG |
| `Space` | Select a task for selective bulk approval |
| Arrow keys / `h` `j` `k` `l` | Navigate lists and panes |

AgentMesh is keyboard-only. Mouse capture, clicks, and wheel events are not
supported.

