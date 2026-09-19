# ADR 011 — Observability, Task Timelines, and Failure Diagnostics

**Date:** 2026-09-19  
**Status:** Accepted  

---

## Context

In complex distributed workflows involving multiple agents, asynchronous messaging, and long-running builds, operator visibility is critical. Operators need to know:
- Exactly where a task spent time (proposal, review, delivery, execution, completion).
- What went wrong when a task failed (transport timeout, agent crash, Git merge conflict, compile error).
- High-level throughput and fleet health metrics across projects.

---

## Decision

1. **Chronological Timelines:**
   - `TimelineService` generates aggregated lifecycle timelines for tasks and agents by merging PostgreSQL records from `tasks`, `task_approvals`, `task_deliveries`, `agent_events`, and `coordinator_events`.

2. **Automated Failure Diagnostics:**
   - When a task fails, `FailureDiagnostics` runs root-cause classification (identifying Git conflicts, agent timeouts, or delivery expirations) and produces structured remediation advice for human operators.

3. **System Metrics Aggregation:**
   - `MetricsCollector` computes real-time fleet metrics (active agents, task throughput, delivery success rate percentages) displayed in the interactive TUI Diagnostics screen (`Tab 3`).

---

## Consequences

- Operators gain immediate clarity into task progression and fleet performance without manually querying the database.
- Failure diagnosis is automated, drastically reducing MTTR (Mean Time to Resolution).
