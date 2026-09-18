-- AgentMesh Git Coordination Schema Migration
-- Migration: 002_git_coordination.sql

-- 1. Extend projects with Git repository identity
ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS repo_path TEXT,
    ADD COLUMN IF NOT EXISTS base_branch VARCHAR(255) NOT NULL DEFAULT 'main',
    ADD COLUMN IF NOT EXISTS current_commit_sha VARCHAR(64);

-- 2. Extend tasks with Git branch and commit tracking
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS task_branch VARCHAR(255),
    ADD COLUMN IF NOT EXISTS base_commit_sha VARCHAR(64),
    ADD COLUMN IF NOT EXISTS completion_commit_sha VARCHAR(64),
    ADD COLUMN IF NOT EXISTS actual_modified_resources JSONB NOT NULL DEFAULT '[]'::jsonb;

-- 3. Cross-agent Git conflicts table
CREATE TABLE IF NOT EXISTS git_conflicts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id_a UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    task_id_b UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    conflicting_path TEXT NOT NULL,
    description TEXT NOT NULL,
    resolved BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_git_conflicts_project_id ON git_conflicts(project_id);
CREATE INDEX IF NOT EXISTS idx_git_conflicts_unresolved ON git_conflicts(resolved) WHERE resolved = FALSE;

-- 4. Unexpected resource modifications table
CREATE TABLE IF NOT EXISTS unexpected_resource_changes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    resource_path TEXT NOT NULL,
    reason TEXT NOT NULL,
    acknowledged BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_unexpected_resources_task_id ON unexpected_resource_changes(task_id);
