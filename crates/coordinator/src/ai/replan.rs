use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ai::matcher::AgentCapabilityMatcher;
use crate::ai::complexity::ComplexityEstimator;
use crate::ai::provider::LlmProvider;
use crate::ai::repo_scanner::RepositoryContext;
use crate::ai::schema::{
    AvailableAgentContext, CompletedTaskContext, ExistingTaskContext, FailedTaskContext,
    PlanningRequest, ReplanRequest,
};
use crate::ai::validator::PlanValidator;
use crate::db::repositories::{
    AgentRepository, GitConflictRepository, ProjectRepository, ProposalRepository, TaskRepository,
    UnexpectedResourceRepository,
};
use crate::domain::{
    DependencyKind, NewProposal, NewTask, NewTaskDependency, TaskStatus,
};

/// Orchestrates re-planning when a task completes, fails, or when project state changes.
pub struct ReplanEngine;

impl ReplanEngine {
    /// Gathers all current project state, active/failed/completed tasks, unexpected changes,
    /// and unresolved conflicts into a structured `ReplanRequest`.
    pub async fn gather_replan_context(
        pool: &PgPool,
        project_id: Uuid,
        repo_context: Option<RepositoryContext>,
    ) -> Result<ReplanRequest> {
        let project = ProjectRepository::find_by_id(pool, project_id)
            .await?
            .context("Project not found")?;

        let tasks = TaskRepository::list_by_project(pool, project_id).await?;
        let agents = AgentRepository::list(pool).await?;

        let available_agents: Vec<AvailableAgentContext> = agents
            .into_iter()
            .map(|a| {
                let caps = a.capabilities_list();
                AvailableAgentContext {
                    agent_id: a.id,
                    human_owner: a.human_owner,
                    adapter_type: format!("{:?}", a.adapter_type),
                    capabilities: caps,
                }
            })
            .collect();

        let all_task_ids: Vec<Uuid> = tasks.iter().map(|t| t.id).collect();
        let mut completed_tasks = Vec::new();
        let mut failed_tasks = Vec::new();
        let mut active_or_pending_tasks = Vec::new();

        for t in tasks {
            match t.status {
                TaskStatus::Completed => {
                    let git_ctx = TaskRepository::find_git_context(pool, t.id).await.unwrap_or(None);
                    let actual_mods = git_ctx
                        .map(|g| g.actual_modified_resources)
                        .unwrap_or_else(|| t.resources());
                    completed_tasks.push(CompletedTaskContext {
                        task_id: t.id,
                        short_id: t.short_id,
                        title: t.title,
                        actual_modified_resources: actual_mods,
                    });
                }
                TaskStatus::Failed | TaskStatus::Blocked => {
                    failed_tasks.push(FailedTaskContext {
                        task_id: t.id,
                        short_id: t.short_id,
                        title: t.title,
                        error: "Task failed during execution".to_string(),
                        blocker_reason: None,
                    });
                }
                _ => {
                    let resources = t.resources();
                    active_or_pending_tasks.push(ExistingTaskContext {
                        task_id: t.id,
                        short_id: t.short_id,
                        title: t.title,
                        status: format!("{:?}", t.status),
                        affected_resources: resources,
                    });
                }
            }
        }

        // Fetch unexpected resource changes across all project tasks
        let mut unexpected_changes = Vec::new();
        for tid in all_task_ids {
            if let Ok(unexp) = UnexpectedResourceRepository::list_by_task(pool, tid).await {
                for u in unexp {
                    unexpected_changes.push(format!("{}: {}", u.resource_path, u.reason));
                }
            }
        }

        // Fetch unresolved conflicts
        let conflicts = GitConflictRepository::list_unresolved(pool, project_id).await.unwrap_or_default();
        let cross_agent_conflicts = conflicts
            .into_iter()
            .map(|c| format!("Conflict on '{}': {}", c.conflicting_path, c.description))
            .collect();

        Ok(ReplanRequest {
            project_id,
            project_name: project.name,
            project_description: project.description,
            available_agents,
            completed_tasks,
            failed_tasks,
            active_or_pending_tasks,
            unexpected_changes,
            cross_agent_conflicts,
            repo_context,
        })
    }

