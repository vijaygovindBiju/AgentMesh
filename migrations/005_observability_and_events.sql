-- AgentMesh Observability and Coordinator Events Schema Migration
-- Migration: 005_observability_and_events.sql

-- 1. Add task duration tracking fields to tasks table
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS completed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS duration_ms BIGINT;

-- 2. Create coordinator_events table for tracking system-wide lifecycle events
CREATE TABLE IF NOT EXISTS coordinator_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_type VARCHAR(100) NOT NULL, -- 'plan_generated', 'task_approved', 'task_rejected', 'task_assigned', 'task_reassigned', 'overlap_detected', 'git_conflict_detected', 'agent_health_changed', 'agent_drained', 'agent_revoked'
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
    message TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_coord_events_timestamp ON coordinator_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_coord_events_type ON coordinator_events(event_type);
CREATE INDEX IF NOT EXISTS idx_coord_events_project ON coordinator_events(project_id);
CREATE INDEX IF NOT EXISTS idx_coord_events_task ON coordinator_events(task_id);
CREATE INDEX IF NOT EXISTS idx_coord_events_agent ON coordinator_events(agent_id);
