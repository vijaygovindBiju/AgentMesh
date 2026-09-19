use std::path::PathBuf;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::{AgentMessage, CoordinatorMessage};
use coordinator::ai::{
    PlanValidator, PlanningRequest, PlanningResponse, ProposedDependency, ProposedTask,
    RepositoryScanner, ValidationError,
};
use coordinator::coordinator::{AssignmentService, CommandHandler, OverlapDetector};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, GitConflictRepository, OverlapWarningRepository, ProjectRepository,
    ProposalRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, DependencyKind, NewAgent, NewProject, NewProposal, NewTask,
    NewTaskDependency, TaskStatus,
};
use coordinator::git::{AgentWorkspace, RepositoryIdentity};
use coordinator::messaging::{connect, ensure_streams, RegistrationHandler};
use coordinator::observability::{FailureDiagnostics, MetricsCollector, TimelineService};
use coordinator::reliability::StaleTaskSweeper;

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (client, jetstream) = connect(&nats_url, None).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, client, jetstream))
}

fn create_temp_repo(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}_{}", prefix, Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_phase15_1_real_git_repository_and_architecture_discovery() {
    let repo_dir = create_temp_repo("agentmesh_v1_repo");

    // Initialize real git repo
    let identity = RepositoryIdentity::init(&repo_dir, "main")
        .await
        .expect("Initialize repository");

    assert_eq!(identity.base_branch, "main");
    assert!(identity.is_clean().await.expect("is_clean"));

    // Populate real project structure
    tokio::fs::create_dir_all(repo_dir.join("src")).await.unwrap();
    tokio::fs::create_dir_all(repo_dir.join("crates/core")).await.unwrap();
    tokio::fs::write(
        repo_dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        repo_dir.join("crates/core/Cargo.toml"),
        "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        repo_dir.join("crates/core/lib.rs"),
        "pub fn core_engine() -> &'static str { \"v1.0\" }\n",
    )
    .await
    .unwrap();

    // Scan repository architecture
    let context = RepositoryScanner::scan(&repo_dir).await.expect("Repo scan");
    assert!(context.primary_languages.iter().any(|l| l.contains("Rust")));
    assert!(!context.key_modules.is_empty());
    assert!(context.detected_ecosystems.iter().any(|e| e.contains("Rust") || e.contains("Cargo")));

    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[tokio::test]
async fn test_phase15_2_ai_task_decomposition_and_dag_validation() {
    let plan = PlanningResponse {
        reasoning: "Architecture decomposition into DB and API layers".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "T1".to_string(),
                title: "Setup Database Schema".to_string(),
                description: "Run migrations and indexes".to_string(),
                affected_resources: vec!["crates/db".to_string()],
                estimated_size: Some("M".to_string()),
                suggested_agent_id: None,
            },
            ProposedTask {
                short_id: "T2".to_string(),
                title: "Implement API Endpoints".to_string(),
                description: "Build NATS and HTTP routes".to_string(),
                affected_resources: vec!["crates/api".to_string()],
                estimated_size: Some("L".to_string()),
                suggested_agent_id: None,
            },
        ],
        proposed_dependencies: vec![ProposedDependency {
            dependent_short_id: "T2".to_string(),
            depends_on_short_id: "T1".to_string(),
            kind: "blocks".to_string(),
            reason: "API endpoints depend on DB schema".to_string(),
        }],
    };

    let req = PlanningRequest::new(
        Uuid::new_v4(),
        "AgentMesh Demo",
        "E2E Validation",
        vec![],
        vec![],
    );

    let validated = PlanValidator::validate(&req, &plan).expect("Valid DAG plan");
    assert_eq!(validated.response.proposed_tasks.len(), 2);
    assert_eq!(validated.response.proposed_dependencies.len(), 1);

    // Validate cycle rejection
    let cyclic = PlanningResponse {
        reasoning: "Cycle test".to_string(),
        proposed_tasks: plan.proposed_tasks.clone(),
        proposed_dependencies: vec![
            ProposedDependency {
                dependent_short_id: "T2".to_string(),
                depends_on_short_id: "T1".to_string(),
                kind: "blocks".to_string(),
                reason: "".to_string(),
            },
            ProposedDependency {
                dependent_short_id: "T1".to_string(),
                depends_on_short_id: "T2".to_string(),
                kind: "blocks".to_string(),
                reason: "".to_string(),
            },
        ],
    };
    let err = PlanValidator::validate(&req, &cyclic);
    assert!(matches!(err, Err(ValidationError::DependencyCycle(_))));
}

