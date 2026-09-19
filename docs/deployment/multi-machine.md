# AgentMesh — Multi-Machine Deployment

This guide covers running the coordinator on one machine and agents (`agent-mock` or `agent-agy`) on others. It documents what the v1.0 binaries actually support. For the security caveats that apply to any deployment beyond a trusted LAN, see the "Security" section of the [README](../../README.md#15-security).

---

## Topology

```text
                 NETWORK (LAN / WireGuard / Tailscale)
                    │
        ┌───────────┴───────────────────────┐
        │                                   │
        ▼                                   ▼
   COORDINATOR HOST                    AGENT HOST(S)
   ────────────────                    ─────────────
   coordinator (TUI or headless)       agent-agy  or  agent-mock
   PostgreSQL  :5432  (local only)       NATS_URL=nats://<coordinator-ip>:4222
   NATS        :4222  (exposed)          NATS_AUTH_TOKEN=<shared token>
               :8222  (local only)       AGY_AGENT_API_KEY / MOCK_AGENT_API_KEY
   optional local agents
```

Only **NATS port 4222** must be reachable from agent hosts. PostgreSQL is used exclusively by the coordinator, and NATS monitoring (8222) should stay local.

---

## 1. Coordinator host

```bash
cd AgentMesh
cp .env.example .env
```

Edit `.env` before exposing NATS:

```dotenv
NATS_AUTH_TOKEN=<long random secret>   # every client must present this
POSTGRES_PASSWORD=<something other than agentmesh_dev>
DATABASE_URL=postgres://agentmesh:<same password>@localhost:5432/agentmesh
```

Start services and the coordinator:

```bash
docker compose up -d           # re-run after changing NATS_AUTH_TOKEN so the NATS container is recreated
docker compose ps              # both healthy
cargo run --bin coordinator    # interactive TUI
# or headless (only registration / heartbeat / event ingestion; no TUI actions):
cargo run --bin coordinator > coordinator.log 2>&1
```

The coordinator applies migrations and creates the `TASK_ASSIGNMENTS` and `AGENT_EVENTS` JetStream streams on startup. Agents fail with `TASK_ASSIGNMENTS stream not found` if they start against a NATS server the coordinator has never connected to.

### Firewall

Allow inbound TCP 4222 from agent hosts only. Examples:

```bash
# ufw
sudo ufw allow from 192.168.1.0/24 to any port 4222 proto tcp
# Tailscale subnet
sudo ufw allow from 100.64.0.0/10 to any port 4222 proto tcp
```

### Find the address to give agents

```bash
hostname -I            # LAN IP(s)
tailscale ip -4        # Tailscale IP, if using Tailscale
wg show                # WireGuard peers/addresses
```

---

## 2. Agent host

Build only the agent binary (a full clone is required for the workspace):

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git
cd AgentMesh
cargo build --release --bin agent-agy      # or --bin agent-mock
```

Agents read **only the process environment** (they do not load `.env`).

### agent-agy

```bash
NATS_URL="nats://192.168.1.50:4222" \
NATS_AUTH_TOKEN="<shared token>" \
AGY_AGENT_ID="$(uuidgen)" \
AGY_AGENT_OWNER="Friend (Frontend)" \
AGY_AGENT_API_KEY="<this agent's secret>" \
AGY_EFFORT="medium" \
AGY_TIMEOUT_SECS=900 \
./target/release/agent-agy
```

- `agy` must already be installed and authenticated on this host (`AGY_BIN_PATH` if it is not at `~/.local/bin/agy` or on `PATH`).
- Record `AGY_AGENT_ID` and `AGY_AGENT_API_KEY`: the same pair is required to re-register after a restart. A different key for a known ID is rejected.
- Use a key of the form `am_ak_<64 hex>` if you want it stored hashed (`ApiKeyManager::generate_key` produces this format; `openssl rand -hex 32` also works: `AGY_AGENT_API_KEY="am_ak_$(openssl rand -hex 32)"`).

### agent-mock

```bash
NATS_URL="nats://192.168.1.50:4222" NATS_AUTH_TOKEN="<shared token>" \
MOCK_AGENT_OWNER="Remote Mock" ./target/release/agent-mock
```

### Expected log lines

```text
INFO  Starting AgentMesh agy Adapter agent_id=… owner=…
INFO  agy agent successfully registered agent_id=… subject=agents.<id>.events
INFO  Listening for task assignments on JetStream
```

---

## 3. Verify from the coordinator host

```bash
docker compose exec postgres psql -U agentmesh -d agentmesh -c \
  "SELECT id, human_owner, adapter_type, status, health_status, last_seen FROM agents ORDER BY last_seen DESC;"
```

- `status = idle` and `last_seen` advancing every ~5 s → connected.
- `status = offline` → no heartbeat for 30 s.
- In the TUI, Dashboard (`3`) lists the fleet; press `r` to reload from PostgreSQL.

Registration attempts (success/denied) are in `audit_logs`:

```sql
SELECT timestamp, actor_id, action, status, details FROM audit_logs ORDER BY timestamp DESC LIMIT 10;
```

NATS-side check (coordinator host):

```bash
curl -s http://localhost:8222/connz | grep -c '"cid"'      # number of connected clients
curl -s "http://localhost:8222/jsz?consumers=true" | grep -o 'agent-agy-[0-9a-f]*' | sort -u
```

---

## 4. How work flows across machines

1. Human approves a task in the TUI → `AssignmentService` writes `TaskDelivery(pending)`, sets task `assigned` / agent `busy`, commits, then publishes `TaskAssignment` to `coordinator.tasks.assign.<agent_id>` (JetStream `TASK_ASSIGNMENTS`, file-backed WorkQueue).
2. The remote agent's durable pull consumer receives it (even if the agent was offline at publish time), transport-ACKs, runs the task.
3. The agent publishes `TaskStarted → ProgressUpdate… → Completed | Failed | Blocked` to `agents.<agent_id>.events` (JetStream `AGENT_EVENTS`).
4. The coordinator's durable `coordinator-events` consumer authorizes (`TaskAuthorizer`), deduplicates (`EventDeduplicator`) and persists each event, updating `tasks`, `agents`, `task_deliveries`, `agent_events`, and the TUI.

Git context: if `tasks.task_branch` / `repo_path` are set (via `GitCoordinator::prepare_task_workspace`, currently a library call), they are included in the `TaskAssignment`. The path is interpreted **on the agent host**, so a remote agent needs the repository (or worktree) at that same path, e.g. via a shared filesystem or an identical clone layout. The v1.0 binaries do not synchronize repositories between machines.

---

## 5. Network options

| Option | Works? | Notes |
| :--- | :--- | :--- |
| Same LAN | Yes | Use the coordinator's LAN IP. Plain TCP + token. |
| WireGuard / Tailscale | Yes | Use the VPN IP in `NATS_URL`. Traffic is encrypted by the VPN; NATS itself is still plain TCP + token. Recommended for anything off-LAN. |
| SSH tunnel | Yes | `ssh -N -L 4222:localhost:4222 user@coordinator` on the agent host, then `NATS_URL=nats://localhost:4222`. |
| Public internet, port-forwarded | Not recommended | The shipped stack has no TLS and a static shared token; registration of new agent IDs is open to anyone with the token. |
| NATS TLS / mTLS / user-password | Library only | `NatsSecurityConfig` + `connect_secure` exist in `crates/coordinator/src/messaging/client.rs`, but `coordinator`, `agent-mock` and `agent-agy` binaries only read `NATS_URL` and `NATS_AUTH_TOKEN`. Using TLS requires code changes plus a TLS-configured NATS server. |

---

## 6. Troubleshooting

| Symptom (agent host) | Cause / fix |
| :--- | :--- |
| `Failed to connect to NATS at nats://…` | Port 4222 not reachable: firewall, wrong IP, coordinator's Docker not running. `nc -zv <ip> 4222`. |
| `Authorization Violation` | `NATS_AUTH_TOKEN` mismatch. Remember the NATS container keeps the token it was started with — `docker compose up -d` after changing `.env`. |
| `TASK_ASSIGNMENTS stream not found` | Coordinator never connected to this NATS. Start the coordinator once. |
| `Registration request failed` (timeout) | Coordinator not running / not subscribed to `coordinator.agents.register`. |
| `Registration rejected: Invalid API key` | `*_AGENT_ID` already registered with a different key. Reuse the original key, pick a new ID, or fix the row (`UPDATE agents SET api_key_hash=… WHERE id=…`). |
| Registered, but never receives tasks | Task is blocked by a dependency, assigned to a different/seeded placeholder agent, or no assignment cycle has run since the blocker completed. Check `tasks.assigned_agent_id` and approve/edit a task in the TUI to trigger a cycle. |
| Shows `offline` in the fleet | Heartbeats not arriving: agent process died, network dropped, or large clock skew. |

Related: [`../protocols/agent-protocol.md`](../protocols/agent-protocol.md), [`../decisions/002-message-queue-nats-jetstream.md`](../decisions/002-message-queue-nats-jetstream.md), [`../decisions/010-security-and-audit-boundaries.md`](../decisions/010-security-and-audit-boundaries.md).
