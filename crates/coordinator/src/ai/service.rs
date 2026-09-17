use std::collections::HashMap;
use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::ai::provider::LlmProvider;
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
    /// Executes the complete AI proposal workflow:
    /// 1. Calls LlmProvider to generate structured PlanResponse
    /// 2. Runs deterministic validation (cycles, schemas, dependencies, agents, overlaps)
    /// 3. Persists Proposal in PostgreSQL (status = Pending)
    /// 4. Persists Tasks (status = Proposed -> HumanReview)
    /// 5. Persists TaskDependencies
    /// 6. Persists OverlapWarnings
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

        let raw_prompt = format!(
            "Project: {}\nDescription: {}",
            request.project_name, request.project_description
        );
        let raw_response = serde_json::to_string(&response)?;

        // 3. Persist Proposal record
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

        // 4. Persist Tasks (starts in Proposed, then advanced to HumanReview)
        let mut short_id_to_uuid: HashMap<String, Uuid> = HashMap::new();

        for pt in &validated.response.proposed_tasks {
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
