use sqlx::PgPool;
use uuid::Uuid;

use coordinator::ai::complexity::ComplexityEstimator;
use coordinator::ai::matcher::AgentCapabilityMatcher;
use coordinator::ai::mock::MockLlmProvider;
use coordinator::ai::prompts::{PlanningPrompt, ReplanPrompt};
use coordinator::ai::replan::ReplanEngine;
use coordinator::ai::repo_scanner::RepositoryScanner;
use coordinator::ai::schema::{
    AvailableAgentContext, PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask,
};
use coordinator::ai::service::PlanningService;
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, ProposalRepository,
    TaskRepository, UnexpectedResourceRepository,
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
async fn test_repo_scanner_discovers_architecture_and_modules() {
    // 1. Scan current repository
    let current_dir = std::env::current_dir().expect("Failed to get current dir");
    let ctx = RepositoryScanner::scan(&current_dir)
        .await
        .expect("Failed to scan current repository");

    assert!(
        ctx.detected_ecosystems.iter().any(|e| e.contains("Rust")),
        "Must detect Rust ecosystem in current repo"
    );
    assert!(
        ctx.primary_languages.iter().any(|l| l == "Rust"),
        "Must identify Rust as primary language"
    );
    assert!(
        ctx.key_modules.iter().any(|m| m.contains("coordinator") || m == "crates"),
        "Must identify crates or coordinator as key module"
    );
    assert!(
        !ctx.file_tree_sample.is_empty(),
        "Must collect file tree samples"
    );
    assert!(
        ctx.readme_summary.is_some(),
        "Must find and sample README summary"
    );

    // 2. Scan simulated polyglot repository in temp dir
    let temp_dir = std::env::temp_dir().join(format!("polyglot_test_{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(temp_dir.join("backend/src")).await.unwrap();
    tokio::fs::create_dir_all(temp_dir.join("frontend/src")).await.unwrap();
    tokio::fs::create_dir_all(temp_dir.join("migrations")).await.unwrap();

    tokio::fs::write(
        temp_dir.join("backend/Cargo.toml"),
        "[package]\nname = \"backend\"\nversion = \"0.1.0\"\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"backend\"]\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("frontend/package.json"),
        "{\"name\": \"frontend\", \"version\": \"1.0.0\"}",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("frontend/tsconfig.json"),
        "{\"compilerOptions\": {}}",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("requirements.txt"),
        "fastapi>=0.100.0\nuvicorn>=0.22.0\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("go.mod"),
        "module github.com/agentmesh/testmod\n\ngo 1.22\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        temp_dir.join("README.md"),
        "# Polyglot Platform\nFull-stack platform with Rust, TypeScript, Python, and Go.",
    )
    .await
    .unwrap();

    let polyglot_ctx = RepositoryScanner::scan(&temp_dir)
        .await
        .expect("Failed to scan polyglot repository");

    assert!(polyglot_ctx.detected_ecosystems.iter().any(|e| e.contains("Rust")));
    assert!(polyglot_ctx.detected_ecosystems.iter().any(|e| e.contains("Node")));
    assert!(polyglot_ctx.detected_ecosystems.iter().any(|e| e.contains("Python")));
    assert!(polyglot_ctx.detected_ecosystems.iter().any(|e| e.contains("Go")));
    assert!(polyglot_ctx.primary_languages.contains(&"Rust".to_string()));
    assert!(polyglot_ctx.primary_languages.contains(&"TypeScript".to_string()));
    assert!(polyglot_ctx.primary_languages.contains(&"Python".to_string()));
    assert!(polyglot_ctx.primary_languages.contains(&"Go".to_string()));
    assert!(polyglot_ctx.key_modules.contains(&"backend".to_string()));

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[test]
fn test_repository_aware_planning_prompt_generation() {
    let current_dir = std::env::current_dir().unwrap();
    let repo_ctx = coordinator::ai::repo_scanner::RepositoryContext {
        repo_root: current_dir.clone(),
        detected_ecosystems: vec!["Rust / Cargo".to_string(), "PostgreSQL".to_string()],
        primary_languages: vec!["Rust".to_string(), "SQL".to_string()],
        key_modules: vec!["crates/coordinator".to_string(), "crates/agent-protocol".to_string()],
        file_tree_sample: vec![
            "crates/coordinator/src/main.rs".to_string(),
            "crates/coordinator/src/ai/service.rs".to_string(),
        ],
        readme_summary: Some("# AgentMesh Architecture\nDistributed agent coordination.".to_string()),
    };

    let request = PlanningRequest {
        project_id: Uuid::new_v4(),
        project_name: "AgentMesh Planning Test".to_string(),
        project_description: "Add intelligent planning capability".to_string(),
        available_agents: vec![AvailableAgentContext {
            agent_id: Uuid::new_v4(),
            human_owner: "Alice".to_string(),
            adapter_type: "Agy".to_string(),
            capabilities: vec!["rust".to_string(), "backend".to_string()],
        }],
        existing_tasks: vec![],
        repo_context: Some(repo_ctx),
    };

    let user_prompt = PlanningPrompt::user_prompt(&request);

    assert!(user_prompt.contains("REPOSITORY ARCHITECTURE & CODEBASE CONTEXT:"));
    assert!(user_prompt.contains("Detected Ecosystems: Rust / Cargo, PostgreSQL"));
    assert!(user_prompt.contains("Primary Languages: Rust, SQL"));
    assert!(user_prompt.contains("crates/coordinator"));
    assert!(user_prompt.contains("crates/coordinator/src/main.rs"));
    assert!(user_prompt.contains("AgentMesh Architecture"));
    assert!(user_prompt.contains("Alice"));
}

#[test]
fn test_agent_capability_matcher_heuristics() {
    let rust_agent_id = Uuid::new_v4();
    let ts_agent_id = Uuid::new_v4();
    let py_agent_id = Uuid::new_v4();
    let db_agent_id = Uuid::new_v4();

    let agents = vec![
        AvailableAgentContext {
            agent_id: rust_agent_id,
            human_owner: "RustDev".to_string(),
            adapter_type: "Agy".to_string(),
            capabilities: vec!["rust".to_string(), "backend".to_string()],
        },
        AvailableAgentContext {
            agent_id: ts_agent_id,
            human_owner: "FrontDev".to_string(),
            adapter_type: "Agy".to_string(),
            capabilities: vec!["frontend".to_string(), "typescript".to_string(), "react".to_string()],
        },
        AvailableAgentContext {
            agent_id: py_agent_id,
            human_owner: "DataDev".to_string(),
            adapter_type: "Agy".to_string(),
            capabilities: vec!["python".to_string(), "fastapi".to_string()],
        },
        AvailableAgentContext {
            agent_id: db_agent_id,
            human_owner: "DbaDev".to_string(),
            adapter_type: "Agy".to_string(),
            capabilities: vec!["database".to_string(), "sql".to_string(), "db".to_string()],
        },
    ];

    // 1. Rust backend task
    let match_rust = AgentCapabilityMatcher::suggest_agent(
        &agents,
        "Implement NATS JetStream Consumer",
        "Build reliable event consumer in Rust",
        &["crates/coordinator/src/messaging/consumer.rs".to_string()],
    );
    assert_eq!(match_rust, Some(rust_agent_id));

    // 2. TypeScript frontend task
    let match_ts = AgentCapabilityMatcher::suggest_agent(
        &agents,
        "Build Task Review Panel",
        "Implement React review screen with keyboard navigation",
        &["frontend/src/components/TaskReview.tsx".to_string()],
    );
    assert_eq!(match_ts, Some(ts_agent_id));

    // 3. Database migration task
    let match_db = AgentCapabilityMatcher::suggest_agent(
        &agents,
        "Add Task Dependencies Migration",
        "Postgres database schema migration for task dependencies",
        &["migrations/0008_task_dependencies.sql".to_string()],
    );
    assert_eq!(match_db, Some(db_agent_id));

    // 4. Python task
    let match_py = AgentCapabilityMatcher::suggest_agent(
        &agents,
        "FastAPI Ingestion Endpoint",
        "Expose Python FastAPI telemetry receiver",
        &["services/telemetry/main.py".to_string()],
    );
    assert_eq!(match_py, Some(py_agent_id));
}

#[test]
fn test_complexity_estimator_sizing_and_risk_factors() {
    // 1. XS / Small task
    let est_small = ComplexityEstimator::estimate(
        "Update Contributing Guide",
        "Fix documentation formatting in CONTRIBUTING.md",
        &["CONTRIBUTING.md".to_string()],
        0,
    );
    assert!(est_small.estimated_size == "XS" || est_small.estimated_size == "S");
    assert!(est_small.score <= 40);
    assert!(est_small.risk_factors.is_empty());

    // 2. Medium task
    let est_medium = ComplexityEstimator::estimate(
        "Add health check endpoint",
        "Expose HTTP GET /healthz in backend",
        &["src/routes.rs".to_string(), "src/health.rs".to_string()],
        1,
    );
    assert_eq!(est_medium.estimated_size, "M");
    assert!(est_medium.score > 20 && est_medium.score <= 65);

    // 3. Large / XL task with schema migration, refactor, and security
    let est_large = ComplexityEstimator::estimate(
        "Database Schema Migration and Auth Token Refactor",
        "Refactor auth security module and execute database migrations across user tables",
        &[
            "migrations/0009_auth_tokens.sql".to_string(),
            "crates/coordinator/src/auth/token.rs".to_string(),
            "crates/coordinator/src/db/users.rs".to_string(),
            "crates/coordinator/src/routes/login.rs".to_string(),
        ],
        4,
    );
    assert!(est_large.estimated_size == "L" || est_large.estimated_size == "XL");
    assert!(est_large.score >= 66);
    assert!(est_large.risk_factors.iter().any(|f| f.contains("database")));
    assert!(est_large.risk_factors.iter().any(|f| f.contains("refactor")));
    assert!(est_large.risk_factors.iter().any(|f| f.contains("Security-critical")));
    assert!(est_large.risk_factors.iter().any(|f| f.contains("High dependency gating")));
}

#[tokio::test]
async fn test_repo_aware_plan_service_integration() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Repo-Aware Intelligent Planning Proj".to_string(),
            description: "Build robust AI planner with ground-truth repository context".to_string(),
        },
    )
    .await
    .unwrap();

    let rust_agent_id = Uuid::new_v4();
    let rust_agent = AgentRepository::create_with_id(
        &pool,
        rust_agent_id,
        &NewAgent {
            human_owner: "RustSpecialist".to_string(),
            api_key_hash: "secret_hash".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: format!("agents.{rust_agent_id}.events"),
        },
    )
    .await
    .unwrap();

    let planning_req = PlanningRequest {
        project_id: project.id,
        project_name: project.name.clone(),
        project_description: project.description.clone(),
        available_agents: vec![AvailableAgentContext {
            agent_id: rust_agent.id,
            human_owner: rust_agent.human_owner.clone(),
            adapter_type: "Agy".to_string(),
            capabilities: rust_agent.capabilities_list(),
        }],
        existing_tasks: vec![],
        repo_context: None,
    };

    // Plan response intentionally omitting suggested_agent_id and estimated_size
    // to verify coordinator's automatic capability matching and complexity estimation
    let plan_response = PlanningResponse {
        reasoning: "Decomposed into data layer and service layer".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "PLAN-101".to_string(),
                title: "Rust Data Repository Setup".to_string(),
                description: "Implement PostgreSQL persistence layer in Rust".to_string(),
                suggested_agent_id: None, // Will be auto-inferred by AgentCapabilityMatcher
                affected_resources: vec!["crates/coordinator/src/db/repo.rs".to_string()],
                estimated_size: None,     // Will be auto-inferred by ComplexityEstimator
            },
            ProposedTask {
                short_id: "PLAN-102".to_string(),
                title: "Rust Service API Integration".to_string(),
                description: "Connect service layer with data repository".to_string(),
                suggested_agent_id: None,
                affected_resources: vec![
                    "crates/coordinator/src/db/repo.rs".to_string(), // Overlaps with PLAN-101
                    "crates/coordinator/src/service.rs".to_string(),
                ],
                estimated_size: None,
            },
        ],
        proposed_dependencies: vec![ProposedDependency {
            dependent_short_id: "PLAN-102".to_string(),
            depends_on_short_id: "PLAN-101".to_string(),
            kind: "blocks".to_string(),
            reason: "Service requires repository layer".to_string(),
        }],
    };

    let mock_llm = MockLlmProvider::new().with_response(plan_response);

    // Call generate_repo_aware_plan scanning current repository
    let current_dir = std::env::current_dir().unwrap();
    let proposal_id = PlanningService::generate_repo_aware_plan(
        &pool,
        &mock_llm,
        &current_dir,
        planning_req,
    )
    .await
    .expect("generate_repo_aware_plan must succeed");

    assert_ne!(proposal_id, Uuid::nil());

    // Verify Proposal stored in DB
    let proposal = ProposalRepository::find_by_id(&pool, proposal_id)
        .await
        .unwrap()
        .expect("Proposal record must exist");
    assert_eq!(proposal.project_id, project.id);

    // Verify Tasks stored with auto-matched agent and auto-estimated complexity
    let tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    assert_eq!(tasks.len(), 2);

    let task1 = tasks.iter().find(|t| t.short_id == "PLAN-101").unwrap();
    let task2 = tasks.iter().find(|t| t.short_id == "PLAN-102").unwrap();

    assert_eq!(task1.status, TaskStatus::HumanReview);
    assert_eq!(task2.status, TaskStatus::HumanReview);

    // Tasks remain unassigned in HumanReview until human approval (domain invariant)
    assert!(task1.assigned_agent_id.is_none());
    assert!(task2.assigned_agent_id.is_none());

    // Auto-matched agent is recorded in proposal raw_response via capability matching
    let enriched_plan: PlanningResponse = serde_json::from_str(&proposal.raw_response).unwrap();
    let p_task1 = enriched_plan.proposed_tasks.iter().find(|t| t.short_id == "PLAN-101").unwrap();
    let p_task2 = enriched_plan.proposed_tasks.iter().find(|t| t.short_id == "PLAN-102").unwrap();
    assert_eq!(
        p_task1.suggested_agent_id,
        Some(rust_agent_id),
        "PLAN-101 must be auto-matched to RustSpecialist via capability matching"
    );
    assert_eq!(
        p_task2.suggested_agent_id,
        Some(rust_agent_id),
        "PLAN-102 must be auto-matched to RustSpecialist via capability matching"
    );

    // Auto-estimated complexity check
    assert!(task1.estimated_size.is_some(), "PLAN-101 must have auto-estimated size");
    assert!(task2.estimated_size.is_some(), "PLAN-102 must have auto-estimated size");

    // Overlap warning check (both touch "crates/coordinator/src/db/repo.rs")
    let overlaps = OverlapWarningRepository::list_by_project(&pool, project.id).await.unwrap_or_default();
    assert!(
        overlaps.iter().any(|o| o.resource.contains("repo.rs")),
        "Must record overlap warning for shared resource repo.rs"
    );

    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

