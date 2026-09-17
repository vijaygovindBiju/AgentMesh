use crate::ai::schema::PlanningRequest;

pub struct PlanningPrompt;

impl PlanningPrompt {
    /// Returns the system prompt describing the coordinator's planning rules.
    pub fn system_prompt() -> &'static str {
        r#"You are an AI Project Coordinator planning a software project for multiple autonomous coding agents.
Your job is to decompose the project into discrete, non-overlapping tasks with explicit dependencies.

RULES:
1. Each task must have a unique `short_id` (e.g. "TASK-1", "TASK-2").
2. Explicitly specify `affected_resources` (file paths or modules) for each task. Minimize file overlaps.
3. If two tasks must modify the same resource, establish a sequential dependency between them.
4. Dependencies MUST be a Directed Acyclic Graph (DAG). NEVER create cyclic dependencies (e.g. A depends on B and B depends on A).
5. Suggest an agent for each task based on agent capabilities, or leave suggested_agent_id null.
6. Return ONLY valid JSON adhering strictly to the required schema. Do not include Markdown fences or extraneous commentary outside the JSON."#
    }

    /// Builds the user prompt containing the project context, available agents, and existing tasks.
    pub fn user_prompt(request: &PlanningRequest) -> String {
        let mut prompt = format!(
            "PROJECT NAME: {}\nPROJECT DESCRIPTION:\n{}\n\nAVAILABLE AGENTS:\n",
            request.project_name, request.project_description
        );

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
  "reasoning": "string explaining the architectural strategy",
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
