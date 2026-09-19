-- AgentMesh Agent Capabilities and Health Schema Migration
-- Migration: 003_agent_capabilities_and_health.sql

-- 1. Create agent health status enum
DO $$ BEGIN
    CREATE TYPE agent_health_status AS ENUM (
        'healthy',
        'degraded',
        'unhealthy',
        'offline'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- 2. Extend agents table with capability profile, availability, and health metrics
ALTER TABLE agents
    ADD COLUMN IF NOT EXISTS capability_profile JSONB,
    ADD COLUMN IF NOT EXISTS health_status agent_health_status NOT NULL DEFAULT 'healthy',
    ADD COLUMN IF NOT EXISTS consecutive_failures INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS tasks_completed_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS tasks_failed_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_error TEXT,
    ADD COLUMN IF NOT EXISTS heartbeat_latency_ms BIGINT,
    ADD COLUMN IF NOT EXISTS max_concurrency INT NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS active_tasks_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS is_draining BOOLEAN NOT NULL DEFAULT FALSE;

-- 3. Indexes for fast availability and health lookups
CREATE INDEX IF NOT EXISTS idx_agents_health_status ON agents(health_status);
CREATE INDEX IF NOT EXISTS idx_agents_is_draining ON agents(is_draining);
