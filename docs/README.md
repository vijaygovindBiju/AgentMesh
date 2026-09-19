# AgentMesh Documentation

Start with the top-level [README](../README.md) — it covers installation, quick start, mock and `agy` agents, multi-machine setup, testing, troubleshooting and the honest v1.0 status. The documents below go deeper on specific areas.

| Directory | Contents |
| :--- | :--- |
| [`architecture/`](architecture/) | [`architecture.md`](architecture/architecture.md) — system overview, design principles, v1.0 module map and what is wired into the binary. [`domain-model.md`](architecture/domain-model.md) — entities, task/agent/delivery state machines, invariants. |
| [`protocols/`](protocols/) | [`agent-protocol.md`](protocols/agent-protocol.md) — NATS subjects, JetStream streams, `AgentMessage` / `CoordinatorMessage` schemas, registration flow, idempotency contract. |
| [`deployment/`](deployment/) | [`multi-machine.md`](deployment/multi-machine.md) — coordinator on one host, agents on others: firewall, addresses, tokens, verification, network options, troubleshooting. |
| [`development/`](development/) | [`v1-validation.md`](development/v1-validation.md) — Phase 15 end-to-end validation report and test matrix. |
| [`decisions/`](decisions/) | Architecture Decision Records 001–012 (language, NATS, PostgreSQL, TUI, protocol, LLM abstraction, delivery semantics, human approval boundary, capabilities, security, observability, resilience). |

Conventions:

- The source code is the source of truth. When a document and the code disagree, the code wins and the document should be fixed.
- ADRs are append-only history; supersede rather than rewrite them.
- SQL schema lives in [`../migrations/`](../migrations/) and is applied automatically by the coordinator at startup.
