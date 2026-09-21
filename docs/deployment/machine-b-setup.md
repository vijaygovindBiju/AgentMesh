# AgentMesh — Machine B (Worker) Setup

This guide details how to set up a secondary machine (**Machine B**) to act as a remote worker in an AgentMesh fleet. Machine B runs `agent-agy` (or `agent-mock`) and connects to the coordinator running on Machine A over the network.

---

## Prerequisites

Before starting, ensure Machine B has the following:

- **Rust toolchain**: Required to build the `agent-agy` adapter from source.
- **`agy` CLI**: Installed and authenticated with your chosen AI provider (verify with `agy --version`).
- **Network connectivity**: Machine B must be able to reach Machine A's NATS port (typically TCP 4222) via a LAN IP or VPN (like Tailscale or WireGuard).
- **Git**: Installed and available on the system path.
- **Local clone**: A local clone of the target repository you intend to work on (see Step 3).

---

## Step 1: Install agent-agy

Clone the AgentMesh repository and use the provided installation script to build only the agent binaries. The `--agent-only` flag ensures that the coordinator and PostgreSQL/Docker dependencies are skipped.

```bash
git clone https://github.com/vijaygovindBiju/AgentMesh.git AgentMesh
cd AgentMesh
./scripts/install.sh --agent-only
```

This will compile the `agent-agy` binary and place it in your local binary path (e.g., `~/.local/bin`).

---

## Step 2: Configure environment

Create a `.env.worker` file to store your environment variables. You can copy the template if available, or create one manually:

```bash
touch .env.worker
```

Fill in `.env.worker` with the necessary connection and identity details:

```dotenv
# Point to Machine A's LAN IP or VPN IP (e.g., find it using `ip addr show` on Machine A)
export NATS_URL="nats://<MACHINE_A_IP>:4222"

# Use the exact same token defined in Machine A's .env file
export NATS_AUTH_TOKEN="<shared_nats_token>"

# Agent Identity and Configuration
export AGY_AGENT_OWNER="Machine B Worker"
export AGY_AGENT_API_KEY="am_ak_$(openssl rand -hex 32)"  # Unique API key for this agent
export AGY_EFFORT="medium"
export AGY_TIMEOUT_SECS=900
```

---

## Step 3: Clone the target repository

For the agent to execute tasks on the codebase, Machine B needs a local clone of the exact same target repository that the coordinator on Machine A is managing.

```bash
git clone <target-repo-url> /path/to/project
```

> **Note**: The absolute path to the repository on Machine B can differ from Machine A. If it differs, you must explicitly set `AGY_WORKSPACE_DIR=/path/to/project` in your `.env.worker` file so the agent knows where to create its Git worktrees.

---

## Step 4: Verify NATS connectivity

Before starting the agent, ensure Machine B can successfully reach the NATS server on Machine A.

If you have the `nats` CLI installed, you can run a quick ping test:
```bash
nats pub test.ping 'hello' --server nats://<MACHINE_A_IP>:4222 --creds <token>
```

Alternatively, you can test connectivity simply by starting the agent in the next step and watching the logs for a successful registration.

---

## Step 5: Start the worker agent

Source your environment variables and start the `agent-agy` process:

```bash
source .env.worker && agent-agy
```

**Expected output:**
You should see log messages indicating that the agent connected to NATS, successfully registered with the coordinator, started its heartbeat loop, and is now waiting for task assignments on JetStream. 

You can also switch to the **Fleet Dashboard (Screen 3)** in Machine A's coordinator TUI to verify that Machine B's agent is listed as `idle` and healthy.

---

## Troubleshooting

If things aren't working as expected, check these common issues:

- **Connection refused**: Check if the firewall on Machine A is blocking port 4222, or verify that the NATS container on Machine A is listening on `0.0.0.0` rather than `127.0.0.1`.
- **Auth failed**: The `NATS_AUTH_TOKEN` on Machine B does not match the one configured on Machine A. Remember to restart Machine A's Docker container if you recently changed the token.
- **Registration rejected**: The agent's `AGY_AGENT_ID` might be registered with a revoked status or a mismatched `AGY_AGENT_API_KEY`. Try clearing the ID or generating a fresh API key.
- **Task execution fails**: Ensure the `agy` CLI is correctly installed, authenticated, and available in the system PATH. Check if the specified `AGY_MODEL` is accessible.
- **Repo path issues**: If tasks fail immediately upon assignment, verify that the target repository exists on Machine B and that `AGY_WORKSPACE_DIR` points to the correct local clone directory.

---

## Security Notes

- **Secret Management**: Never commit your `.env.worker` file, NATS tokens, or API keys to version control.
- **Unique Credentials**: Always use unique `AGY_AGENT_API_KEY`s for each agent in your fleet.
- **Production Deployments**: For production, upgrade from simple token authentication to robust NKey credentials.
- **Network Encryption**: If connecting over an untrusted network (e.g., the public internet), strongly consider configuring TLS for NATS connections or tunneling the traffic through an encrypted VPN like Tailscale or WireGuard.
