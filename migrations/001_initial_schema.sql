-- AgentMesh Initial PostgreSQL Schema Migration
-- Migration: 001_initial_schema.sql

-- Enable pgcrypto for UUID generation if needed
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ============================================================================
-- Custom Enum Types
-- ============================================================================

CREATE TYPE project_status AS ENUM (
    'draft',
    'planning',
    'active',
    'paused',
    'completed'
);

CREATE TYPE proposal_status AS ENUM (
    'pending',
    'partially_approved',
    'fully_approved',
    'rejected'
);

CREATE TYPE agent_status AS ENUM (
    'offline',
    'idle',
    'busy',
    'blocked',
    'error'
);

CREATE TYPE adapter_type AS ENUM (
    'mock',
    'agy'
);

CREATE TYPE task_status AS ENUM (
    'proposed',
    'human_review',
    'approved',
    'rejected',
    'assigned',
    'executing',
    'blocked',
    'completed',
    'failed',
    'cancelled'
);

CREATE TYPE dependency_kind AS ENUM (
    'blocks',
    'relates_to'
);

CREATE TYPE approval_status AS ENUM (
    'approved',
    'rejected',
    'edited_and_approved'
);

CREATE TYPE delivery_status AS ENUM (
    'pending',
    'delivered',
    'acknowledged',
    'nak_requeued',
    'terminal',
    'reassigned'
);

CREATE TYPE ack_kind AS ENUM (
    'ack',
    'nak',
    'term'
);

CREATE TYPE event_type AS ENUM (
    'task_started',
    'progress_update',
    'blocked',
    'completed',
    'failed'
);

CREATE TYPE overlap_severity AS ENUM (
    'info',
    'warning',
    'critical'
);

-- ============================================================================
-- Tables
-- ============================================================================

-- Projects Table
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    description TEXT NOT NULL,
    status project_status NOT NULL DEFAULT 'draft',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Proposals Table
CREATE TABLE proposals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ai_provider VARCHAR(64) NOT NULL,
    ai_model VARCHAR(128) NOT NULL,
    raw_prompt TEXT NOT NULL,
    raw_response TEXT NOT NULL,
    status proposal_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Agents Table
CREATE TABLE agents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    human_owner VARCHAR(255) NOT NULL,
    api_key_hash VARCHAR(255) NOT NULL,
    adapter_type adapter_type NOT NULL,
    capabilities JSONB NOT NULL DEFAULT '[]'::jsonb,
    nats_subject VARCHAR(255) NOT NULL,
    status agent_status NOT NULL DEFAULT 'offline',
    current_task_id UUID, -- Foreign key to tasks(id) added after tasks table creation
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Tasks Table
CREATE TABLE tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    short_id VARCHAR(64) NOT NULL,
    title VARCHAR(255) NOT NULL,
    description TEXT NOT NULL,
    status task_status NOT NULL DEFAULT 'proposed',
    assigned_agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
    affected_resources JSONB NOT NULL DEFAULT '[]'::jsonb,
    estimated_size VARCHAR(16),
    proposal_id UUID NOT NULL REFERENCES proposals(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_tasks_project_short_id UNIQUE (project_id, short_id)
);

-- Add circular foreign key from agents to tasks
ALTER TABLE agents
    ADD CONSTRAINT fk_agents_current_task
    FOREIGN KEY (current_task_id) REFERENCES tasks(id) ON DELETE SET NULL;

-- Task Dependencies Table
CREATE TABLE task_dependencies (
    dependent_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind dependency_kind NOT NULL DEFAULT 'blocks',
    PRIMARY KEY (dependent_id, depends_on_id),
    CONSTRAINT chk_no_self_dependency CHECK (dependent_id <> depends_on_id)
);

-- Task Approvals Table (Human Approval Boundary)
CREATE TABLE task_approvals (
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    proposal_id UUID NOT NULL REFERENCES proposals(id) ON DELETE CASCADE,
    status approval_status NOT NULL,
    edited_desc TEXT,
    approved_by VARCHAR(255) NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id, proposal_id)
);

-- Task Deliveries Table (Explicit Delivery Semantics)
CREATE TABLE task_deliveries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    attempt INT NOT NULL DEFAULT 1,
    nats_stream VARCHAR(128) NOT NULL,
    nats_subject VARCHAR(255) NOT NULL,
    nats_sequence BIGINT,
    idempotency_key VARCHAR(255) NOT NULL UNIQUE,
    delivered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    acknowledged_at TIMESTAMPTZ,
    ack_kind ack_kind,
    expires_at TIMESTAMPTZ NOT NULL,
    status delivery_status NOT NULL DEFAULT 'pending',
    failure_reason TEXT,
    reassigned_to UUID REFERENCES agents(id) ON DELETE SET NULL
);

-- Agent Events Table (Append-only audit trail)
CREATE TABLE agent_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type event_type NOT NULL,
    message TEXT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Overlap Warnings Table
CREATE TABLE overlap_warnings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_ids JSONB NOT NULL,
    resource VARCHAR(512) NOT NULL,
    severity overlap_severity NOT NULL DEFAULT 'warning',
    acknowledged BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- Indexes
-- ============================================================================

CREATE INDEX idx_proposals_project_id ON proposals(project_id);
CREATE INDEX idx_tasks_project_id ON tasks(project_id);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_assigned_agent ON tasks(assigned_agent_id);
CREATE INDEX idx_tasks_proposal_id ON tasks(proposal_id);
CREATE INDEX idx_task_dependencies_dependent ON task_dependencies(dependent_id);
CREATE INDEX idx_task_dependencies_depends_on ON task_dependencies(depends_on_id);
CREATE INDEX idx_task_deliveries_task ON task_deliveries(task_id);
CREATE INDEX idx_task_deliveries_agent ON task_deliveries(agent_id);
CREATE INDEX idx_task_deliveries_status ON task_deliveries(status);
CREATE INDEX idx_task_deliveries_idempotency ON task_deliveries(idempotency_key);
CREATE INDEX idx_agent_events_agent ON agent_events(agent_id);
CREATE INDEX idx_agent_events_task ON agent_events(task_id);
CREATE INDEX idx_agent_events_type ON agent_events(event_type);
CREATE INDEX idx_overlap_warnings_project ON overlap_warnings(project_id);
CREATE INDEX idx_overlap_warnings_ack ON overlap_warnings(acknowledged);