#[tokio::test]
async fn test_dynamic_replanning_on_failure_and_state_change() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Dynamic Replanning Proj".to_string(),
            description: "Test replanning after execution failure and unexpected changes".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "ReplanOwner".to_string(),
            api_key_hash: "hash".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
        },
    )
    .await
    .unwrap();

    // 1. Initial Plan with 2 tasks:
    // Task 1: Finished cleanly
    // Task 2: Failed during run
    let initial_proposal = ProposalRepository::create(
        &pool,
        &coordinator::domain::NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "initial prompt".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &coordinator::domain::NewTask {
            project_id: project.id,
            short_id: "INIT-01".to_string(),
            title: "Setup DB Schema".to_string(),
            description: "Run migrations".to_string(),
            affected_resources: vec!["migrations/0001_init.sql".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: initial_proposal.id,
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &coordinator::domain::NewTask {
            project_id: project.id,
            short_id: "INIT-02".to_string(),
            title: "Connect DB Pool".to_string(),
            description: "Initialize sqlx pool".to_string(),
            affected_resources: vec!["crates/coordinator/src/db/pool.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: initial_proposal.id,
        },
    )
    .await
    .unwrap();

    // Mark task1 completed
    TaskRepository::update_status(&pool, task1.id, TaskStatus::Completed).await.unwrap();

    // Mark task2 failed
    TaskRepository::update_status(&pool, task2.id, TaskStatus::Failed).await.unwrap();

    // Record an unexpected resource modification on task2
    let _ = UnexpectedResourceRepository::record(
        &pool,
        task2.id,
        "crates/coordinator/src/unintended_config.json",
        "Agent modified configuration file outside planned scope",
    )
    .await;

    // 2. Gather Replan Context
    let replan_ctx = ReplanEngine::gather_replan_context(&pool, project.id, None)
        .await
        .expect("Must gather replan context");

    assert_eq!(replan_ctx.completed_tasks.len(), 1);
    assert_eq!(replan_ctx.completed_tasks[0].short_id, "INIT-01");
    assert_eq!(replan_ctx.failed_tasks.len(), 1);
    assert_eq!(replan_ctx.failed_tasks[0].short_id, "INIT-02");
    assert!(replan_ctx.unexpected_changes.iter().any(|c| c.contains("unintended_config.json")));

    // 3. Verify Replan Prompt formatting
    let replan_prompt = ReplanPrompt::user_prompt(&replan_ctx);
    assert!(replan_prompt.contains("COMPLETED TASKS (Preserve & build upon):"));
    assert!(replan_prompt.contains("[INIT-01] Setup DB Schema"));
    assert!(replan_prompt.contains("FAILED / BLOCKED TASKS (Need corrective action):"));
    assert!(replan_prompt.contains("[INIT-02] Connect DB Pool"));
    assert!(replan_prompt.contains("UNEXPECTED RESOURCE MODIFICATIONS DETECTED:"));
    assert!(replan_prompt.contains("unintended_config.json"));

    // 4. Execute Dynamic Re-plan with Corrective Action
    let corrective_plan = PlanningResponse {
        reasoning: "Task INIT-01 completed. Re-attempting connection pool with fixed config and addressing unexpected file.".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "REPLAN-01".to_string(),
                title: "Fix DB Pool Configuration".to_string(),
                description: "Fix sqlx connection parameters and revert unintended_config.json".to_string(),
                suggested_agent_id: Some(agent.id),
                affected_resources: vec![
                    "crates/coordinator/src/db/pool.rs".to_string(),
                    "crates/coordinator/src/unintended_config.json".to_string(),
                ],
                estimated_size: Some("M".to_string()),
            },
        ],
        proposed_dependencies: vec![],
    };

    let replan_provider = MockLlmProvider::new().with_response(corrective_plan);

    let replan_proposal_id = ReplanEngine::execute_replan(&pool, &replan_provider, &replan_ctx)
        .await
        .expect("execute_replan must succeed");

    assert_ne!(replan_proposal_id, Uuid::nil());

    let all_tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    let corrective_task = all_tasks.iter().find(|t| t.short_id == "REPLAN-01").unwrap();
    assert_eq!(corrective_task.status, TaskStatus::HumanReview);
    assert!(corrective_task.assigned_agent_id.is_none(), "Must wait for human approval");
    assert_eq!(corrective_task.resources().len(), 2);

    let replan_prop = ProposalRepository::find_by_id(&pool, replan_proposal_id).await.unwrap().unwrap();
    let enriched_replan: PlanningResponse = serde_json::from_str(&replan_prop.raw_response).unwrap();
    let p_corr = enriched_replan.proposed_tasks.iter().find(|t| t.short_id == "REPLAN-01").unwrap();
    assert_eq!(p_corr.suggested_agent_id, Some(agent.id));

    ProjectRepository::delete(&pool, project.id).await.unwrap();
}
