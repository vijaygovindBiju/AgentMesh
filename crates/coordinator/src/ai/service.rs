use std::collections::HashMap;
use std::path::Path;
use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::ai::complexity::ComplexityEstimator;
use crate::ai::matcher::AgentCapabilityMatcher;
use crate::ai::provider::LlmProvider;
use crate::ai::replan::ReplanEngine;
use crate::ai::repo_scanner::RepositoryScanner;
use crate::ai::schema::PlanningRequest;
use crate::ai::validator::PlanValidator;
use crate::db::repositories::{
    OverlapWarningRepository, ProposalRepository, TaskRepository,
};
use crate::domain::{
    DependencyKind, NewOverlapWarning, NewProposal, NewTask, NewTaskDependency, TaskStatus,
};

pub struct PlanningService;

impl PlanningService {
    /// Executes repository-aware plan decomposition by scanning the project's codebase
    /// and injecting architecture context into the planning prompt.
    pub async fn generate_repo_aware_plan(
        pool: &PgPool,
        provider: &dyn LlmProvider,
        repo_root: impl AsRef<Path>,
        mut request: PlanningRequest,
    ) -> Result<Uuid> {
        let repo_context = RepositoryScanner::scan(repo_root).await.ok();
        request.repo_context = repo_context;
        Self::generate_and_persist_plan(pool, provider, &request).await
    }

    /// Generates a corrective re-plan when tasks fail, complete, or project state shifts.
    pub async fn generate_replan(
        pool: &PgPool,
        provider: &dyn LlmProvider,
        project_id: Uuid,
        repo_root: Option<&Path>,
    ) -> Result<Uuid> {
        let repo_context = match repo_root {
            Some(path) => RepositoryScanner::scan(path).await.ok(),
            None => None,
        };

        let replan_req = ReplanEngine::gather_replan_context(pool, project_id, repo_context).await?;
        ReplanEngine::execute_replan(pool, provider, &replan_req).await
    }

    /// Executes the complete AI proposal workflow:
    /// 1. Calls LlmProvider to generate structured PlanResponse
    /// 2. Runs deterministic validation (cycles, schemas, dependencies, agents, overlaps)
    /// 3. Injects capability matching and task complexity estimates
    /// 4. Persists Proposal in PostgreSQL (status = Pending)
    /// 5. Persists Tasks (status = Proposed -> HumanReview)
    /// 6. Persists TaskDependencies
    /// 7. Persists OverlapWarnings
    ///
    /// DOES NOT assign tasks or publish to NATS. Tasks wait for Human Review.
    pub async fn generate_and_persist_plan(
        pool: &PgPool,
        provider: &dyn LlmProvider,
        request: &PlanningRequest,
    ) -> Result<Uuid> {
        info!(
            project_id = %request.project_id,
            provider = provider.provider_name(),
            model = provider.model_name(),
            has_repo_context = request.repo_context.is_some(),
            "Requesting AI plan decomposition"
        );

        // 1. Call provider
        let response = provider
            .propose_plan(request)
            .await
            .context("LLM provider failed to generate plan")?;

        // 2. Deterministic validation
        let validated = PlanValidator::validate(request, &response)
            .context("Plan failed deterministic coordinator validation")?;

        // 3. Intelligent enrichment: capability matching and complexity estimation
        let mut enriched_response = validated.response.clone();
        for pt in &mut enriched_response.proposed_tasks {
            if pt.suggested_agent_id.is_none() {
                pt.suggested_agent_id = AgentCapabilityMatcher::suggest_agent(
                    &request.available_agents,
                    &pt.title,
                    &pt.description,
                    &pt.affected_resources,
                );
            }

            if pt.estimated_size.is_none() {
                let dep_count = enriched_response
                    .proposed_dependencies
                    .iter()
                    .filter(|d| d.dependent_short_id == pt.short_id)
                    .count();
                pt.estimated_size = Some(
                    ComplexityEstimator::estimate(&pt.title, &pt.description, &pt.affected_resources, dep_count)
                        .estimated_size,
                );
            }
        }

        let raw_prompt = format!(
            "Project: {}\nDescription: {}",
            request.project_name, request.project_description
        );
        let raw_response = serde_json::to_string(&enriched_response)?;

        // 4. Persist Proposal record
        let proposal = ProposalRepository::create(
            pool,
            &NewProposal {
                project_id: request.project_id,
                ai_provider: provider.provider_name().to_string(),
                ai_model: provider.model_name().to_string(),
                raw_prompt,
                raw_response,
            },
        )
        .await
        .context("Failed to persist proposal record")?;

        // 5. Persist Tasks (starts in Proposed, then advanced to HumanReview; unassigned until human approves)
        let mut short_id_to_uuid: HashMap<String, Uuid> = HashMap::new();

        for pt in &enriched_response.proposed_tasks {
            let task = TaskRepository::create(
                pool,
                &NewTask {
                    project_id: request.project_id,
                    short_id: pt.short_id.clone(),
                    title: pt.title.clone(),
                    description: pt.description.clone(),
                    affected_resources: pt.affected_resources.clone(),
                    estimated_size: pt.estimated_size.clone(),
                    proposal_id: proposal.id,
                },
            )
            .await
            .with_context(|| format!("Failed to create task {}", pt.short_id))?;

            // Move to HumanReview for human gate
            TaskRepository::update_status(pool, task.id, TaskStatus::HumanReview).await?;

            short_id_to_uuid.insert(pt.short_id.clone(), task.id);
        }

        // Map existing tasks into ID map for dependency resolution
        for et in &request.existing_tasks {
            short_id_to_uuid.insert(et.short_id.clone(), et.task_id);
        }

        // 5. Persist Dependencies
        for dep in &validated.response.proposed_dependencies {
            if let (Some(&dependent_id), Some(&depends_on_id)) = (
                short_id_to_uuid.get(&dep.dependent_short_id),
                short_id_to_uuid.get(&dep.depends_on_short_id),
            ) {
                let kind = if dep.kind == "relates_to" {
                    DependencyKind::RelatesTo
                } else {
                    DependencyKind::Blocks
                };

                TaskRepository::add_dependency(
                    pool,
                    &NewTaskDependency {
                        dependent_id,
                        depends_on_id,
                        kind,
                    },
                )
                .await
                .with_context(|| {
                    format!(
                        "Failed to add dependency {} -> {}",
                        dep.dependent_short_id, dep.depends_on_short_id
                    )
                })?;
            }
        }

        // 6. Persist Overlap Warnings
        for overlap in &validated.detected_overlaps {
            let task_uuids: Vec<Uuid> = overlap
                .task_short_ids
                .iter()
                .filter_map(|s| short_id_to_uuid.get(s).copied())
                .collect();

            if !task_uuids.is_empty() {
                OverlapWarningRepository::create(
                    pool,
                    &NewOverlapWarning {
                        project_id: request.project_id,
                        task_ids: task_uuids,
                        resource: overlap.resource.clone(),
                        severity: overlap.severity,
                    },
                )
                .await
                .with_context(|| format!("Failed to record overlap for {}", overlap.resource))?;
            }
        }

        info!(
            proposal_id = %proposal.id,
            tasks_count = validated.response.proposed_tasks.len(),
            overlaps_count = validated.detected_overlaps.len(),
            "AI plan successfully validated and queued for Human Review"
        );

        Ok(proposal.id)
    }
}
