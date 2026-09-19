use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::schema::AvailableAgentContext;
use crate::domain::Agent;
use agent_protocol::{AgentCapabilities, HealthStatus, TaskRequirements};

/// Detailed breakdown of how well an agent fits a task's requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchScore {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub score: u32,
    pub is_eligible: bool,
    pub matched_capabilities: Vec<String>,
    pub missing_requirements: Vec<String>,
    pub match_reasons: Vec<String>,
}

/// Candidate agent evaluated for task assignment.
#[derive(Debug, Clone)]
pub struct CandidateAgent {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub capabilities: Vec<String>,
    pub profile: Option<AgentCapabilities>,
    pub is_available: bool,
    pub health_status: HealthStatus,
}

impl From<&AvailableAgentContext> for CandidateAgent {
    fn from(ctx: &AvailableAgentContext) -> Self {
        let mut caps = ctx.capabilities.clone();
        for c in &ctx.capabilities {
            let low = c.to_lowercase();
            if low == "rust" && !caps.iter().any(|x| x.to_lowercase() == "cargo") {
                caps.push("cargo".to_string());
            }
        }
        Self {
            agent_id: ctx.agent_id,
            human_owner: ctx.human_owner.clone(),
            capabilities: caps,
            profile: None,
            is_available: true,
            health_status: HealthStatus::Healthy,
        }
    }
}

