use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;
use uuid::Uuid;

use crate::ai::schema::{PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask};
use crate::domain::OverlapSeverity;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("Plan contains no tasks: proposed_tasks must have at least one task")]
    EmptyPlan,

    #[error("Task short_id '{short_id}' has an empty or blank field: {field}")]
    EmptyField { short_id: String, field: &'static str },

    #[error("Duplicate task short_id detected: '{0}'")]
    DuplicateShortId(String),

    #[error("Task short_id '{short_id}' collides with an existing task in the project")]
    ShortIdCollisionWithExisting { short_id: String },

    #[error("Task '{0}' cannot depend on itself")]
    SelfDependency(String),

    #[error("Dependent task '{dependent}' references non-existent prerequisite task '{depends_on}'")]
    UnknownDependencyTarget { dependent: String, depends_on: String },

    #[error("Dependent task '{dependent}' is not defined in this proposed plan")]
    UnknownDependentTask { dependent: String },

    #[error("Dependency cycle detected involving tasks: {0:?}")]
    DependencyCycle(Vec<String>),

    #[error("Task '{short_id}' suggests unregistered agent ID: {agent_id}")]
    InvalidAgentReference { short_id: String, agent_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedOverlap {
    pub resource: String,
    pub task_short_ids: Vec<String>,
    pub severity: OverlapSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPlan {
    pub response: PlanningResponse,
    pub detected_overlaps: Vec<DetectedOverlap>,
}

pub struct PlanValidator;

impl PlanValidator {
    /// Deterministically validates a PlanningResponse against the PlanningRequest context.
    pub fn validate(
        request: &PlanningRequest,
        response: &PlanningResponse,
    ) -> Result<ValidatedPlan, ValidationError> {
        // 1. Check for empty plan
        if response.proposed_tasks.is_empty() {
            return Err(ValidationError::EmptyPlan);
        }

        // 2. Validate tasks: non-empty fields, uniqueness, and collision with existing tasks
        let mut proposed_ids = HashSet::new();
        let existing_ids: HashSet<&str> = request
            .existing_tasks
            .iter()
            .map(|t| t.short_id.as_str())
            .collect();

        for task in &response.proposed_tasks {
            let short_id = task.short_id.trim();
            if short_id.is_empty() {
                return Err(ValidationError::EmptyField {
                    short_id: task.short_id.clone(),
                    field: "short_id",
                });
            }
            if task.title.trim().is_empty() {
                return Err(ValidationError::EmptyField {
                    short_id: task.short_id.clone(),
                    field: "title",
                });
            }
            if task.description.trim().is_empty() {
                return Err(ValidationError::EmptyField {
                    short_id: task.short_id.clone(),
                    field: "description",
                });
            }

            if !proposed_ids.insert(short_id.to_string()) {
                return Err(ValidationError::DuplicateShortId(short_id.to_string()));
            }

            if existing_ids.contains(short_id) {
                return Err(ValidationError::ShortIdCollisionWithExisting {
                    short_id: short_id.to_string(),
                });
            }
        }

        // 3. Validate suggested agent references
        let available_agent_ids: HashSet<Uuid> = request
            .available_agents
            .iter()
            .map(|a| a.agent_id)
            .collect();

        for task in &response.proposed_tasks {
            if let Some(suggested_id) = task.suggested_agent_id {
                if !available_agent_ids.contains(&suggested_id) {
                    return Err(ValidationError::InvalidAgentReference {
                        short_id: task.short_id.clone(),
                        agent_id: suggested_id,
                    });
                }
            }
        }

        // 4. Validate dependency references (resolved against proposed + existing)
        for dep in &response.proposed_dependencies {
            let dep_from = dep.dependent_short_id.trim();
            let dep_to = dep.depends_on_short_id.trim();

            if dep_from == dep_to {
                return Err(ValidationError::SelfDependency(dep_from.to_string()));
            }

            if !proposed_ids.contains(dep_from) {
                return Err(ValidationError::UnknownDependentTask {
                    dependent: dep_from.to_string(),
                });
            }

            if !proposed_ids.contains(dep_to) && !existing_ids.contains(dep_to) {
                return Err(ValidationError::UnknownDependencyTarget {
                    dependent: dep_from.to_string(),
                    depends_on: dep_to.to_string(),
                });
            }
        }

        // 5. Detect dependency cycles among proposed tasks (Kahn's algorithm)
        Self::check_for_cycles(&response.proposed_tasks, &response.proposed_dependencies)?;

        // 6. Detect resource overlaps
        let detected_overlaps = Self::detect_resource_overlaps(
            &response.proposed_tasks,
            &response.proposed_dependencies,
        );

        Ok(ValidatedPlan {
            response: response.clone(),
            detected_overlaps,
        })
    }

    /// Validates that dependencies form a Directed Acyclic Graph (DAG) without cycles.
    fn check_for_cycles(
        tasks: &[ProposedTask],
        dependencies: &[ProposedDependency],
    ) -> Result<(), ValidationError> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        for t in tasks {
            in_degree.insert(t.short_id.as_str(), 0);
            adj.insert(t.short_id.as_str(), Vec::new());
        }

        // Only dependencies between proposed tasks can participate in proposed cycles
        for dep in dependencies {
            let from = dep.dependent_short_id.as_str();
            let to = dep.depends_on_short_id.as_str();

            // to -> from (prerequisite 'to' must complete before 'from' can execute)
            if adj.contains_key(to) && in_degree.contains_key(from) {
                adj.get_mut(to).unwrap().push(from);
                *in_degree.get_mut(from).unwrap() += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut visited_count = 0;
        while let Some(u) = queue.pop_front() {
            visited_count += 1;
            if let Some(neighbors) = adj.get(u) {
                for &v in neighbors {
                    let deg = in_degree.get_mut(v).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(v);
                    }
                }
            }
        }

        if visited_count != tasks.len() {
            // Find nodes with remaining in-degree for detailed error reporting
            let cycle_members: Vec<String> = in_degree
                .into_iter()
                .filter(|(_, deg)| *deg > 0)
                .map(|(id, _)| id.to_string())
                .collect();
            return Err(ValidationError::DependencyCycle(cycle_members));
        }

        Ok(())
    }

    /// Identifies resources shared by multiple tasks and flags potential concurrency conflicts.
    fn detect_resource_overlaps(
        tasks: &[ProposedTask],
        dependencies: &[ProposedDependency],
    ) -> Vec<DetectedOverlap> {
        let mut resource_to_tasks: HashMap<String, Vec<&str>> = HashMap::new();

        for task in tasks {
            for res in &task.affected_resources {
                let norm = res.trim().trim_matches('/').to_string();
                if !norm.is_empty() {
                    resource_to_tasks
                        .entry(norm)
                        .or_default()
                        .push(task.short_id.as_str());
                }
            }
        }

        // Build simple reachability set to check if two tasks are sequential
        let mut reachability: HashSet<(&str, &str)> = HashSet::new();
        for dep in dependencies {
            reachability.insert((
                dep.depends_on_short_id.as_str(),
                dep.dependent_short_id.as_str(),
            ));
        }

        // Transitive closure for sequential dependencies
        let task_ids: Vec<&str> = tasks.iter().map(|t| t.short_id.as_str()).collect();
        for &k in &task_ids {
            for &i in &task_ids {
                for &j in &task_ids {
                    if reachability.contains(&(i, k)) && reachability.contains(&(k, j)) {
                        reachability.insert((i, j));
                    }
                }
            }
        }

        let mut overlaps = Vec::new();
        for (resource, mut sharing_tasks) in resource_to_tasks {
            if sharing_tasks.len() > 1 {
                sharing_tasks.sort();
                sharing_tasks.dedup();

                // If any pair of tasks sharing this resource has NO dependency ordering,
                // concurrent modification is possible -> Critical / Warning
                let mut has_concurrent_pair = false;
                for i in 0..sharing_tasks.len() {
                    for j in (i + 1)..sharing_tasks.len() {
                        let t1 = sharing_tasks[i];
                        let t2 = sharing_tasks[j];
                        let is_sequential = reachability.contains(&(t1, t2))
                            || reachability.contains(&(t2, t1));
                        if !is_sequential {
                            has_concurrent_pair = true;
                            break;
                        }
                    }
                    if has_concurrent_pair {
                        break;
                    }
                }

                let severity = if has_concurrent_pair {
                    OverlapSeverity::Critical
                } else {
                    OverlapSeverity::Info
                };

                overlaps.push(DetectedOverlap {
                    resource,
                    task_short_ids: sharing_tasks.into_iter().map(String::from).collect(),
                    severity,
                });
            }
        }

        overlaps.sort_by(|a, b| a.resource.cmp(&b.resource));
        overlaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::schema::{AvailableAgentContext, ExistingTaskContext};

    fn make_request() -> PlanningRequest {
        PlanningRequest {
            project_id: Uuid::new_v4(),
            project_name: "Test Project".to_string(),
            project_description: "Build an API".to_string(),
            available_agents: vec![AvailableAgentContext {
                agent_id: Uuid::from_u128(1),
                human_owner: "Alice".to_string(),
                adapter_type: "Mock".to_string(),
                capabilities: vec!["rust".to_string()],
            }],
            existing_tasks: vec![ExistingTaskContext {
                task_id: Uuid::new_v4(),
                short_id: "EXISTING-001".to_string(),
                title: "Existing base task".to_string(),
                status: "completed".to_string(),
                affected_resources: vec!["Cargo.toml".to_string()],
            }],
        }
    }

    #[test]
    fn test_valid_plan_passes_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Solid architecture plan".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-1".to_string(),
                    title: "Setup database".to_string(),
                    description: "Create tables".to_string(),
                    suggested_agent_id: Some(Uuid::from_u128(1)),
                    affected_resources: vec!["migrations/001.sql".to_string()],
                    estimated_size: Some("S".to_string()),
                },
                ProposedTask {
                    short_id: "TASK-2".to_string(),
                    title: "Implement models".to_string(),
                    description: "User structs".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec!["src/models.rs".to_string()],
                    estimated_size: Some("M".to_string()),
                },
            ],
            proposed_dependencies: vec![
                ProposedDependency {
                    dependent_short_id: "TASK-2".to_string(),
                    depends_on_short_id: "TASK-1".to_string(),
                    kind: "blocks".to_string(),
                    reason: "Need DB first".to_string(),
                },
                // Depends on existing project task
                ProposedDependency {
                    dependent_short_id: "TASK-1".to_string(),
                    depends_on_short_id: "EXISTING-001".to_string(),
                    kind: "blocks".to_string(),
                    reason: "Need base Cargo.toml".to_string(),
                },
            ],
        };

        let result = PlanValidator::validate(&req, &resp);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_plan_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Nothing to do".to_string(),
            proposed_tasks: vec![],
            proposed_dependencies: vec![],
        };

        assert_eq!(
            PlanValidator::validate(&req, &resp).unwrap_err(),
            ValidationError::EmptyPlan
        );
    }

    #[test]
    fn test_duplicate_short_id_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Duplicate tasks".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-1".to_string(),
                    title: "First".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "TASK-1".to_string(),
                    title: "Second".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
            ],
            proposed_dependencies: vec![],
        };

        assert_eq!(
            PlanValidator::validate(&req, &resp).unwrap_err(),
            ValidationError::DuplicateShortId("TASK-1".to_string())
        );
    }

    #[test]
    fn test_collision_with_existing_task_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Collision".to_string(),
            proposed_tasks: vec![ProposedTask {
                short_id: "EXISTING-001".to_string(),
                title: "Colliding task".to_string(),
                description: "Desc".to_string(),
                suggested_agent_id: None,
                affected_resources: vec![],
                estimated_size: None,
            }],
            proposed_dependencies: vec![],
        };

        assert_eq!(
            PlanValidator::validate(&req, &resp).unwrap_err(),
            ValidationError::ShortIdCollisionWithExisting {
                short_id: "EXISTING-001".to_string()
            }
        );
    }

    #[test]
    fn test_self_dependency_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Self dep".to_string(),
            proposed_tasks: vec![ProposedTask {
                short_id: "TASK-1".to_string(),
                title: "Self loop".to_string(),
                description: "Desc".to_string(),
                suggested_agent_id: None,
                affected_resources: vec![],
                estimated_size: None,
            }],
            proposed_dependencies: vec![ProposedDependency {
                dependent_short_id: "TASK-1".to_string(),
                depends_on_short_id: "TASK-1".to_string(),
                kind: "blocks".to_string(),
                reason: "Myself".to_string(),
            }],
        };

        assert_eq!(
            PlanValidator::validate(&req, &resp).unwrap_err(),
            ValidationError::SelfDependency("TASK-1".to_string())
        );
    }

    #[test]
    fn test_direct_dependency_cycle_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Direct cycle".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-A".to_string(),
                    title: "A".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "TASK-B".to_string(),
                    title: "B".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
            ],
            proposed_dependencies: vec![
                ProposedDependency {
                    dependent_short_id: "TASK-A".to_string(),
                    depends_on_short_id: "TASK-B".to_string(),
                    kind: "blocks".to_string(),
                    reason: "A needs B".to_string(),
                },
                ProposedDependency {
                    dependent_short_id: "TASK-B".to_string(),
                    depends_on_short_id: "TASK-A".to_string(),
                    kind: "blocks".to_string(),
                    reason: "B needs A".to_string(),
                },
            ],
        };

        match PlanValidator::validate(&req, &resp).unwrap_err() {
            ValidationError::DependencyCycle(cycle) => {
                assert!(cycle.contains(&"TASK-A".to_string()));
                assert!(cycle.contains(&"TASK-B".to_string()));
            }
            other => panic!("Expected DependencyCycle, got {other:?}"),
        }
    }

    #[test]
    fn test_indirect_dependency_cycle_fails_validation() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Indirect cycle A -> B -> C -> A".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "A".to_string(),
                    title: "A".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "B".to_string(),
                    title: "B".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "C".to_string(),
                    title: "C".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec![],
                    estimated_size: None,
                },
            ],
            proposed_dependencies: vec![
                ProposedDependency {
                    dependent_short_id: "B".to_string(),
                    depends_on_short_id: "A".to_string(),
                    kind: "blocks".to_string(),
                    reason: "B needs A".to_string(),
                },
                ProposedDependency {
                    dependent_short_id: "C".to_string(),
                    depends_on_short_id: "B".to_string(),
                    kind: "blocks".to_string(),
                    reason: "C needs B".to_string(),
                },
                ProposedDependency {
                    dependent_short_id: "A".to_string(),
                    depends_on_short_id: "C".to_string(),
                    kind: "blocks".to_string(),
                    reason: "A needs C -> CYCLE".to_string(),
                },
            ],
        };

        match PlanValidator::validate(&req, &resp).unwrap_err() {
            ValidationError::DependencyCycle(cycle) => {
                assert_eq!(cycle.len(), 3);
            }
            other => panic!("Expected DependencyCycle, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_agent_reference_fails_validation() {
        let req = make_request();
        let fake_agent = Uuid::new_v4();
        let resp = PlanningResponse {
            reasoning: "Bad agent".to_string(),
            proposed_tasks: vec![ProposedTask {
                short_id: "TASK-1".to_string(),
                title: "Task with fake agent".to_string(),
                description: "Desc".to_string(),
                suggested_agent_id: Some(fake_agent),
                affected_resources: vec![],
                estimated_size: None,
            }],
            proposed_dependencies: vec![],
        };

        assert_eq!(
            PlanValidator::validate(&req, &resp).unwrap_err(),
            ValidationError::InvalidAgentReference {
                short_id: "TASK-1".to_string(),
                agent_id: fake_agent,
            }
        );
    }

    #[test]
    fn test_resource_overlap_detection_critical_when_concurrent() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Resource overlap".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-A".to_string(),
                    title: "Write user model".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec!["src/models/user.rs".to_string()],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "TASK-B".to_string(),
                    title: "Write auth routes".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec!["src/models/user.rs".to_string()],
                    estimated_size: None,
                },
            ],
            // No dependency between A and B -> concurrent modification risk!
            proposed_dependencies: vec![],
        };

        let validated = PlanValidator::validate(&req, &resp).expect("Validation should pass");
        assert_eq!(validated.detected_overlaps.len(), 1);
        assert_eq!(
            validated.detected_overlaps[0].severity,
            OverlapSeverity::Critical
        );
        assert_eq!(
            validated.detected_overlaps[0].resource,
            "src/models/user.rs"
        );
    }

    #[test]
    fn test_resource_overlap_detection_info_when_sequential() {
        let req = make_request();
        let resp = PlanningResponse {
            reasoning: "Resource overlap but sequential".to_string(),
            proposed_tasks: vec![
                ProposedTask {
                    short_id: "TASK-A".to_string(),
                    title: "Create schema".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec!["src/schema.rs".to_string()],
                    estimated_size: None,
                },
                ProposedTask {
                    short_id: "TASK-B".to_string(),
                    title: "Use schema".to_string(),
                    description: "Desc".to_string(),
                    suggested_agent_id: None,
                    affected_resources: vec!["src/schema.rs".to_string()],
                    estimated_size: None,
                },
            ],
            // B depends on A -> sequential execution!
            proposed_dependencies: vec![ProposedDependency {
                dependent_short_id: "TASK-B".to_string(),
                depends_on_short_id: "TASK-A".to_string(),
                kind: "blocks".to_string(),
                reason: "B modifies schema after A completes".to_string(),
            }],
        };

        let validated = PlanValidator::validate(&req, &resp).expect("Validation should pass");
        assert_eq!(validated.detected_overlaps.len(), 1);
        assert_eq!(
            validated.detected_overlaps[0].severity,
            OverlapSeverity::Info
        );
    }
}
