# AgentMesh

> **AI suggests → humans decide → real coding agents execute → the coordinator enforces**

AgentMesh coordinates multiple real coding-agent processes working on the same software project. It turns a development goal into a validated dependency graph, pauses for human approval, assigns approved work over NATS JetStream, and records lifecycle state in PostgreSQL. Each dispatched task receives an isolated Git worktree when the target repository supports Git.

AgentMesh is not a replacement for an agent runtime. The included `agent-agy` binary is an adapter that connects an authenticated `agy` CLI process to AgentMesh. No mock agent is included in the runtime.

## What it provides

- Repository-aware planning through the configured `agy` CLI or Anthropic API.
- Deterministic task and dependency validation before review.
- Keyboard-only Ratatui TUI for project creation, plan review, fleet status, and diagnostics.
- Explicit human approval before a task can be assigned.
- NATS JetStream delivery with acknowledgements, idempotency, heartbeats, and recovery.
- PostgreSQL-backed task, agent, delivery, event, overlap, and audit state.
- Safe Doctor diagnostics available as `coordinator --doctor` or TUI screen `7`.
- Git worktree isolation, resource tracking, merge checks, and completion finalization.
- Real `agent-agy` workers on the same machine or on reachable worker machines.

## Architecture

```text
Operator (keyboard)
        │
        ▼
Coordinator TUI and orchestration engine
   ┌────┼──────────┬─────────────┐
   ▼    ▼          ▼             ▼
Planner  PostgreSQL  NATS        Git coordinator
         state       JetStream   worktrees
                              │
                              ▼
                    agent-agy → authenticated agy CLI
```

PostgreSQL is authoritative for state. NATS is the transport and durable delivery layer. The coordinator runs migrations and creates the required JetStream streams at startup. Workers do not read `.env`; their required variables must be exported in the worker process environment.

## Status and limitations

Implemented and wired into the coordinator: planning, DAG validation, human approval, capability and health filtering, task assignment, heartbeats, delivery recovery, stale-task sweeping, event timelines, diagnostics, API-key registration, permission checks, audit logging, and Git worktree preparation/finalization.

Important limits:

- Only `agy` and `anthropic` planner providers are implemented. OpenAI, Gemini, and other providers are not.
- Real task execution requires an installed and authenticated `agy` CLI. AgentMesh does not provide that CLI or its credentials.
- Remote workers receive repository paths in task specifications; AgentMesh does not synchronize repositories between machines. The repository must exist at a usable path on the worker host.
- The shipped Docker NATS setup uses a shared token and does not enable TLS. Keep it on a trusted network or use a VPN/SSH tunnel.
- TUI input is keyboard-only; mouse capture, clicks, and wheel events are not supported.
- `--auto-approve` is intended for headless operation and bypasses the normal interactive approval step. Use it only in a controlled environment.

## Prerequisites

- Rust toolchain (`cargo`, `rustc`) compatible with the workspace.
- Git.
- Docker Engine and Docker Compose plugin for the bundled PostgreSQL 16 and NATS 2.10 services.
- An authenticated `agy` CLI for the default planner and workers, or an Anthropic API key when using `AI_PROVIDER=anthropic`.
- A Git repository for worktree isolation and repository-aware execution.

Linux and macOS are the documented environments. Other platforms are unverified.