impl From<&Agent> for CandidateAgent {
    fn from(agent: &Agent) -> Self {
        Self {
            agent_id: agent.id,
            human_owner: agent.human_owner.clone(),
            capabilities: agent.capabilities_list(),
            profile: agent.capabilities_profile(),
            is_available: agent.is_available(),
            health_status: agent.health_status.into(),
        }
    }
}

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
        let candidates: Vec<CandidateAgent> =
            available_agents.iter().map(CandidateAgent::from).collect();
        let requirements = Self::infer_task_requirements(title, description, affected_resources);
        let ranked = Self::rank_candidates(&candidates, &requirements);

        ranked
            .into_iter()
            .find(|m| m.is_eligible && m.score > 0)
            .map(|m| m.agent_id)
    }

    /// Evaluates registered coordinator agents against a task and returns the best matching eligible agent.
    pub fn find_best_agent(
        agents: &[Agent],
        title: &str,
        description: &str,
        affected_resources: &[String],
    ) -> Option<MatchScore> {
        let candidates: Vec<CandidateAgent> = agents.iter().map(CandidateAgent::from).collect();
        let requirements = Self::infer_task_requirements(title, description, affected_resources);
        let ranked = Self::rank_candidates(&candidates, &requirements);

        ranked.into_iter().find(|m| m.is_eligible && m.score > 0)
    }

    /// Ranks candidate agents against a set of explicit task requirements.
    pub fn rank_candidates(
        candidates: &[CandidateAgent],
        requirements: &TaskRequirements,
    ) -> Vec<MatchScore> {
        let mut results = Vec::new();

        for candidate in candidates {
            let match_score = Self::evaluate_candidate(candidate, requirements);
            results.push(match_score);
        }

        results.sort_by(|a, b| {
            b.is_eligible
                .cmp(&a.is_eligible)
                .then_with(|| b.score.cmp(&a.score))
        });

        results
    }

    /// Evaluates a single candidate against task requirements.
    pub fn evaluate_candidate(
        candidate: &CandidateAgent,
        requirements: &TaskRequirements,
    ) -> MatchScore {
        let mut score: u32 = 0;
        let mut matched_capabilities = Vec::new();
        let mut missing_requirements = Vec::new();
        let mut match_reasons = Vec::new();
        let mut is_eligible = true;

        // 1. Availability check
        if !candidate.is_available {
            is_eligible = false;
            missing_requirements
                .push("Agent not currently available (busy or draining)".to_string());
        }

        // 2. Health check
        if matches!(
            candidate.health_status,
            HealthStatus::Unhealthy | HealthStatus::Offline
        ) {
            is_eligible = false;
            missing_requirements.push(format!(
                "Unfavorable health state: {:?}",
                candidate.health_status
            ));
        } else if candidate.health_status == HealthStatus::Healthy {
            score += 10;
            match_reasons.push("Agent health is optimal".to_string());
        }

        // 3. Operating system constraint
        if let Some(ref req_os) = requirements.required_os {
            let req_os_lower = req_os.to_lowercase();
            let matches_os = if let Some(ref prof) = candidate.profile {
                prof.runtime.os.to_lowercase() == req_os_lower
            } else {
                candidate
                    .capabilities
                    .iter()
                    .any(|c| c.to_lowercase() == req_os_lower)
            };

            if matches_os {
                score += 15;
                matched_capabilities.push(format!("os:{}", req_os_lower));
                match_reasons.push(format!("Matches target OS: {}", req_os_lower));
            } else {
                is_eligible = false;
                missing_requirements.push(format!("Requires OS: {}", req_os_lower));
            }
        }

        // 4. Required languages
        for req_lang in &requirements.required_languages {
            let lang_lower = req_lang.to_lowercase();
            let matches_lang = if let Some(ref prof) = candidate.profile {
                prof.has_language(&lang_lower)
            } else {
                candidate
                    .capabilities
                    .iter()
                    .any(|c| c.to_lowercase() == lang_lower)
            };

            if matches_lang {
                score += 40;
                matched_capabilities.push(format!("lang:{}", lang_lower));
                match_reasons.push(format!("Supports required language: {}", lang_lower));
            } else {
                is_eligible = false;
                missing_requirements.push(format!("Missing required language: {}", lang_lower));
            }
        }

        // 5. Required tools
        for req_tool in &requirements.required_tools {
            let tool_lower = req_tool.to_lowercase();
            let matches_tool = if let Some(ref prof) = candidate.profile {
                prof.has_tool(&tool_lower)
                    || candidate
                        .capabilities
                        .iter()
                        .any(|c| c.to_lowercase() == tool_lower)
                    || (tool_lower == "cargo" && prof.has_language("rust"))
                    || (tool_lower == "flutter" && prof.has_language("dart"))
            } else {
                candidate.capabilities.iter().any(|c| {
                    let c_low = c.to_lowercase();
                    c_low == tool_lower
                        || (tool_lower == "cargo" && (c_low == "rust" || c_low == "backend"))
                        || (tool_lower == "flutter" && (c_low == "flutter" || c_low == "dart"))
                })
            };

            if matches_tool {
                score += 25;
                matched_capabilities.push(format!("tool:{}", tool_lower));
                match_reasons.push(format!("Has required tool: {}", tool_lower));
            } else {
                is_eligible = false;
                missing_requirements.push(format!("Missing required tool: {}", tool_lower));
            }
        }

        // 6. Required tags
        for req_tag in &requirements.required_tags {
            let tag_lower = req_tag.to_lowercase();
            let has_tag = candidate.capabilities.iter().any(|c| {
                let c_low = c.to_lowercase();
                c_low == tag_lower || c_low.contains(&tag_lower) || tag_lower.contains(&c_low)
            });

            if has_tag {
                score += 20;
                matched_capabilities.push(tag_lower.clone());
                match_reasons.push(format!("Has required skill tag: {}", tag_lower));
            } else {
                is_eligible = false;
                missing_requirements.push(format!("Missing required skill tag: {}", tag_lower));
            }
        }

        // 7. Preferred tags (soft match scoring)
        for pref_tag in &requirements.preferred_tags {
            let tag_lower = pref_tag.to_lowercase();
            let has_tag = candidate.capabilities.iter().any(|c| {
                let c_low = c.to_lowercase();
                c_low == tag_lower || c_low.contains(&tag_lower) || tag_lower.contains(&c_low)
            });

            if has_tag {
                score += 15;
                matched_capabilities.push(tag_lower.clone());
                match_reasons.push(format!("Matches preferred tag: {}", tag_lower));
            }
        }

        MatchScore {
            agent_id: candidate.agent_id,
            human_owner: candidate.human_owner.clone(),
            score,
            is_eligible,
            matched_capabilities,
            missing_requirements,
            match_reasons,
        }
    }

    /// Infers structured `TaskRequirements` from task title, description, and affected file paths.
    pub fn infer_task_requirements(
        title: &str,
        description: &str,
        affected_resources: &[String],
    ) -> TaskRequirements {
        let text = format!("{} {}", title, description).to_lowercase();
        let mut required_languages = Vec::new();
        let mut required_tools = Vec::new();
        let mut required_tags = Vec::new();
        let mut preferred_tags = Vec::new();
        let mut required_os = None;

        // 1. File extension heuristics
        for path in affected_resources {
            let path_lower = path.to_lowercase();
            if path_lower.ends_with(".rs") {
                required_languages.push("rust".to_string());
                preferred_tags.push("cargo".to_string());
                preferred_tags.push("backend".to_string());
            } else if path_lower.ends_with(".ts") || path_lower.ends_with(".tsx") {
                required_languages.push("typescript".to_string());
                preferred_tags.push("frontend".to_string());
            } else if path_lower.ends_with(".js") || path_lower.ends_with(".jsx") {
                required_languages.push("javascript".to_string());
                preferred_tags.push("frontend".to_string());
            } else if path_lower.ends_with(".py") {
                required_languages.push("python".to_string());
            } else if path_lower.ends_with(".go") {
                required_languages.push("go".to_string());
            } else if path_lower.ends_with(".dart") {
                required_languages.push("dart".to_string());
                preferred_tags.push("flutter".to_string());
            } else if path_lower.ends_with(".sql") {
                preferred_tags.push("sql".to_string());
                preferred_tags.push("database".to_string());
            }
        }

        // 2. Keyword heuristics
        if text.contains("rust") || text.contains("cargo") {
            if !required_languages.contains(&"rust".to_string()) {
                required_languages.push("rust".to_string());
            }
            preferred_tags.push("backend".to_string());
        }
        if text.contains("flutter") || text.contains("dart") {
            if !required_languages.contains(&"dart".to_string()) {
                required_languages.push("dart".to_string());
            }
            preferred_tags.push("flutter".to_string());
            preferred_tags.push("frontend".to_string());
        }
        if (text.contains("python") || text.contains("django") || text.contains("fastapi"))
            && !required_languages.contains(&"python".to_string())
        {
            required_languages.push("python".to_string());
        }
        if text.contains("react") || text.contains("frontend") || text.contains("ui") {
            preferred_tags.push("frontend".to_string());
        }
        if text.contains("backend") || text.contains("server") || text.contains("api") {
            preferred_tags.push("backend".to_string());
        }
        if text.contains("database")
            || text.contains("postgres")
            || text.contains("migration")
            || text.contains("sql")
        {
            preferred_tags.push("database".to_string());
            preferred_tags.push("sql".to_string());
        }
        if text.contains("docker") || text.contains("compose") || text.contains("devops") {
            required_tools.push("docker".to_string());
            preferred_tags.push("devops".to_string());
        }
        if text.contains("linux") {
            required_os = Some("linux".to_string());
        }

        required_languages.dedup();
        required_tools.dedup();
        required_tags.dedup();
        preferred_tags.dedup();

        TaskRequirements {
            required_languages,
            required_tools,
            required_os,
            required_tags,
            preferred_tags,
        }
    }

    /// Infers required skill and language tags (legacy helper for backward compatibility).
    pub fn infer_required_capabilities(
        title: &str,
        description: &str,
        affected_resources: &[String],
    ) -> Vec<String> {
        let reqs = Self::infer_task_requirements(title, description, affected_resources);
        let mut tags = Vec::new();
        tags.extend(reqs.required_languages);
        tags.extend(reqs.required_tools);
        tags.extend(reqs.required_tags);
        tags.extend(reqs.preferred_tags);
        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{LanguageCapability, RuntimeCapability, ToolCapability};

    #[test]
    fn test_agent_matching_by_file_and_keywords() {
        let rust_agent_id = Uuid::new_v4();
        let frontend_agent_id = Uuid::new_v4();

        let agents = vec![
            AvailableAgentContext {
                agent_id: rust_agent_id,
                human_owner: "Alice".to_string(),
                adapter_type: "Agy".to_string(),
                capabilities: vec![
                    "rust".to_string(),
                    "backend".to_string(),
                    "cargo".to_string(),
                ],
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

    #[test]
    fn test_capability_matching_example_from_spec() {
        // Agent A: [rust, linux, backend]
        // Agent B: [flutter, dart, frontend]
        let agent_a_id = Uuid::new_v4();
        let agent_b_id = Uuid::new_v4();

        let profile_a = AgentCapabilities::new(RuntimeCapability {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            adapter_type: "Agy".to_string(),
            agy_version: Some("0.4.0".to_string()),
            cpu_count: 8,
            memory_mb: None,
        })
        .with_language(LanguageCapability::new(
            "rust",
            Some("1.79.0".to_string()),
            vec![],
        ))
        .with_tool(ToolCapability::new("cargo", None, None))
        .with_tag("backend");

        let profile_b = AgentCapabilities::new(RuntimeCapability {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            adapter_type: "Agy".to_string(),
            agy_version: Some("0.4.0".to_string()),
            cpu_count: 8,
            memory_mb: None,
        })
        .with_language(LanguageCapability::new(
            "dart",
            Some("3.4.0".to_string()),
            vec!["flutter".to_string()],
        ))
        .with_tag("frontend");

        let candidates = vec![
            CandidateAgent {
                agent_id: agent_a_id,
                human_owner: "Agent A".to_string(),
                capabilities: profile_a.all_tags(),
                profile: Some(profile_a),
                is_available: true,
                health_status: HealthStatus::Healthy,
            },
            CandidateAgent {
                agent_id: agent_b_id,
                human_owner: "Agent B".to_string(),
                capabilities: profile_b.all_tags(),
                profile: Some(profile_b),
                is_available: true,
                health_status: HealthStatus::Healthy,
            },
        ];

        // 1. Rust backend task
        let rust_reqs = AgentCapabilityMatcher::infer_task_requirements(
            "Implement high-throughput pipeline",
            "Write asynchronous worker in rust",
            &["src/worker.rs".to_string()],
        );
        let ranked_rust = AgentCapabilityMatcher::rank_candidates(&candidates, &rust_reqs);
        assert_eq!(ranked_rust[0].agent_id, agent_a_id);
        assert!(ranked_rust[0].is_eligible);
        assert!(
            !ranked_rust[1].is_eligible,
            "Agent B should be ineligible because missing Rust"
        );

        // 2. Flutter frontend task
        let flutter_reqs = AgentCapabilityMatcher::infer_task_requirements(
            "Build mobile client screen",
            "Develop modern UI widgets with flutter dart",
            &["lib/ui/login.dart".to_string()],
        );
        let ranked_flutter = AgentCapabilityMatcher::rank_candidates(&candidates, &flutter_reqs);
        assert_eq!(ranked_flutter[0].agent_id, agent_b_id);
        assert!(ranked_flutter[0].is_eligible);
        assert!(
            !ranked_flutter[1].is_eligible,
            "Agent A should be ineligible because missing Dart"
        );
    }

    #[test]
    fn test_unhealthy_or_busy_agent_is_ineligible() {
        let agent_id = Uuid::new_v4();
        let candidate = CandidateAgent {
            agent_id,
            human_owner: "Alice".to_string(),
            capabilities: vec!["rust".to_string()],
            profile: None,
            is_available: false, // Busy!
            health_status: HealthStatus::Healthy,
        };

        let reqs = TaskRequirements {
            required_languages: vec!["rust".to_string()],
            ..Default::default()
        };

        let score = AgentCapabilityMatcher::evaluate_candidate(&candidate, &reqs);
        assert!(!score.is_eligible);
    }
}
