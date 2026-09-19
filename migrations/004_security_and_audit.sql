-- AgentMesh Security and Audit Logging Schema Migration
-- Migration: 004_security_and_audit.sql

-- 1. Extend agents table with security, role, and permission boundary columns
ALTER TABLE agents
    ADD COLUMN IF NOT EXISTS role VARCHAR(50) NOT NULL DEFAULT 'worker',
    ADD COLUMN IF NOT EXISTS permissions JSONB NOT NULL DEFAULT '{"allowed_paths":[],"denied_paths":[".env*","*id_rsa*","*credentials*","*secrets*"],"can_modify_code":true,"can_run_commands":true}'::jsonb,
    ADD COLUMN IF NOT EXISTS is_revoked BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS api_key_created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN IF NOT EXISTS api_key_expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_agents_is_revoked ON agents(is_revoked);
CREATE INDEX IF NOT EXISTS idx_agents_role ON agents(role);

-- 2. Audit logging table for security-sensitive actions
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    actor_type VARCHAR(50) NOT NULL, -- 'agent', 'human', 'system'
    actor_id VARCHAR(100),            -- agent UUID, human user name, or system component
    action VARCHAR(100) NOT NULL,    -- 'agent_register', 'auth_success', 'auth_failure', 'task_authorize', 'permission_denied', 'key_rotate', 'key_revoke', 'secret_redacted'
    resource_type VARCHAR(50) NOT NULL, -- 'agent', 'task', 'project', 'nats', 'system'
    resource_id VARCHAR(100),        -- target resource UUID or subject
    status VARCHAR(50) NOT NULL,     -- 'success', 'denied', 'failure'
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip_address VARCHAR(45)
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_timestamp ON audit_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor ON audit_logs(actor_type, actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs(action);
CREATE INDEX IF NOT EXISTS idx_audit_logs_status ON audit_logs(status);