#[tokio::test]
async fn test_phase15_3_human_review_approval_and_overlap_detection() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "v1.0 Validation Proj".to_string(),
            description: "End-to-End Human Review".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // Two tasks targeting the same file crates/db/schema.rs
    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("T1-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task 1 Schema".to_string(),
            description: "Modify schema".to_string(),
            affected_resources: vec!["crates/db/schema.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("T2-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task 2 Indexing".to_string(),
            description: "Add indexes to schema".to_string(),
            affected_resources: vec!["crates/db/schema.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    // Detect overlap and persist
    let overlaps = OverlapDetector::detect_and_persist_for_project(&pool, proj.id).await.unwrap();
    assert!(!overlaps.is_empty(), "Should detect overlap on crates/db/schema.rs");

    // Human review: acknowledge overlap warning and approve tasks
    for w in &overlaps {
        OverlapWarningRepository::acknowledge(&pool, w.id).await.unwrap();
    }
    TaskRepository::update_status(&pool, task1.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::update_status(&pool, task2.id, TaskStatus::Approved).await.unwrap();

    let t1 = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    let t2 = TaskRepository::find_by_id(&pool, task2.id).await.unwrap().unwrap();
    assert_eq!(t1.status, TaskStatus::Approved);
    assert_eq!(t2.status, TaskStatus::Approved);
}

#[tokio::test]
async fn test_phase15_4_multi_agent_registration_and_secure_fleet() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let agent_a_id = Uuid::new_v4();
    let agent_b_id = Uuid::new_v4();

    // Register Agent A (Rust Specialist)
    let msg_a = AgentMessage::Register {
        agent_id: agent_a_id,
        human_owner: "Alice (Machine 1 / Rust Specialist)".to_string(),
        adapter_type: "Agy".to_string(),
        capabilities: vec!["rust".to_string(), "backend".to_string(), "git".to_string()],
        profile: None,
        api_key: "am_ak_alice_secret_token_12345".to_string(),
    };
    let resp_a = RegistrationHandler::process_registration(&pool, msg_a).await.unwrap();
    assert!(matches!(resp_a, CoordinatorMessage::RegisterResponse { status, .. } if status == "ok"));

    // Register Agent B (Infra Specialist on Machine 2)
    let msg_b = AgentMessage::Register {
        agent_id: agent_b_id,
        human_owner: "Bob (Machine 2 / Infra Specialist)".to_string(),
        adapter_type: "Mock".to_string(),
        capabilities: vec!["infra".to_string(), "nats".to_string(), "docker".to_string()],
        profile: None,
        api_key: "am_ak_bob_secret_token_67890".to_string(),
    };
    let resp_b = RegistrationHandler::process_registration(&pool, msg_b).await.unwrap();
    assert!(matches!(resp_b, CoordinatorMessage::RegisterResponse { status, .. } if status == "ok"));

    let agent_a = AgentRepository::find_by_id(&pool, agent_a_id).await.unwrap().unwrap();
    let agent_b = AgentRepository::find_by_id(&pool, agent_b_id).await.unwrap().unwrap();

    assert_eq!(agent_a.human_owner, "Alice (Machine 1 / Rust Specialist)");
    assert_eq!(agent_b.human_owner, "Bob (Machine 2 / Infra Specialist)");
    assert_eq!(agent_a.status, AgentStatus::Idle);
    assert_eq!(agent_b.status, AgentStatus::Idle);
}

#[tokio::test]
async fn test_phase15_5_parallel_task_execution_and_worktree_isolation() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let repo_dir = create_temp_repo("agentmesh_v1_worktree");
    let _identity = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Worktree Mesh".to_string(),
            description: "Parallel worktree execution".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_1 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Worker1".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let agent_2 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Worker2".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("WT1-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Worktree Task 1".to_string(),
            description: "Edit module A".to_string(),
            affected_resources: vec!["src/mod_a.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("WT2-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Worktree Task 2".to_string(),
            description: "Edit module B".to_string(),
            affected_resources: vec!["src/mod_b.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task_1.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::Approved).await.unwrap();

    // Assign tasks
    TaskRepository::assign_agent(&pool, task_1.id, Some(agent_1.id)).await.unwrap();
    TaskRepository::assign_agent(&pool, task_2.id, Some(agent_2.id)).await.unwrap();

    // Initialize agent worktrees with isolated branches
    let wt1 = AgentWorkspace::create(&repo_dir, task_1.id, &task_1.short_id, "task-wt1", "HEAD", None).await.unwrap();
    let wt2 = AgentWorkspace::create(&repo_dir, task_2.id, &task_2.short_id, "task-wt2", "HEAD", None).await.unwrap();

    assert!(wt1.worktree_path.exists());
    assert!(wt2.worktree_path.exists());
    assert_ne!(wt1.worktree_path, wt2.worktree_path);

    // Each agent writes independently to its isolated worktree
    tokio::fs::write(wt1.worktree_path.join("mod_a.rs"), "pub fn a() {}\n").await.unwrap();
    tokio::fs::write(wt2.worktree_path.join("mod_b.rs"), "pub fn b() {}\n").await.unwrap();

    assert!(wt1.worktree_path.join("mod_a.rs").exists());
    assert!(!wt2.worktree_path.join("mod_a.rs").exists());
    assert!(wt2.worktree_path.join("mod_b.rs").exists());
    assert!(!wt1.worktree_path.join("mod_b.rs").exists());

    // Cleanup
    let _ = wt1.cleanup().await;
    let _ = wt2.cleanup().await;
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[tokio::test]
async fn test_phase15_6_dependency_enforcement_order() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Dependency Enforcement".to_string(),
            description: "Task B depends on Task A".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "DepWorker".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let task_a = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("DEP-A-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task A Foundation".to_string(),
            description: "Must finish first".to_string(),
            affected_resources: vec!["a.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task_b = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("DEP-B-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task B Consumer".to_string(),
            description: "Must wait for Task A".to_string(),
            affected_resources: vec!["b.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    // Record blocking dependency: Task B depends on Task A
    TaskRepository::add_dependency(
        &pool,
        &NewTaskDependency {
            dependent_id: task_b.id,
            depends_on_id: task_a.id,
            kind: DependencyKind::Blocks,
        },
    )
    .await
    .unwrap();

    // Move to human review and approve both
    TaskRepository::update_status(&pool, task_a.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task_b.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task_a.id, Some(agent.id)).await.unwrap();
    TaskRepository::assign_agent(&pool, task_b.id, Some(agent.id)).await.unwrap();

    CommandHandler::execute_approve_task(&pool, task_a.id, "Supervisor").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task_b.id, "Supervisor").await.unwrap();

    // Assignment run 1: Task A is unblocked, Task B is blocked -> only Task A assigned
    let assigned_1 = AssignmentService::assign_ready_tasks(&pool, Some(proj.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned_1.len(), 1);
    assert_eq!(assigned_1[0].task_id, task_a.id);

    let t_b = TaskRepository::find_by_id(&pool, task_b.id).await.unwrap().unwrap();
    assert_eq!(t_b.status, TaskStatus::Approved, "Task B must remain Approved while Task A is not completed");

    // Complete Task A and set agent back to Idle
    TaskRepository::update_status(&pool, task_a.id, TaskStatus::Completed).await.unwrap();
    AgentRepository::set_current_task(&pool, agent.id, None, AgentStatus::Idle).await.unwrap();

    // Assignment run 2: Now Task B's blocker is satisfied -> Task B is assigned
    let assigned_2 = AssignmentService::assign_ready_tasks(&pool, Some(proj.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned_2.len(), 1);
    assert_eq!(assigned_2[0].task_id, task_b.id);
}

#[tokio::test]
async fn test_phase15_7_agent_crash_recovery_and_reassignment() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Crash Recovery Test".to_string(),
            description: "Test crash detection".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_crashed = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "CrashedWorker".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let agent_backup = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "BackupWorker".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("RECOV-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Resilient Task".to_string(),
            description: "Reclaimed upon agent failure".to_string(),
            affected_resources: vec!["res.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_crashed.id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Executing).await.unwrap();

    // Simulate silent crash: last_seen is 60 seconds ago
    let old_time = Utc::now() - Duration::seconds(60);
    sqlx::query!("UPDATE agents SET last_seen = $1, status = 'busy' WHERE id = $2", old_time, agent_crashed.id)
        .execute(&pool)
        .await
        .unwrap();

    // Sweeper runs
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(15)).await.unwrap();
    assert!(sweep.tasks_reclaimed.contains(&task.id));

    // Task is safely restored to Approved
    let reclaimed_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(reclaimed_task.status, TaskStatus::Approved);
    assert_eq!(reclaimed_task.assigned_agent_id, None);

    // Reassigned to backup agent and completed
    TaskRepository::assign_agent(&pool, task.id, Some(agent_backup.id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Executing).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Completed).await.unwrap();

    let final_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(final_task.status, TaskStatus::Completed);
    assert_eq!(final_task.assigned_agent_id, Some(agent_backup.id));
}

#[tokio::test]
async fn test_phase15_8_cross_agent_git_conflict_and_failure_diagnostics() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Conflict Diagnostics".to_string(),
            description: "Test git conflict diagnostics".to_string(),
        },
    )
    .await
    .unwrap();

    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "mock".to_string(),
            ai_model: "v1".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("CONF-1-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task 1 Conflicting".to_string(),
            description: "Edit lines 1-10".to_string(),
            affected_resources: vec!["file.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("CONF-2-{}", &Uuid::new_v4().to_string()[..6]),
            title: "Task 2 Conflicting".to_string(),
            description: "Edit lines 5-15".to_string(),
            affected_resources: vec!["file.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    // Record conflict
    GitConflictRepository::record_conflict(
        &pool,
        proj.id,
        task1.id,
        task2.id,
        "file.rs",
        "Overlapping diff on file.rs",
    )
    .await
    .unwrap();

    // Inspect diagnostics
    let diag = FailureDiagnostics::diagnose_task(&pool, task1.id).await.unwrap();
    assert!(diag.has_git_conflicts);
    assert!(diag.remediation_advice.iter().any(|r| r.title == "Resolve Cross-Agent Git Merge Conflicts"));
}

#[tokio::test]
async fn test_phase15_9_v1_system_metrics_and_execution_integrity() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB not reachable");
        return;
    };

    // System-wide metrics snapshot
    let metrics = MetricsCollector::collect(&pool).await.expect("Collect metrics");

    assert!(metrics.total_tasks >= metrics.tasks_completed);
    assert!(metrics.total_agents >= metrics.agents_idle + metrics.agents_busy + metrics.agents_offline);
    assert!(metrics.delivery_success_rate_percent >= 0.0 && metrics.delivery_success_rate_percent <= 100.0);
}

#[tokio::test]
async fn test_phase15_10_complete_project_lifecycle_execution() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    // 1. Project Creation
    let proj = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "AgentMesh v1.0 Full Lifecycle".to_string(),
            description: "End-to-End full workflow validation".to_string(),
        },
    )
    .await
    .unwrap();

    // 2. Proposal Decomposition
    let prop = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: proj.id,
            ai_provider: "anthropic".to_string(),
            ai_model: "claude-3-5-sonnet".to_string(),
            raw_prompt: "Decompose AgentMesh project".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // 3. Create discrete tasks
    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("V1-T1-{}", &Uuid::new_v4().to_string()[..4]),
            title: "Task 1 Data Layer".to_string(),
            description: "Implement data structures".to_string(),
            affected_resources: vec!["crates/data/lib.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: proj.id,
            short_id: format!("V1-T2-{}", &Uuid::new_v4().to_string()[..4]),
            title: "Task 2 Service Layer".to_string(),
            description: "Implement business services".to_string(),
            affected_resources: vec!["crates/service/lib.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: prop.id,
        },
    )
    .await
    .unwrap();

    // 4. Register two workers
    let agent_1 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Worker 1".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let agent_2 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Worker 2".to_string(),
            api_key_hash: format!("hash_{}", Uuid::new_v4()),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: format!("agents.{}.events", Uuid::new_v4()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // 5. Human approval
    TaskRepository::update_status(&pool, task_1.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task_1.id, Some(agent_1.id)).await.unwrap();
    TaskRepository::assign_agent(&pool, task_2.id, Some(agent_2.id)).await.unwrap();

    CommandHandler::execute_approve_task(&pool, task_1.id, "Lead").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task_2.id, "Lead").await.unwrap();

    // 6. Assignment of both tasks in parallel
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(proj.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned.len(), 2, "Both approved independent tasks should be assigned concurrently");

    // 7. Complete tasks and verify project timeline
    TaskRepository::update_status(&pool, task_1.id, TaskStatus::Completed).await.unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::Completed).await.unwrap();

    let task_tl = TimelineService::build_task_timeline(&pool, task_1.id).await.unwrap();
    assert_eq!(task_tl.status, TaskStatus::Completed);
    assert!(!task_tl.events.is_empty());

    let agent_tl = TimelineService::build_agent_timeline(&pool, agent_1.id).await.unwrap();
    assert_eq!(agent_tl.agent_id, agent_1.id);
}
