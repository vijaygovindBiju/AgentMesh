use crate::ai::schema::{PlanningRequest, ReplanRequest};

pub struct PlanningPrompt;

impl PlanningPrompt {
    /// Returns the system prompt describing the coordinator's planning rules.
    pub fn system_prompt() -> &'static str {
        r#"You are an AI Project Coordinator planning a software project for multiple autonomous coding agents.
Your job is to decompose the project into discrete, non-overlapping tasks with explicit dependencies.

RULES:
1. Each task must have a unique `short_id` (e.g. "TASK-1", "TASK-2").
2. Explicitly specify `affected_resources` (file paths or modules) for each task. Minimize file overlaps. Ground paths in the real repository layout.
3. If two tasks must modify the same resource, establish a sequential dependency between them.
4. Dependencies MUST be a Directed Acyclic Graph (DAG). NEVER create cyclic dependencies.
5. Suggest an agent for each task based on agent capabilities, or leave suggested_agent_id null.
6. Provide an estimated_size for each task ("XS", "S", "M", "L", "XL").
7. Return ONLY valid JSON adhering strictly to the required schema. Do not include Markdown fences or extraneous commentary outside the JSON."#
    }

    /// Builds the user prompt containing the project context, repository architecture, available agents, and existing tasks.
    pub fn user_prompt(request: &PlanningRequest) -> String {
        let mut prompt = format!(
            "PROJECT NAME: {}\nPROJECT DESCRIPTION:\n{}\n\n",
            request.project_name, request.project_description
        );

        // Append real repository architecture if available
        if let Some(ref repo) = request.repo_context {
            prompt.push_str("REPOSITORY ARCHITECTURE & CODEBASE CONTEXT:\n");
            prompt.push_str(&format!("- Root Path: {}\n", repo.repo_root.display()));
            if !repo.detected_ecosystems.is_empty() {
                prompt.push_str(&format!("- Detected Ecosystems: {}\n", repo.detected_ecosystems.join(", ")));
            }
            if !repo.primary_languages.is_empty() {
                prompt.push_str(&format!("- Primary Languages: {}\n", repo.primary_languages.join(", ")));
            }
            if !repo.key_modules.is_empty() {
                prompt.push_str(&format!("- Key Modules & Directories: {}\n", repo.key_modules.join(", ")));
            }
            if let Some(ref readme) = repo.readme_summary {
                prompt.push_str(&format!("- README Excerpt:\n{}\n", readme));
            }
            if !repo.file_tree_sample.is_empty() {
                prompt.push_str("- Existing Repository File Structure (Sample):\n");
                for file in repo.file_tree_sample.iter().take(40) {
                    prompt.push_str(&format!("  * {}\n", file));
                }
            }
            prompt.push('\n');
        }

        prompt.push_str("AVAILABLE AGENTS:\n");
        if request.available_agents.is_empty() {
            prompt.push_str("None registered yet.\n");
        } else {
            for a in &request.available_agents {
                prompt.push_str(&format!(
                    "- ID: {} | Owner: {} | Capabilities: {:?}\n",
                    a.agent_id, a.human_owner, a.capabilities
                ));
            }
        }

        prompt.push_str("\nEXISTING TASKS ALREADY IN PROJECT:\n");
        if request.existing_tasks.is_empty() {
            prompt.push_str("None (greenfield project).\n");
        } else {
            for t in &request.existing_tasks {
                prompt.push_str(&format!(
                    "- Short ID: {} | Title: {} | Status: {} | Resources: {:?}\n",
                    t.short_id, t.title, t.status, t.affected_resources
                ));
            }
        }

        prompt.push_str(
            r#"
OUTPUT JSON SCHEMA:
{
  "reasoning": "string explaining the architectural strategy and decomposition rationale",
  "proposed_tasks": [
    {
      "short_id": "string",
      "title": "string",
      "description": "string",
      "suggested_agent_id": "uuid or null",
      "affected_resources": ["string"],
      "estimated_size": "XS | S | M | L | XL"
    }
  ],
  "proposed_dependencies": [
    {
      "dependent_short_id": "string",
      "depends_on_short_id": "string",
      "kind": "blocks | relates_to",
      "reason": "string"
    }
  ]
}
"#,
        );

        prompt
    }
}

pub struct ReplanPrompt;

impl ReplanPrompt {
    pub fn system_prompt() -> &'static str {
        r#"You are an AI Project Coordinator responsible for dynamic re-planning of a software project.
Tasks may have completed, failed, encountered blockers, or introduced unexpected changes.
Your job is to produce a corrective plan:
1. Preserve completed work; DO NOT re-execute already completed tasks.
2. Address failed tasks by proposing replacement fix tasks or adjusting scope.
3. Integrate unexpected file changes into future task scopes.
4. Establish valid DAG dependencies and assign appropriate agents.
5. Return ONLY valid JSON matching the PlanningResponse schema."#
    }

    pub fn user_prompt(request: &ReplanRequest) -> String {
        let mut prompt = format!(
            "RE-PLANNING REQUEST FOR PROJECT: {}\nDESCRIPTION: {}\n\n",
            request.project_name, request.project_description
        );

        prompt.push_str("COMPLETED TASKS (Preserve & build upon):\n");
        if request.completed_tasks.is_empty() {
            prompt.push_str("None.\n");
        } else {
            for t in &request.completed_tasks {
                prompt.push_str(&format!(
                    "- [{}] {} (Modified: {:?})\n",
                    t.short_id, t.title, t.actual_modified_resources
                ));
            }
        }

        prompt.push_str("\nFAILED / BLOCKED TASKS (Need corrective action):\n");
        if request.failed_tasks.is_empty() {
            prompt.push_str("None.\n");
        } else {
            for t in &request.failed_tasks {
                prompt.push_str(&format!(
                    "- [{}] {} | Error: {} | Blocker: {:?}\n",
                    t.short_id, t.title, t.error, t.blocker_reason
                ));
            }
        }

        if !request.unexpected_changes.is_empty() {
            prompt.push_str("\nUNEXPECTED RESOURCE MODIFICATIONS DETECTED:\n");
            for change in &request.unexpected_changes {
                prompt.push_str(&format!("- {}\n", change));
            }
        }

        if !request.cross_agent_conflicts.is_empty() {
            prompt.push_str("\nCROSS-AGENT CONFLICTS DETECTED:\n");
            for conflict in &request.cross_agent_conflicts {
                prompt.push_str(&format!("- {}\n", conflict));
            }
        }

        prompt.push_str("\nACTIVE OR PENDING TASKS:\n");
        if request.active_or_pending_tasks.is_empty() {
            prompt.push_str("None.\n");
        } else {
            for t in &request.active_or_pending_tasks {
                prompt.push_str(&format!("- [{}] {} ({})\n", t.short_id, t.title, t.status));
            }
        }

        prompt.push_str(
            r#"
OUTPUT JSON SCHEMA:
{
  "reasoning": "string explaining corrective strategy",
  "proposed_tasks": [
    {
      "short_id": "string",
      "title": "string",
      "description": "string",
      "suggested_agent_id": "uuid or null",
      "affected_resources": ["string"],
      "estimated_size": "XS | S | M | L | XL"
    }
  ],
  "proposed_dependencies": [
    {
      "dependent_short_id": "string",
      "depends_on_short_id": "string",
      "kind": "blocks | relates_to",
      "reason": "string"
    }
  ]
}
"#,
        );

        prompt
    }
}