## Install and quick start

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cp .env.example .env
# Edit .env: set real secrets and choose the planner.
docker compose up -d --wait
cargo build --workspace
```

Start the interactive coordinator in one terminal:

```bash
cargo run --bin coordinator -- --tui
```

Start a real worker in another terminal. The worker reads process environment variables:

```bash
export NATS_URL="nats://localhost:4222"
export NATS_AUTH_TOKEN="<same token configured for NATS>"
export AGY_AGENT_OWNER="Local Worker"
export AGY_AGENT_API_KEY="am_ak_<64-hex-characters>"
export AGY_BIN_PATH="/path/to/agy" # omit when agy is on PATH
cargo run --bin agent-agy
```

For a guided setup, use `./install.sh`. For installing compiled binaries, use `./scripts/install.sh`. Both scripts support `--help`; inspect the output before using them on a production host.

## Configuration

Copy `.env.example` to `.env`. The coordinator loads it automatically with `dotenvy`; `agent-agy` does not.

| Variable | Used by | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | Coordinator/tests | PostgreSQL connection string |
| `POSTGRES_PASSWORD` | Docker Compose | Password for the local PostgreSQL container |
| `NATS_URL` | Coordinator/worker | NATS server URL |
| `NATS_AUTH_TOKEN` | Coordinator/worker/Docker | Shared NATS authentication token |
| `AI_PROVIDER` | Coordinator | `agy` or `anthropic`; default `agy` |
| `AI_MODEL` | Coordinator | Model passed to the selected planner |
| `ANTHROPIC_API_KEY` | Coordinator | Required only for `AI_PROVIDER=anthropic` |
| `RUST_LOG` | All binaries | `tracing` filter |
| `AGY_AGENT_OWNER` | Worker | Human-readable worker owner |
| `AGY_AGENT_ID` | Worker | Optional stable UUID |
| `AGY_AGENT_API_KEY` | Worker | Registration key |
| `AGY_BIN_PATH` | Worker | Optional path to `agy` |
| `AGY_MODEL` / `AGY_EFFORT` | Worker | Optional runtime model/effort |
| `AGY_WORKSPACE_DIR` | Worker | Optional local repository base directory |
| `AGY_TIMEOUT_SECS` | Worker | Subprocess timeout, default 600 seconds |

Never commit `.env`, API keys, NATS tokens, or worker keys.

## Coordinator modes

```bash
cargo run --bin coordinator -- --help
cargo run --bin coordinator -- --tui
cargo run --bin coordinator -- --headless
cargo run --bin coordinator -- --project "<UUID or project name>"
cargo run --bin coordinator -- --status
cargo run --bin coordinator -- --doctor
```

Without an attached TTY, the coordinator selects headless mode unless `--tui` is specified. Use `--status` to print a database-backed summary and exit.

## TUI essentials

The TUI is keyboard-only. The main screens are Dashboard, Project Management/Input, Plan Review, Fleet Inspector, Diagnostics, Settings, and Doctor.

| Key | Action |
| --- | --- |
| `1`–`7` | Jump to a screen |
| `Tab`, `BackTab`, `[`, `]` | Cycle screens |
| `?` | Open help |
| `:` | Open command palette |
| `q` / `Ctrl-C` | Quit |
| `I` / `N` | Start project input |
| `Enter` | Submit/load/inspect depending on the focused screen |
| `D` | Submit the built-in demo project through the configured planner |
| `Y` / `N` | Approve/reject the focused review task |
| `E` | Edit the focused task description |
| `R` | Retry a failed or cancelled task |
| `C` | Cancel an active task or inspect an agent, depending on screen |
| `Shift-A` / `Ctrl-A` | Bulk approval flow |
| `G` | Toggle the plan DAG view |
| Arrow keys / `h`,`j`,`k`,`l` | Navigate lists and panes |
| `R` on Doctor | Re-run safe diagnostic checks |

The detailed workflow, safety notes, environment reference, and troubleshooting are in the **[User Guide](docs/USER_GUIDE.md)**.
For the normal two-user setup, see the **[Two-User Workflow](WORKFLOW.md)**.

## Documentation

- [User Guide](docs/USER_GUIDE.md)
- [Two-User Workflow](WORKFLOW.md)
- [Architecture](docs/architecture/architecture.md)
- [Domain model](docs/architecture/domain-model.md)
- [Agent protocol](docs/protocols/agent-protocol.md)
- [Multi-machine deployment](docs/deployment/multi-machine.md)
- [Development validation](docs/development/v1-validation.md)
- [Documentation index](docs/README.md)
- [TODO and project state](TODO.md)

## Validation

```bash
cargo fmt --all -- --check
cargo check --workspace
DATABASE_URL="postgres://agentmesh:<password>@localhost:5432/agentmesh" \
  cargo test --workspace
```

Tests that use SQLx need a reachable PostgreSQL database with migrations applied. The full suite may include timing-sensitive reliability tests; run a failing reliability test serially when diagnosing an isolated timing failure.

## Cleanup

Stop services without deleting data:

```bash
docker compose down
```

Delete local PostgreSQL and NATS data permanently:

```bash
docker compose down -v
```

The second command is destructive. It removes local projects, tasks, events, agent registrations, deliveries, and JetStream messages.

## License

MIT. See the repository history and architecture decision records for design rationale.
