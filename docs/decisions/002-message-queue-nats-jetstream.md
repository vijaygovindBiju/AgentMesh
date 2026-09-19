# ADR 002 — Message Queue: NATS with JetStream

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

The coordinator must deliver task assignments to agents reliably and receive lifecycle events back. The delivery system must handle:

- Agent disconnection and reconnection (durable delivery)
- Exactly-once processing per task assignment (idempotency)
- Multiple simultaneous agents
- Internet-accessible deployment (agents may be remote)

---

## Options

| Technology | Pros | Cons |
|---|---|---|
| Redis Streams | Familiar, battle-tested, durable | Requires Redis server; heavier dependency |
| RabbitMQ | Feature-rich, reliable | Heavier setup; Rust client less mature |
| Apache Kafka | Very durable, high-throughput | Massive overkill for this scale; requires ZooKeeper or KRaft |
| NATS core only | Extremely lightweight, fast | No durability — messages lost if consumer offline |
| NATS + JetStream | Durable, fast, Rust-native, no ZooKeeper | JetStream adds complexity vs. core NATS |

---

## Decision

**NATS 2.x with JetStream** for streams requiring durable delivery.  
**NATS core** (non-JetStream) for fire-and-forget messages like heartbeats.

---

## Reasons

1. **Rust-native client.** `async-nats` is the official Rust client, actively maintained by Synadia. It supports both core NATS and JetStream with full async/await.

2. **Lightweight.** NATS server is a single binary (~20MB). No ZooKeeper, no Kafka ecosystem.

3. **JetStream provides required durability.** WorkQueue streams give exactly-once delivery per consumer group; agents can disconnect and reconnect without losing their assigned task.

4. **Auth model fits.** NATS supports token auth (dev), NKey credentials (production), and TLS. Per-agent security can be layered at the application level using agent IDs in subjects.

5. **Monitoring.** NATS exposes an HTTP monitoring endpoint (`/healthz`, `/jsz`, `/connz`) at no extra cost.

---

## JetStream vs. Core NATS — Usage Split

| Message type | Transport |
|---|---|
| Task assignment (coordinator → agent) | JetStream `TASK_ASSIGNMENTS` (WorkQueue) |
| Agent lifecycle events (agent → coordinator) | JetStream `AGENT_EVENTS` (Limits) |
| Agent registration | Core NATS request-reply |
| Heartbeat | Core NATS publish (fire-and-forget) |

---

## Consequences

- JetStream requires a `store_dir` — persisted in a Docker volume.
- Agents must implement JetStream consumer API (not just Subscribe).
- Production deployments must replace the dev token with NKey credentials and add TLS.
- See `docs/decisions/007-nats-delivery-semantics.md` for the explicit delivery model.
