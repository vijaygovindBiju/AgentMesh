# ADR 003 — Database: PostgreSQL

**Date:** 2026-09-16  
**Status:** Accepted

---

## Context

The coordinator needs persistent storage for:
- Project definitions and metadata
- Task state and history
- Agent registrations and credentials (hashed)
- Proposal and approval records
- Task delivery tracking (attempt, ack status, idempotency)
- Agent event log
- Overlap warnings

The database is the **authoritative source of truth**. NATS is the transport layer only.

---

## Options

| Technology | Pros | Cons |
|---|---|---|
| SQLite | Zero-config, embedded, simple | Concurrent writes limited; not suitable for remote agents writing simultaneously |
| PostgreSQL | ACID, JSONB, UUID, concurrent writes, strong Rust support | Requires a running server |
| MongoDB | Flexible schema, easy JSONB-like storage | Less ACID guarantees; weaker Rust ecosystem |
| In-memory only | Zero setup | No persistence; not suitable for a coordination system |

---

## Decision

**PostgreSQL 16.**

---

## Reasons

1. **ACID guarantees.** Task state transitions must be atomic. A coordinator crash between "mark task Assigned" and "publish to NATS" must leave the database in a consistent, recoverable state.

2. **JSONB support.** Flexible fields (`affected_resources`, `capabilities`, `payload`) benefit from JSONB — queryable JSON without a separate document store.

3. **UUID support.** First-class UUID type and indexing.

4. **sqlx compile-time query checking.** `sqlx` in Rust verifies SQL queries against the actual schema at compile time (using the `DATABASE_URL` or offline `.sqlx/` cache). This eliminates an entire class of runtime SQL errors.

5. **Concurrent agent writes.** Multiple agents reporting events simultaneously requires proper row-level locking, which PostgreSQL handles correctly.

6. **Operational maturity.** PostgreSQL is the most widely-deployed open-source relational database. The Docker image is stable and well-understood.

---

## Consequences

- Requires a running PostgreSQL server. Handled by `docker-compose.yml`.
- Developers need `sqlx-cli` for running migrations locally.
- `sqlx` compile-time checking requires a `DATABASE_URL` set in the environment during builds (or an offline `.sqlx/` cache committed to the repository).
- Schema changes require migration files in `migrations/`.
