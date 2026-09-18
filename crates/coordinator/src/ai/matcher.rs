use uuid::Uuid;
use crate::ai::schema::AvailableAgentContext;

/// Matches tasks to candidate agents based on capabilities, task description, and affected files.
pub struct AgentCapabilityMatcher;

impl AgentCapabilityMatcher {
    /// Evaluates available agents against a task's title, description, and affected resources.
    /// Returns the ID of the agent with the highest capability match score, if any match exists.
    pub fn suggest_agent(
        available_agents: &[AvailableAgentContext],
        title: &str,
        description: &str,
        affected_resources: &[String],
    ) -> Option<Uuid> {
        if available_agents.is_empty() {
            return None;
        }

        let inferred_tags = Self::infer_required_capabilities(title, description, affected_resources);

        let mut best_agent = None;
        let mut highest_score = 0;

        for agent in available_agents {
            let mut score = 0;
            for cap in &agent.capabilities {
                let cap_norm = cap.to_lowercase();
                for tag in &inferred_tags {
                    if cap_norm == *tag || cap_norm.contains(tag) || tag.contains(&cap_norm) {
                        score += 10;
                    }
                }
            }

            if score > highest_score {
                highest_score = score;
                best_agent = Some(agent.agent_id);
            }
        }

        best_agent
    }

    /// Infers required skill and language tags from task context.
    pub fn infer_required_capabilities(
        title: &str,
        description: &str,
        affected_resources: &[String],
    ) -> Vec<String> {
        let text = format!("{} {}", title, description).to_lowercase();
        let mut tags = Vec::new();

        // 1. File extension heuristics
        for path in affected_resources {
            let path_lower = path.to_lowercase();
            if path_lower.ends_with(".rs") {
                tags.push("rust".to_string());
                tags.push("backend".to_string());
            } else if path_lower.ends_with(".ts") || path_lower.ends_with(".tsx") {
                tags.push("typescript".to_string());
                tags.push("frontend".to_string());
            } else if path_lower.ends_with(".js") || path_lower.ends_with(".jsx") {
                tags.push("javascript".to_string());
                tags.push("frontend".to_string());
            } else if path_lower.ends_with(".py") {
                tags.push("python".to_string());
            } else if path_lower.ends_with(".go") {
                tags.push("go".to_string());
            } else if path_lower.ends_with(".dart") {
                tags.push("dart".to_string());
                tags.push("flutter".to_string());
            } else if path_lower.ends_with(".sql") {
                tags.push("sql".to_string());
                tags.push("db".to_string());
                tags.push("database".to_string());
            }
        }

        // 2. Keyword heuristics
        if text.contains("rust") || text.contains("cargo") {
            tags.push("rust".to_string());
        }
        if text.contains("frontend") || text.contains("ui") || text.contains("web") || text.contains("react") || text.contains("html") {
            tags.push("frontend".to_string());
        }
        if text.contains("backend") || text.contains("server") || text.contains("api") || text.contains("endpoint") {
            tags.push("backend".to_string());
        }
        if text.contains("database") || text.contains("postgres") || text.contains("migration") || text.contains("sql") {
            tags.push("database".to_string());
            tags.push("db".to_string());
        }
        if text.contains("python") || text.contains("django") || text.contains("fastapi") {
            tags.push("python".to_string());
        }
        if text.contains("flutter") || text.contains("dart") {
            tags.push("flutter".to_string());
            tags.push("dart".to_string());
        }

        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_matching_by_file_and_keywords() {
        let rust_agent_id = Uuid::new_v4();
        let frontend_agent_id = Uuid::new_v4();

        let agents = vec![
            AvailableAgentContext {
                agent_id: rust_agent_id,
                human_owner: "Alice".to_string(),
                adapter_type: "Agy".to_string(),
                capabilities: vec!["rust".to_string(), "backend".to_string()],
            },
            AvailableAgentContext {
                agent_id: frontend_agent_id,
                human_owner: "Bob".to_string(),
                adapter_type: "Agy".to_string(),
                capabilities: vec!["frontend".to_string(), "typescript".to_string()],
            },
        ];

        // 1. Rust backend task
        let suggested_rust = AgentCapabilityMatcher::suggest_agent(
            &agents,
            "Build REST API endpoint",
            "Implement user login endpoint in Rust",
            &["src/api/auth.rs".to_string()],
        );
        assert_eq!(suggested_rust, Some(rust_agent_id));

        // 2. Frontend React task
        let suggested_frontend = AgentCapabilityMatcher::suggest_agent(
            &agents,
            "Build React Dashboard",
            "Render user profile card",
            &["frontend/src/components/Card.tsx".to_string()],
        );
        assert_eq!(suggested_frontend, Some(frontend_agent_id));
    }
}
