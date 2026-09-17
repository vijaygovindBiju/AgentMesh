use sqlx::PgPool;
use uuid::Uuid;

use coordinator::ai::mock::MockLlmProvider;
use coordinator::ai::schema::{
    AvailableAgentContext, PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask,
};
use coordinator::ai::service::PlanningService;
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, ProjectRepository, ProposalRepository, TaskRepository,
};
use coordinator::domain::{AdapterType, NewAgent, NewProject, TaskStatus};

async fn setup_test_pool() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;
    Some(pool)
}

#[tokio::test]
async fn test_ai_planning_service_full_workflow() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    // 1. Setup Project & Registered Agent
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "AI Planning E2E Project".to_string(),
            description: "Build an event-driven architecture".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "PlannerTestOwner".to_string(),
            api_key_hash: "secret_hash".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string(), "nats".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
        },
    )
    .await
    .unwrap();

    // 2. Build PlanningRequest
    let request = PlanningRequest {
        project_id: project.id,
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        available_agents: vec![AvailableAgentContext {
            agent_id: agent.id,
            human_owner: agent.human_owner.clone(),
            adapter_type: "Mock".to_string(),
            capabilities: agent.capabilities_list().into_iter().map(String::from).collect(),
        }],
        existing_tasks: vec![],
    };

    // 3. Configure MockLlmProvider with a valid 2-task plan with overlap
    let provider = MockLlmProvider::new().with_response(PlanningResponse {
        reasoning: "Separated messaging broker setup from publisher client".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "PLAN-1".to_string(),
                title: "Setup NATS broker".to_string(),
                description: "Configure JetStream streams".to_string(),
                suggested_agent_id: Some(agent.id),
                affected_resources: vec!["crates/coordinator/src/messaging/streams.rs".to_string()],
                estimated_size: Some("S".to_string()),
            },
            ProposedTask {
                short_id: "PLAN-2".to_string(),
                title: "Implement Publisher".to_string(),
                description: "Publish task assignments".to_string(),
                suggested_agent_id: Some(agent.id),
                affected_resources: vec!["crates/coordinator/src/messaging/streams.rs".to_string()],
                estimated_size: Some("M".to_string()),
            },
        ],
        proposed_dependencies: vec![ProposedDependency {
            dependent_short_id: "PLAN-2".to_string(),
            depends_on_short_id: "PLAN-1".to_string(),
            kind: "blocks".to_string(),
            reason: "Publisher requires stream to be provisioned first".to_string(),
        }],
    });

    // 4. Run PlanningService
    let proposal_id = PlanningService::generate_and_persist_plan(&pool, &provider, &request)
        .await
        .expect("Plan generation and persistence failed");

    // 5. Verify PostgreSQL State:
    // (A) Proposal is persisted in Pending state
    let proposal = ProposalRepository::find_by_id(&pool, proposal_id)
        .await
        .unwrap()
        .expect("Proposal must exist");
    assert_eq!(proposal.project_id, project.id);
    assert_eq!(proposal.ai_provider, "mock");

    // (B) Tasks are created and placed directly into HumanReview state
    let tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
    for t in &tasks {
        assert_eq!(
            t.status,
            TaskStatus::HumanReview,
            "All proposed tasks must be placed in HumanReview gate"
        );
        assert!(
            t.assigned_agent_id.is_none(),
            "Tasks must NOT be assigned prior to human approval"
        );
    }

    // (C) Dependencies are recorded
    let plan2 = tasks.iter().find(|t| t.short_id == "PLAN-2").unwrap();
    let deps = TaskRepository::list_dependencies(&pool, plan2.id).await.unwrap();
    assert_eq!(deps.len(), 1);
    let plan1 = tasks.iter().find(|t| t.short_id == "PLAN-1").unwrap();
    assert_eq!(deps[0].depends_on_id, plan1.id);

    // (D) Overlap is detected and recorded (sequential overlap -> Info)
    let overlaps = coordinator::db::repositories::OverlapWarningRepository::list_by_project(&pool, project.id)
        .await
        .unwrap();
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0].resource, "crates/coordinator/src/messaging/streams.rs");

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}

#[tokio::test]
async fn test_ai_planning_service_rejects_cycle_and_aborts() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Cycle Rejection Proj".to_string(),
            description: "Testing cycle rejection".to_string(),
        },
    )
    .await
    .unwrap();

    let request = PlanningRequest {
        project_id: project.id,
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        available_agents: vec![],
        existing_tasks: vec![],
    };

    // Configure provider with a cyclic plan (TASK-A depends on B, B depends on A)
    let provider = MockLlmProvider::new().with_response(PlanningResponse {
        reasoning: "Fatal cyclic plan".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "CYCLE-A".to_string(),
                title: "A".to_string(),
                description: "A".to_string(),
                suggested_agent_id: None,
                affected_resources: vec![],
                estimated_size: None,
            },
            ProposedTask {
                short_id: "CYCLE-B".to_string(),
                title: "B".to_string(),
                description: "B".to_string(),
                suggested_agent_id: None,
                affected_resources: vec![],
                estimated_size: None,
            },
        ],
        proposed_dependencies: vec![
            ProposedDependency {
                dependent_short_id: "CYCLE-A".to_string(),
                depends_on_short_id: "CYCLE-B".to_string(),
                kind: "blocks".to_string(),
                reason: "Cycle 1".to_string(),
            },
            ProposedDependency {
                dependent_short_id: "CYCLE-B".to_string(),
                depends_on_short_id: "CYCLE-A".to_string(),
                kind: "blocks".to_string(),
                reason: "Cycle 2".to_string(),
            },
        ],
    });

    let res = PlanningService::generate_and_persist_plan(&pool, &provider, &request).await;
    assert!(res.is_err(), "Cyclic plan must fail validation");

    // Verify NO tasks were persisted for this project
    let tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    assert_eq!(tasks.len(), 0, "No tasks should be persisted when validation fails");

    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

#[tokio::test]
async fn test_ai_planning_service_handles_provider_error_gracefully() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Error Handling Proj".to_string(),
            description: "Testing provider error".to_string(),
        },
    )
    .await
    .unwrap();

    let request = PlanningRequest {
        project_id: project.id,
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        available_agents: vec![],
        existing_tasks: vec![],
    };

    let provider = MockLlmProvider::new().with_error("Rate limit exceeded / malformed response");

    let res = PlanningService::generate_and_persist_plan(&pool, &provider, &request).await;
    assert!(res.is_err(), "Provider failure must return Err");
    let err_str = format!("{:#}", res.unwrap_err());
    assert!(err_str.contains("Rate limit exceeded"));

    ProjectRepository::delete(&pool, project.id).await.unwrap();
}
