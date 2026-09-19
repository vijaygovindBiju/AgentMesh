# ADR 010 — Security Architecture, Authentication, and Audit Logging

**Date:** 2026-09-18  
**Status:** Accepted  

---

## Context

When autonomous agents run across separate physical machines, network boundaries, and shared code repositories, several security vectors emerge:
- Unauthorized agents spoofing messages or stealing tasks.
- Agents attempting to modify files outside their designated project scope or repository root.
- Accidental leakage of coordinator secrets or environment credentials in task payloads or logs.
- Lack of non-repudiation and auditability for actions taken by autonomous agents.

---

## Decision

1. **Agent Authentication & Cryptographic Hashing:**
   - Agents authenticate using tokens prefixed with `am_ak_` stored as SHA-256 hashes in PostgreSQL (`api_key_hash`). Plaintext keys are never stored.
   - Verification uses constant-time comparison to prevent timing attacks.
   - Revocation flags (`is_revoked`) and expiry timestamps (`api_key_expires_at`) immediately terminate rogue or outdated agent sessions.

2. **Permission Boundaries & Path Enforcement:**
   - Every agent is bounded by a `PermissionBoundary` defining `allowed_directories`, `denied_paths`, and `allow_network`.
   - Any task specifying resources matching `denied_paths` (e.g. `.env`, `.git/`, private keys) is rejected prior to assignment.

3. **Secret Redaction & Immutable Audit Trail:**
   - Sensitive environment variables and secrets are systematically redacted from task specs and logs.
   - An append-only `audit_logs` table records every critical action (`agent_register`, `task_approve`, `task_assign`, `key_revocation`, `auth_failure`) with actor, timestamp, outcome, and metadata.

---

## Consequences

- Full defense-in-depth across the control plane.
- Remote agents cannot exceed their sandboxed authority.
- Complete regulatory audit trail for all AI and human actions.
