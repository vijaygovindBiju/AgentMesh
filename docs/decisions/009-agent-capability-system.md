# ADR 009 — Agent Capability Matching and Health-Gated Assignment

**Date:** 2026-09-18  
**Status:** Accepted  

---

## Context

In a heterogeneous multi-agent fleet, agents differ in runtime environments (Rust, Python, Go, Docker), available CLI tools, and machine specifications. Assigning tasks randomly or purely round-robin leads to immediate failures if an agent lacks the necessary language toolchain or permissions. Furthermore, assigning work to unhealthy or degraded agents leads to cascading delivery timeouts.

---

## Decision

1. **Structured Capability Profiles:**
   - Agents register capability manifests containing supported languages, frameworks, toolchains, and platform metadata.
   - Task requirements are extracted during AI planning (`affected_resources`, file extensions, project tags).
   - The coordinator evaluates matching scores and candidate compatibility via `AgentCapabilityMatcher`.

2. **Health-Gated Assignment:**
   - Agents maintain an operational `HealthStatus` (`Healthy`, `Degraded`, `Unhealthy`) tracked via consecutive execution successes/failures.
   - Tasks are only assigned to agents with `HealthStatus::Healthy` or `Degraded` with adequate score. Unhealthy agents are excluded from assignment until recovery.

---

## Consequences

- Tasks are deterministically routed to workers capable of executing them.
- Degraded agents are protected from receiving critical tasks.
- Unregistered or mismatched capabilities trigger human review warnings rather than silent execution failures.
