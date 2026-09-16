# ADR 001 — Language: Rust

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

AgentMesh is a coordination daemon that needs to:
- Run continuously as a long-lived process
- Handle concurrent async events (NATS messages, TUI, DB queries)
- Be deployable as a small, self-contained binary
- Maintain correct state under concurrent access
- Be reliable — coordinator bugs would affect all agents

Language choices considered: Python, TypeScript/Node.js, Go, Rust.

---

## Options

| Language | Pros | Cons |
|---|---|---|
| Python | Fast iteration, rich AI/async libs | GIL limitations, runtime required, packaging complexity |
| TypeScript | Fast iteration, async ecosystem | Runtime (Node.js) required, weaker type guarantees |
| Go | Simple concurrency, single binary, fast compile | Less expressive type system, no trait-based abstraction |
| Rust | Strong type safety, fearless concurrency, single binary, no GC pauses | Steeper learning curve, longer initial setup |

---

## Decision

**Rust.**

---

## Reasons

1. **Type safety for protocol correctness.** The agent protocol carries state transitions that must be correct. Rust's type system can encode state machine transitions as compile-time guarantees (e.g., `enum TaskStatus` exhaustively matched).

2. **Ownership model for safe concurrency.** The coordinator handles concurrent NATS events, TUI rendering, and database writes. Rust's ownership model prevents data races at compile time.

3. **Single binary deployment.** The coordinator and agent adapters ship as standalone binaries with no runtime dependency. Docker images are small.

4. **Strong async ecosystem.** `tokio`, `async-nats`, `sqlx`, and `ratatui` are all production-quality Rust libraries with active maintenance.

5. **No GC pauses.** A coordination daemon with a live TUI must not stall unpredictably.

---

## Consequences

- Steeper initial setup and learning curve compared to Python/TypeScript.
- Longer compile times compared to Go.
- Some concepts (lifetimes, ownership) require understanding before contributing.
- The above are acceptable tradeoffs for the correctness and deployment benefits.