    /// Executes the re-planning workflow:
    /// 1. Prompts LLM for corrective plan
    /// 2. Deterministically validates the corrective plan
    /// 3. Injects capability matching and complexity estimation
    /// 4. Persists the new proposal, new tasks, and dependencies
    pub async fn execute_replan(
        pool: &PgPool,
        provider: &dyn LlmProvider,
        replan_req: &ReplanRequest,
    ) -> Result<Uuid> {
        // Convert ReplanRequest to PlanningRequest for standard LLM provider compatibility
        let planning_req = PlanningRequest {
            project_id: replan_req.project_id,
            project_name: replan_req.project_name.clone(),
            project_description: format!(
                "{}\n\n[REPLAN CONTEXT]\nCompleted Tasks: {}\nFailed Tasks: {}\nUnexpected Changes: {}\nConflicts: {}",
                replan_req.project_description,
                replan_req.completed_tasks.len(),
                replan_req.failed_tasks.len(),
                replan_req.unexpected_changes.len(),
                replan_req.cross_agent_conflicts.len(),
            ),
            available_agents: replan_req.available_agents.clone(),
            existing_tasks: replan_req.active_or_pending_tasks.clone(),
            repo_context: replan_req.repo_context.clone(),
        };

        let response = provider.propose_plan(&planning_req).await?;
        let validated = PlanValidator::validate(&planning_req, &response)?;

        let mut enriched_response = validated.response.clone();
        for pt in &mut enriched_response.proposed_tasks {
            if pt.suggested_agent_id.is_none() {
                pt.suggested_agent_id = AgentCapabilityMatcher::suggest_agent(
                    &replan_req.available_agents,
                    &pt.title,
                    &pt.description,
                    &pt.affected_resources,
                );
            }

            if pt.estimated_size.is_none() {
                pt.estimated_size = Some(
                    ComplexityEstimator::estimate(&pt.title, &pt.description, &pt.affected_resources, 0)
                        .estimated_size,
                );
            }
        }

        let proposal = ProposalRepository::create(
            pool,
            &NewProposal {
                project_id: replan_req.project_id,
                ai_provider: provider.provider_name().to_string(),
                ai_model: provider.model_name().to_string(),
                raw_prompt: format!("Re-plan for project {}", replan_req.project_id),
                raw_response: serde_json::to_string(&enriched_response)?,
            },
        )
        .await?;

        let mut short_id_to_uuid = std::collections::HashMap::new();

        for pt in &enriched_response.proposed_tasks {
            let task = TaskRepository::create(
                pool,
                &NewTask {
                    project_id: replan_req.project_id,
                    short_id: pt.short_id.clone(),
                    title: pt.title.clone(),
                    description: pt.description.clone(),
                    affected_resources: pt.affected_resources.clone(),
                    estimated_size: pt.estimated_size.clone(),
                    proposal_id: proposal.id,
                },
            )
            .await?;

            TaskRepository::update_status(pool, task.id, TaskStatus::HumanReview).await?;
            short_id_to_uuid.insert(pt.short_id.clone(), task.id);
        }

        for dep in &validated.response.proposed_dependencies {
            if let (Some(&dep_id), Some(&parent_id)) = (
                short_id_to_uuid.get(&dep.dependent_short_id),
                short_id_to_uuid.get(&dep.depends_on_short_id),
            ) {
                let kind = if dep.kind == "relates_to" {
                    DependencyKind::RelatesTo
                } else {
                    DependencyKind::Blocks
                };
                let _ = TaskRepository::add_dependency(
                    pool,
                    &NewTaskDependency {
                        dependent_id: dep_id,
                        depends_on_id: parent_id,
                        kind,
                    },
                )
                .await;
            }
        }

        Ok(proposal.id)
    }
}
