# ADR 005 — Agent Protocol Design

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

AgentMesh must communicate with multiple agent runtimes: currently `agy` (Antigravity), with others possible in the future. The protocol must:

- Work regardless of agent runtime
- Support different transports (NATS today; HTTP or IPC later if needed)
- Define a stable message vocabulary that agents implement
- Not require the coordinator to know how an agent internally works

The first concrete integration is `agy`, which is driven via CLI subprocess and stdout parsing. A mock agent is also required for automated testing.

---

## Decision

**Transport-independent message types in a shared `agent-protocol` crate. NATS as the initial transport.**

---

## Design

### `agent-protocol` crate (shared)
- Contains only: `AgentMessage` enum, `CoordinatorMessage` enum, `TaskSpec` struct, `AgentStatus` enum, `DeliveryAck` type.
- Has **no dependency** on async-nats, tokio, HTTP, or any I/O library.
- Is imported by: coordinator, agent-mock, agent-agy (and all future adapters).

### Transport boundary
Each agent binary is responsible for connecting to NATS and serializing/deserializing `agent-protocol` types. The protocol is JSON-encoded for human readability and debuggability.

### AgentAdapter trait (in coordinator)
The coordinator interacts with agents through a trait:

```rust
// coordinator/src/coordinator/agent_adapter.rs
pub trait AgentAdapter: Send + Sync {
    fn agent_id(&self) -> Uuid;
    async fn send_task(&self, spec: TaskSpec) -> Result<()>;
    async fn cancel_task(&self, task_id: Uuid, reason: &str) -> Result<()>;
}
```

This trait is implemented per adapter type. The coordinator does not call NATS directly when communicating with an agent — it calls the adapter. This allows future adapters to use a different transport.

---

## `agy` Integration Mechanism

`agy` does not expose a REST or WebSocket API today. The integration is:

1. `agent-agy` process connects to NATS as a registered agent.
2. It receives `TaskAssignment` from the coordinator via NATS.
3. It spawns `agy` as a subprocess with the task description as a prompt or instruction file.
4. It monitors `agy`'s stdout for progress indicators and completion.
5. It translates stdout output to `AgentMessage` events and publishes them back to the coordinator.

The exact stdout parsing rules will be defined in a separate ADR once the agy integration format is confirmed. The `agent-agy` crate is stubbed in v0.1.

---

## Mock Agent Requirements

The mock agent (`agent-mock`) is **not** a coordinator-internal fake. It is:
- A separate binary that connects to NATS the same way `agent-agy` would
- Implements the same message contract (same `agent-protocol` types)
- Simulates work with configurable delays (`MOCK_TASK_DELAY_MS`)
- Reports a complete lifecycle: `TaskStarted` → `ProgressUpdate` × N → `Blocked` (optional) → `Completed` or `Failed`
- Can be used in integration tests and live demos without real LLM or agy

---

## Consequences

- `agent-protocol` must be versioned carefully — breaking changes require all adapters to update.
- The adapter boundary means the coordinator can be tested without real agents by using `MockAgentAdapter` (an in-process test double distinct from the `agent-mock` binary).
- Protocol stability is a constraint: once agents are in production, the message format must evolve backwards-compatibly.
