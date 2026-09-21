//! Phase 18: Real-World Project Validation
//!
//! Validates AgentMesh v1.0 against the real repository `/media/pirate/Shared/Rust/minigrep`
//! using real `agy` execution across the complete end-to-end lifecycle.

use chrono::{Duration, Utc};
use futures::StreamExt;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::sleep;
use uuid::Uuid;

use agent_protocol::{AgentMessage, CoordinatorMessage, TaskSpec};
use coordinator::ai::{
    AvailableAgentContext, MockLlmProvider, PlanningRequest, PlanningResponse, PlanningService,
    ProposedDependency, ProposedTask, ReplanEngine, RepositoryScanner,
};
use coordinator::coordinator::{AssignmentService, CommandHandler, CoordinatorCommand, CoordinatorCore};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentEventRepository, AgentRepository, ProjectRepository, ProposalRepository,
    TaskDeliveryRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, DependencyKind, NewAgent, NewProject, NewProposal, NewTask,
    NewTaskDelivery, NewTaskDependency, TaskStatus,
};
use coordinator::git::{AgentWorkspace, BranchStrategy, RepositoryIdentity};
use coordinator::messaging::{connect, ensure_streams, EventSubscriber};
use coordinator::observability::MetricsCollector;
use coordinator::reliability::{CoordinatorRecoveryService, StaleTaskSweeper};

const MINIGREP_PATH: &str = "/media/pirate/Shared/Rust/minigrep";

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token =
        std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    let pool = match create_pool(&db_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("DB pool creation failed: {e}");
            return None;
        }
    };
    if let Err(e) = run_migrations(&pool).await {
        eprintln!("Migrations failed: {e}");
        return None;
    }

    let (client, jetstream) = match connect(&nats_url, Some(&nats_token)).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("NATS connect failed: {e}");
            return None;
        }
    };
    if let Err(e) = ensure_streams(&jetstream).await {
        eprintln!("ensure_streams failed: {e}");
        return None;
    }

    Some((pool, client, jetstream))
}

// ============================================================================
// Test 1: Real Repository Discovery (Section 6)
// ============================================================================
#[tokio::test]
async fn test_phase18_1_real_repository_discovery() {
    let repo_path = PathBuf::from(MINIGREP_PATH);
    assert!(
        repo_path.exists(),
        "Target repository must exist at {MINIGREP_PATH}"
    );

    // 1. Discover repository Git identity
    let identity = RepositoryIdentity::discover(&repo_path)
        .await
        .expect("Git repository identity discovery failed");

    println!("=== Test 1: Real Repository Discovery ===");
    println!("Repository Root: {}", identity.repo_root.display());
    println!("Base Branch:     {}", identity.base_branch);
    println!("HEAD Commit:     {}", identity.head_commit_sha);

    assert_eq!(identity.base_branch, "master");
    assert!(!identity.head_commit_sha.is_empty());
    let is_clean = identity.is_clean().await.expect("Check if clean");
    println!("Is Clean:        {is_clean}");
    assert!(is_clean, "Repository must have a clean working tree");

    // 2. Scan repository architecture
    let context = RepositoryScanner::scan(&repo_path)
        .await
        .expect("RepositoryScanner failed to scan minigrep");

    println!("Detected Ecosystems: {:?}", context.detected_ecosystems);
    println!("Primary Languages:   {:?}", context.primary_languages);
    println!("Key Modules:         {:?}", context.key_modules);
    println!("File Tree Sample:    {:?}", context.file_tree_sample);

    assert!(
        context
            .detected_ecosystems
            .iter()
            .any(|e| e.contains("Rust") || e.contains("Cargo")),
        "Must detect Rust / Cargo ecosystem"
    );
    assert!(
        context.primary_languages.iter().any(|l| l == "Rust"),
        "Must detect Rust as primary language"
    );
    assert!(
        context.key_modules.iter().any(|m| m == "minigrep" || m == "src"),
        "Must identify minigrep or src module"
    );
    assert!(
        context.file_tree_sample.iter().any(|f| f == "src/lib.rs"),
        "Sample tree must contain src/lib.rs"
    );
    assert!(
        context.file_tree_sample.iter().any(|f| f == "src/main.rs"),
        "Sample tree must contain src/main.rs"
    );
    assert!(
        context.file_tree_sample.iter().any(|f| f == "Cargo.toml"),
        "Sample tree must contain Cargo.toml"
    );

    println!("✔ Test 1: Real Repository Discovery PASSED\n");
}

// ============================================================================
// Test 2: AI Planning & Human Approval Gate (Section 7)
// ============================================================================
#[tokio::test]
async fn test_phase18_2_ai_planning_and_human_approval_gate() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);

    // 1. Create project with real repo identity
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Minigrep Real Validation".to_string(),
            description: "Add line numbering and case-insensitive search to minigrep".to_string(),
        },
    )
    .await
    .unwrap();

    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();
    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    // 2. Register real AGY agent
    let agy_agent_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agy_agent_id,
        &NewAgent {
            human_owner: "AgyOperator".to_string(),
            api_key_hash: "key_agy_real".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec![
                "rust".to_string(),
                "general-coding".to_string(),
                "backend".to_string(),
            ],
            nats_subject: format!("agents.{agy_agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agy_agent_id, AgentStatus::Idle)
        .await
        .unwrap();

    let agents = AgentRepository::list(&pool).await.unwrap();
    let agent_contexts: Vec<AvailableAgentContext> = agents
        .iter()
        .map(|a| AvailableAgentContext {
            agent_id: a.id,
            human_owner: a.human_owner.clone(),
            adapter_type: format!("{:?}", a.adapter_type),
            capabilities: a.capabilities_list(),
        })
        .collect();

    // 3. Scan repo context
    let repo_context = RepositoryScanner::scan(&repo_path).await.ok();

    // 4. Generate plan using AgyProvider if agy CLI is available, otherwise mock with real plan
    let plan_provider: Arc<dyn coordinator::ai::LlmProvider> =
        match coordinator::ai::create_provider_by_name("agy") {
            Ok(p) => p,
            Err(_) => {
                let mock_plan = PlanningResponse {
                    reasoning: "Decomposed minigrep enhancement into lib.rs search and main.rs CLI".to_string(),
                    proposed_tasks: vec![
                        ProposedTask {
                            short_id: "MINI-001".to_string(),
                            title: "Implement helper and unit tests in lib.rs".to_string(),
                            description: "Add version info helper function and unit tests in src/lib.rs".to_string(),
                            suggested_agent_id: Some(agy_agent_id),
                            affected_resources: vec!["src/lib.rs".to_string()],
                            estimated_size: Some("S".to_string()),
                        },
                        ProposedTask {
                            short_id: "MINI-002".to_string(),
                            title: "Update CLI argument parsing in main.rs".to_string(),
                            description: "Add flag parsing and wire to lib in src/main.rs".to_string(),
                            suggested_agent_id: Some(agy_agent_id),
                            affected_resources: vec!["src/main.rs".to_string()],
                            estimated_size: Some("S".to_string()),
                        },
                    ],
                    proposed_dependencies: vec![
                        ProposedDependency {
                            dependent_short_id: "MINI-002".to_string(),
                            depends_on_short_id: "MINI-001".to_string(),
                            kind: "blocks".to_string(),
                            reason: "CLI relies on lib functions".to_string(),
                        }
                    ],
                };
                Arc::new(MockLlmProvider::new().with_response(mock_plan))
            }
        };

    let proposal_id = PlanningService::generate_and_persist_plan(
        &pool,
        plan_provider.as_ref(),
        &PlanningRequest {
            project_id: project.id,
            project_name: project.name.clone(),
            project_description: project.description.clone(),
            available_agents: agent_contexts,
            existing_tasks: vec![],
            repo_context,
        },
    )
    .await
    .expect("Plan generation and persistence");

    println!("Proposal ID: {proposal_id}");

    // 5. Verify human approval gate: tasks MUST be in HumanReview, NOT Approved or Assigned!
    let tasks = TaskRepository::list_by_project(&pool, project.id)
        .await
        .unwrap();
    assert!(!tasks.is_empty(), "Must have proposed tasks");

    for task in &tasks {
        assert_eq!(
            task.status,
            TaskStatus::HumanReview,
            "Task {} must start in HumanReview status",
            task.short_id
        );
        assert!(
            task.assigned_agent_id.is_none(),
            "Task {} must not be assigned before approval",
            task.short_id
        );
    }
    println!("✔ Human Review Gate verified: {} task(s) waiting for operator", tasks.len());

    // 6. Operator explicitly approves only the first task
    let task_to_approve = &tasks[0];
    let approved_task = CommandHandler::execute_approve_task(
        &pool,
        task_to_approve.id,
        "Lead Engineer Human Gate",
    )
    .await
    .expect("Approve task");

    assert_eq!(approved_task.status, TaskStatus::Approved);
    println!("Approved Task: {} [{}]", approved_task.short_id, approved_task.id);

    // Verify unapproved tasks remain in HumanReview
    if tasks.len() > 1 {
        let unapproved = TaskRepository::find_by_id(&pool, tasks[1].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            unapproved.status,
            TaskStatus::HumanReview,
            "Unapproved task must remain in HumanReview"
        );
    }

    println!("✔ Test 2: AI Planning + Human Approval Gate PASSED\n");
}

// ============================================================================
// Test 3 & 4 & 5: Worktree, Real AGY Coding, and Git Finalization (Sections 8-10)
// ============================================================================
#[tokio::test]
async fn test_phase18_3_worktree_real_agy_execution_and_finalization() {
    let Some((pool, client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);
    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Minigrep Real AGY Coding".to_string(),
            description: "Add a version helper function to minigrep src/lib.rs".to_string(),
        },
    )
    .await
    .unwrap();

    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "agy".to_string(),
            ai_model: "gemini-3.8-flash-low".to_string(),
            raw_prompt: "real coding".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "REAL-001".to_string(),
            title: "Add minigrep version info helper".to_string(),
            description: "In src/lib.rs, add a public function `pub fn minigrep_version() -> &'static str { \"0.1.0\" }` and a unit test `test_minigrep_version` verifying it returns \"0.1.0\".".to_string(),
            affected_resources: vec!["src/lib.rs".to_string()],
            estimated_size: Some("XS".to_string()),
        },
    )
    .await
    .unwrap();

    // Register real AGY agent
    let agent_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "RealAgyTester".to_string(),
            api_key_hash: "key_real_agy_1".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Idle)
        .await
        .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id))
        .await
        .unwrap();
    // Approve the task through the human gate
    CommandHandler::execute_approve_task(&pool, task.id, "Test Operator")
        .await
        .unwrap();

    // Run assignment: should create isolated worktree and assign to agent
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assignments.len(), 1, "Task must be assigned");
    let assignment = &assignments[0];
    assert_eq!(assignment.task_id, task.id);
    assert_eq!(assignment.agent_id, agent_id);

    // Verify worktree created
    let expected_worktree = AgentWorkspace::expected_worktree_path(&identity.repo_root, "REAL-001");
    println!("Worktree Path: {}", expected_worktree.display());
    assert!(
        expected_worktree.exists(),
        "Isolated worktree must exist at {}",
        expected_worktree.display()
    );

    // Verify task branch
    let git_branch_output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&expected_worktree)
        .output()
        .await
        .unwrap();
    let current_branch = String::from_utf8_lossy(&git_branch_output.stdout)
        .trim()
        .to_string();
    println!("Worktree Branch: {current_branch}");
    assert_eq!(current_branch, "agentmesh/real-001");

    // Verify original repository working tree remains clean
    let original_clean = identity.is_clean().await.unwrap();
    assert!(
        original_clean,
        "Original repo working tree must remain untouched"
    );

    // Setup JetStream event subscriber for coordinator
    let sub_pool = pool.clone();
    let events_consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();
    let subscriber_handle = tokio::spawn(async move {
        use futures::StreamExt;
        let mut messages = events_consumer.messages().await.unwrap();
        while let Some(Ok(msg)) = messages.next().await {
            let _ = msg.ack().await;
            if let Ok(agent_msg) = serde_json::from_slice::<AgentMessage>(&msg.payload) {
                let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg).await;
            }
        }
    });

    // Execute task using real agent-agy runner
    let model = std::env::var("AGY_MODEL").unwrap_or_else(|_| "gemini-3.8-flash-low".to_string());
    let agy_agent = agent_agy::AgyAgent::new("RealAgyTester", "key_real_agy_1")
        .with_id(agent_id)
        .with_model(model)
        .with_effort("low")
        .with_timeout(std::time::Duration::from_secs(180));

    let task_spec = TaskSpec::new(
        task.id,
        "REAL-001",
        "Add minigrep version info helper",
        &task.description,
        vec!["src/lib.rs".to_string()],
        vec![],
        &assignment.idempotency_key,
    )
    .with_git_context(
        expected_worktree.to_str().unwrap(),
        "agentmesh/real-001",
        "master",
    );

    let event_subject = format!("agents.{agent_id}.events");
    println!("Dispatching real coding task to agy CLI inside worktree...");
    let exec_res =
        agent_agy::AgyAgentRunner::execute_task(&client, &agy_agent, &task_spec, &event_subject)
            .await;

    println!("agy execution result: {:?}", exec_res);
    assert!(exec_res.is_ok(), "Real agy task execution must succeed");

    // Allow events to process
    sleep(std::time::Duration::from_millis(500)).await;
    subscriber_handle.abort();

    // Verify event records in agent_events
    let recorded_events = AgentEventRepository::list_by_task(&pool, task.id)
        .await
        .unwrap();
    println!(
        "Recorded events count: {}",
        recorded_events.len()
    );
    for ev in &recorded_events {
        println!("  - Event: {:?} | Message: {:?}", ev.event_type, ev.message);
    }

    assert!(
        recorded_events
            .iter()
            .any(|e| e.event_type == coordinator::domain::AgentEventType::TaskStarted),
        "TaskStarted event must be recorded"
    );

    // Verify task reached Completed status
    let completed_task = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .expect("Task must exist");
    assert_eq!(
        completed_task.status,
        TaskStatus::Completed,
        "Task must be marked Completed in PostgreSQL"
    );

    // Verify automatic Git finalization performed by EventSubscriber
    let git_ctx = TaskRepository::find_git_context(&pool, task.id)
        .await
        .unwrap()
        .expect("Git context must exist");

    let completion_commit_sha = git_ctx
        .completion_commit_sha
        .expect("Completion commit SHA must be recorded");

    println!("=== Test 5: Git Finalization Results ===");
    println!("Completion Commit SHA: {completion_commit_sha}");
    println!("Actual Modified:       {:?}", git_ctx.actual_modified_resources);
    println!("Worktree cleaned up:    {}", !expected_worktree.exists());

    assert!(
        !completion_commit_sha.is_empty(),
        "Must generate completion commit SHA"
    );
    assert!(
        git_ctx.actual_modified_resources.iter().any(|r| r.contains("src/lib.rs")),
        "Modified resources must include src/lib.rs"
    );
    assert!(
        !expected_worktree.exists(),
        "Worktree must be cleaned up upon finalization"
    );

    // Test mergeability of task branch against master
    let conflict_report = coordinator::git::ConflictDetector::detect_cross_agent_conflicts(
        &identity.repo_root,
        "agentmesh/real-001",
        "master",
    )
    .await
    .expect("Conflict detection");
    assert!(
        conflict_report.is_none(),
        "Changes must be cleanly mergeable to master"
    );

    // Verify original repository remains clean
    let clean_after = identity.is_clean().await.unwrap();
    assert!(clean_after, "Original repository must remain clean after finalization");

    // Clean up task branch from repository
    let branch_name = BranchStrategy::task_branch_name("REAL-001");
    let _ = tokio::process::Command::new("git")
        .args(["branch", "-D", &branch_name])
        .current_dir(&identity.repo_root)
        .output()
        .await;

    println!("✔ Tests 3, 4, 5: Worktree Isolation, Real AGY Execution & Git Finalization PASSED\n");
}

// ============================================================================
// Test 6: Multiple Tasks / Parallel Execution (Section 11)
// ============================================================================
#[tokio::test]
async fn test_phase18_4_parallel_tasks_and_worktree_isolation() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);
    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Minigrep Parallel Tasks".to_string(),
            description: "Test parallel independent worktrees".to_string(),
        },
    )
    .await
    .unwrap();

    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "parallel".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task_a = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "PARA-001".to_string(),
            title: "Task A - Independent module 1".to_string(),
            description: "Work on module 1".to_string(),
            affected_resources: vec!["src/mod1.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let task_b = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "PARA-002".to_string(),
            title: "Task B - Independent module 2".to_string(),
            description: "Work on module 2".to_string(),
            affected_resources: vec!["src/mod2.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    // Register two agents
    let ag1 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        ag1,
        &NewAgent {
            human_owner: "Agent 1".to_string(),
            api_key_hash: "k1".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{ag1}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, ag1, AgentStatus::Idle)
        .await
        .unwrap();

    let ag2 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        ag2,
        &NewAgent {
            human_owner: "Agent 2".to_string(),
            api_key_hash: "k2".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{ag2}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, ag2, AgentStatus::Idle)
        .await
        .unwrap();

    TaskRepository::update_status(&pool, task_a.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_a.id, Some(ag1))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_b.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_b.id, Some(ag2))
        .await
        .unwrap();

    // Human approves both tasks
    CommandHandler::execute_approve_task(&pool, task_a.id, "Operator")
        .await
        .unwrap();
    CommandHandler::execute_approve_task(&pool, task_b.id, "Operator")
        .await
        .unwrap();

    // Assignment worker assigns both simultaneously
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assignments.len(), 2, "Both independent tasks must be assigned");

    let wt_a = AgentWorkspace::expected_worktree_path(&identity.repo_root, "PARA-001");
    let wt_b = AgentWorkspace::expected_worktree_path(&identity.repo_root, "PARA-002");

    assert!(wt_a.exists(), "Worktree A must exist");
    assert!(wt_b.exists(), "Worktree B must exist");
    assert_ne!(wt_a, wt_b, "Worktree paths must be completely separated");

    // Clean up worktrees and branches
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt_a.to_string_lossy()])
        .current_dir(&identity.repo_root)
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt_b.to_string_lossy()])
        .current_dir(&identity.repo_root)
        .output()
        .await;
    let b_a = BranchStrategy::task_branch_name("PARA-001");
    let b_b = BranchStrategy::task_branch_name("PARA-002");
    let _ = tokio::process::Command::new("git")
        .args(["branch", "-D", &b_a, &b_b])
        .current_dir(&identity.repo_root)
        .output()
        .await;

    println!("✔ Test 6: Multiple Tasks / Parallel Execution PASSED\n");
}

// ============================================================================
// Test 7: Dependencies & Auto-assignment (Section 12)
// ============================================================================
#[tokio::test]
async fn test_phase18_5_dependency_chain_and_auto_assignment() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);
    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Dependency Validation".to_string(),
            description: "Test task dependency blocking".to_string(),
        },
    )
    .await
    .unwrap();

    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "deps".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "DEP-1".to_string(),
            title: "Task 1 - Prerequisite".to_string(),
            description: "Base task".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "DEP-2".to_string(),
            title: "Task 2 - Dependent".to_string(),
            description: "Blocks on Task 1".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    // Establish blocking dependency: task_2 blocks on task_1
    TaskRepository::add_dependency(
        &pool,
        &NewTaskDependency {
            dependent_id: task_2.id,
            depends_on_id: task_1.id,
            kind: DependencyKind::Blocks,
        },
    )
    .await
    .unwrap();

    let agent_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "DepWorker".to_string(),
            api_key_hash: "k_dep".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Idle)
        .await
        .unwrap();

    TaskRepository::update_status(&pool, task_1.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_1.id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_2.id, Some(agent_id))
        .await
        .unwrap();

    // Human approves both tasks
    CommandHandler::execute_approve_task(&pool, task_1.id, "Operator")
        .await
        .unwrap();
    CommandHandler::execute_approve_task(&pool, task_2.id, "Operator")
        .await
        .unwrap();

    // First assignment cycle: only Task 1 should be assigned! Task 2 must wait!
    let assign1 =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assign1.len(), 1, "Only Task 1 should be assigned");
    assert_eq!(assign1[0].task_id, task_1.id);

    let t2_state = TaskRepository::find_by_id(&pool, task_2.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        t2_state.status,
        TaskStatus::Approved,
        "Task 2 must remain in Approved (waiting on dependency)"
    );

    // Complete Task 1
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id,
            task_id: task_1.id,
            summary: "Task 1 done".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    let t1_completed = TaskRepository::find_by_id(&pool, task_1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t1_completed.status, TaskStatus::Completed);

    // Free the agent
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Idle)
        .await
        .unwrap();

    // Second assignment cycle: Task 2 is now automatically unblocked and ready!
    let assign2 =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assign2.len(), 1, "Task 2 must be automatically assigned after prerequisite completes");
    assert_eq!(assign2[0].task_id, task_2.id);

    // Clean up worktrees
    let wt1 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "DEP-1");
    let wt2 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "DEP-2");
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt1.to_string_lossy()])
        .current_dir(&identity.repo_root)
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt2.to_string_lossy()])
        .current_dir(&identity.repo_root)
        .output()
        .await;
    let b_1 = BranchStrategy::task_branch_name("DEP-1");
    let b_2 = BranchStrategy::task_branch_name("DEP-2");
    let _ = tokio::process::Command::new("git")
        .args(["branch", "-D", &b_1, &b_2])
        .current_dir(&identity.repo_root)
        .output()
        .await;

    println!("✔ Test 7: Dependencies & Auto-assignment PASSED\n");
}

// ============================================================================
// Test 8 & 9: Agent Failure, Stale Sweep & Reassignment (Sections 13-14)
// ============================================================================
#[tokio::test]
async fn test_phase18_6_agent_failure_stale_sweep_and_reassignment() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);
    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Agent Failure Validation".to_string(),
            description: "Test crash recovery and reassignment".to_string(),
        },
    )
    .await
    .unwrap();

    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "fail".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "FAIL-001".to_string(),
            title: "Task with simulated agent failure".to_string(),
            description: "Agent will crash mid-task".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let crashed_agent = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        crashed_agent,
        &NewAgent {
            human_owner: "DoomedAgent".to_string(),
            api_key_hash: "k_doom".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{crashed_agent}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, crashed_agent, AgentStatus::Idle)
        .await
        .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(crashed_agent))
        .await
        .unwrap();

    // Human approves task
    CommandHandler::execute_approve_task(&pool, task.id, "Operator")
        .await
        .unwrap();

    // Assignment cycle: delivery 1 created
    let assign1 =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();
    assert_eq!(assign1.len(), 1);
    assert_eq!(assign1[0].idempotency_key, format!("{}:1", task.id));

    // Simulate agent crash: heartbeat stops, agent marked offline
    AgentRepository::update_status(&pool, crashed_agent, AgentStatus::Offline)
        .await
        .unwrap();

    // StaleTaskSweeper detects stale task assigned to offline agent
    let sweep = StaleTaskSweeper::sweep(&pool, Duration::seconds(0))
        .await
        .expect("Stale sweep");

    assert!(
        sweep.tasks_reclaimed.contains(&task.id),
        "Task must be reclaimed from offline agent"
    );

    // Verify task state is reverted to Approved for reassignment
    let reclaimed_task = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reclaimed_task.status,
        TaskStatus::Approved,
        "Reclaimed task must be in Approved state"
    );

    // Register a second agent for reassignment
    let new_agent = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        new_agent,
        &NewAgent {
            human_owner: "ReplacementAgent".to_string(),
            api_key_hash: "k_replace".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{new_agent}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, new_agent, AgentStatus::Idle)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(new_agent))
        .await
        .unwrap();

    // Reassignment cycle
    let assign2 =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assign2.len(), 1, "Reclaimed task must be assigned to new agent");
    assert_eq!(assign2[0].agent_id, new_agent);
    assert_eq!(
        assign2[0].idempotency_key,
        format!("{}:2", task.id),
        "Idempotency key must follow pattern <task_id>:2 for attempt 2"
    );

    // Clean up
    let wt = AgentWorkspace::expected_worktree_path(&identity.repo_root, "FAIL-001");
    let _ = tokio::process::Command::new("git")
        .args(["worktree", "remove", "--force", &wt.to_string_lossy()])
        .current_dir(&identity.repo_root)
        .output()
        .await;
    let b_fail = BranchStrategy::task_branch_name("FAIL-001");
    let _ = tokio::process::Command::new("git")
        .args(["branch", "-D", &b_fail])
        .current_dir(&identity.repo_root)
        .output()
        .await;

    println!("✔ Tests 8 & 9: Agent Failure, Stale Sweep & Reassignment PASSED\n");
}

// ============================================================================
// Test 10: Task Cancellation & Subprocess Check (Section 15)
// ============================================================================
#[tokio::test]
async fn test_phase18_7_task_cancellation_and_subprocess_check() {
    let Some((pool, client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Cancellation Test".to_string(),
            description: "Test task cancellation".to_string(),
        },
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "cancel".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "CANCEL-001".to_string(),
            title: "Task to cancel".to_string(),
            description: "Operator will cancel".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let agent_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "CancelTarget".to_string(),
            api_key_hash: "k_cancel".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Busy)
        .await
        .unwrap();

    // Subscribe to task assignments / cancellation subject
    let cancel_subject = format!("coordinator.tasks.assign.{agent_id}");
    let mut cancel_sub = client.subscribe(cancel_subject.clone()).await.unwrap();

    // Mark task executing
    TaskRepository::update_status(&pool, task.id, TaskStatus::Executing)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id))
        .await
        .unwrap();

    // Create delivery record
    TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id: task.id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: cancel_subject,
            idempotency_key: format!("{}:1", task.id),
            expires_at: Utc::now() + Duration::seconds(300),
        },
    )
    .await
    .unwrap();

    // Initialize CoordinatorCore and execute CancelTask command
    let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));
    coordinator.set_active_project(project.id);

    let cancel_cmd = CoordinatorCommand::CancelTask {
        task_id: task.id,
        reason: "Cancelled by Operator test".to_string(),
    };

    println!("Executing CoordinatorCommand::CancelTask...");
    coordinator
        .handle_command(cancel_cmd)
        .await
        .expect("Handle CancelTask");

    // 1. Verify database state
    let cancelled_task = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        cancelled_task.status,
        TaskStatus::Cancelled,
        "Database task status must be Cancelled"
    );

    // 2. Verify coordinator published TaskCancelled message
    let msg_received = tokio::time::timeout(std::time::Duration::from_secs(2), cancel_sub.next())
        .await
        .ok()
        .flatten();

    assert!(msg_received.is_some(), "NATS must receive TaskCancelled message");
    let payload = msg_received.unwrap().payload;
    let coord_msg: CoordinatorMessage = serde_json::from_slice(&payload).unwrap();

    match coord_msg {
        CoordinatorMessage::TaskCancelled {
            task_id, reason, ..
        } => {
            assert_eq!(task_id, task.id);
            println!("Received CoordinatorMessage::TaskCancelled: task_id={task_id}, reason={reason}");
        }
        other => panic!("Expected TaskCancelled, got {other:?}"),
    }

    println!("✔ Test 10: Task Cancellation PASSED");
    println!("NOTE ON AGENT-AGY INTEGRATION GAP: agent-agy acknowledges and logs cancellation with warning, but currently lacks an active subprocess abort handle. Documented in final report.\n");
}

// ============================================================================
// Test 11: Real Replanning Behind Human Gate (Section 16)
// ============================================================================
#[tokio::test]
async fn test_phase18_8_dynamic_replanning_behind_human_gate() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Replanning Test Proj".to_string(),
            description: "Test corrective replan".to_string(),
        },
    )
    .await
    .unwrap();

    let initial_proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "init".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // Task 1: Completed
    let t1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: initial_proposal.id,
            short_id: "REPLAN-001".to_string(),
            title: "Completed base task".to_string(),
            description: "Done".to_string(),
            affected_resources: vec!["src/lib.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, t1.id, TaskStatus::Completed)
        .await
        .unwrap();

    // Task 2: Failed
    let t2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: initial_proposal.id,
            short_id: "REPLAN-002".to_string(),
            title: "Failed feature task".to_string(),
            description: "Encountered compiler error".to_string(),
            affected_resources: vec!["src/main.rs".to_string()],
            estimated_size: Some("M".to_string()),
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, t2.id, TaskStatus::Failed)
        .await
        .unwrap();

    // Gather replan context
    let repo_context = RepositoryScanner::scan(&repo_path).await.ok();
    let replan_req =
        ReplanEngine::gather_replan_context(&pool, project.id, repo_context).await.unwrap();

    assert_eq!(replan_req.completed_tasks.len(), 1);
    assert_eq!(replan_req.failed_tasks.len(), 1);

    // Provide corrective plan via Mock provider
    let corrective_plan = PlanningResponse {
        reasoning: "Correct failed compilation in main.rs while preserving completed lib.rs work".to_string(),
        proposed_tasks: vec![
            ProposedTask {
                short_id: "REPLAN-FIX-001".to_string(),
                title: "Fix compiler errors in main.rs".to_string(),
                description: "Fix import and trait bounds in src/main.rs".to_string(),
                suggested_agent_id: None,
                affected_resources: vec!["src/main.rs".to_string()],
                estimated_size: Some("S".to_string()),
            }
        ],
        proposed_dependencies: vec![],
    };

    let replan_provider = MockLlmProvider::new().with_response(corrective_plan);
    let new_proposal_id =
        ReplanEngine::execute_replan(&pool, &replan_provider, &replan_req)
            .await
            .expect("Execute replan");

    // Verify corrective proposal created
    assert_ne!(new_proposal_id, initial_proposal.id);

    // Verify human approval gate: new corrective tasks are in HumanReview (NOT auto-approved!)
    let project_tasks = TaskRepository::list_by_project(&pool, project.id)
        .await
        .unwrap();
    let fix_task = project_tasks
        .iter()
        .find(|t| t.short_id == "REPLAN-FIX-001")
        .expect("Fix task exists");

    assert_eq!(
        fix_task.status,
        TaskStatus::HumanReview,
        "Corrective task must wait for HumanReview approval"
    );

    // Human explicitly approves the corrective task
    let approved =
        CommandHandler::execute_approve_task(&pool, fix_task.id, "Operator Reviewer")
            .await
            .unwrap();
    assert_eq!(approved.status, TaskStatus::Approved);

    println!("✔ Test 11: Real Replanning Behind Human Gate PASSED\n");
}

// ============================================================================
// Test 12 & 13: Live Metrics & Coordinator Restart Recovery (Sections 17-18)
// ============================================================================
#[tokio::test]
async fn test_phase18_9_metrics_and_coordinator_restart_recovery() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    // 1. Verify Metrics Collector
    let metrics = MetricsCollector::collect(&pool).await.expect("Collect metrics");
    println!("=== Test 12: Live System Metrics ===");
    println!("Total tasks:            {}", metrics.total_tasks);
    println!("Tasks (completed):      {}", metrics.tasks_completed);
    println!("Tasks (failed):         {}", metrics.tasks_failed);
    println!("Total agents:           {}", metrics.total_agents);
    println!("Delivery success rate:  {:.2}%", metrics.delivery_success_rate_percent);

    // 2. Simulate Coordinator Restart Recovery
    println!("\n=== Test 13: Coordinator Restart Recovery ===");
    let recovery_report =
        CoordinatorRecoveryService::recover_on_startup(&pool, Some(&jetstream))
            .await
            .expect("Startup recovery");

    println!(
        "Republished pending deliveries: {}",
        recovery_report.pending_deliveries_republished
    );
    println!(
        "Reclaimed tasks count:          {}",
        recovery_report.sweep_summary.tasks_reclaimed.len()
    );

    println!("✔ Tests 12 & 13: Metrics & Coordinator Restart Recovery PASSED\n");
}

// ============================================================================
// Test 14: Distributed / Two-Agent Concurrent Execution (Section 19)
// ============================================================================
#[tokio::test]
async fn test_phase18_10_distributed_two_agent_execution() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: DB/NATS not available");
        return;
    };

    let repo_path = PathBuf::from(MINIGREP_PATH);
    let identity = RepositoryIdentity::discover(&repo_path).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Minigrep Concurrent Distributed Validation".to_string(),
            description: "Test two parallel agents operating on minigrep repo".to_string(),
        },
    )
    .await
    .unwrap();

    ProjectRepository::update_git_identity(
        &pool,
        project.id,
        &identity.repo_root.to_string_lossy(),
        &identity.base_branch,
        Some(&identity.head_commit_sha),
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "mock".to_string(),
            ai_model: "mock-v1".to_string(),
            raw_prompt: "dist".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "DIST-001".to_string(),
            title: "Task 1 - Concurrent Feature A".to_string(),
            description: "Concurrent work on feature A".to_string(),
            affected_resources: vec!["src/feature_a.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "DIST-002".to_string(),
            title: "Task 2 - Concurrent Feature B".to_string(),
            description: "Concurrent work on feature B".to_string(),
            affected_resources: vec!["src/feature_b.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    // Register two agents (simulating distributed or multi-agent workers)
    let ag1 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        ag1,
        &NewAgent {
            human_owner: "DistWorker1".to_string(),
            api_key_hash: "k_dist1".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{ag1}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, ag1, AgentStatus::Idle)
        .await
        .unwrap();

    let ag2 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        ag2,
        &NewAgent {
            human_owner: "DistWorker2".to_string(),
            api_key_hash: "k_dist2".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{ag2}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, ag2, AgentStatus::Idle)
        .await
        .unwrap();

    // Set human review gate and targeted assignments
    TaskRepository::update_status(&pool, task_1.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_1.id, Some(ag1))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task_2.id, Some(ag2))
        .await
        .unwrap();

    // Human operator approves both tasks
    CommandHandler::execute_approve_task(&pool, task_1.id, "Lead Operator")
        .await
        .unwrap();
    CommandHandler::execute_approve_task(&pool, task_2.id, "Lead Operator")
        .await
        .unwrap();

    // Assignment worker assigns both tasks in parallel
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();

    assert_eq!(assignments.len(), 2, "Both tasks must be assigned in parallel");

    let wt1 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "DIST-001");
    let wt2 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "DIST-002");

    assert!(wt1.exists(), "Worktree 1 must exist");
    assert!(wt2.exists(), "Worktree 2 must exist");
    assert_ne!(wt1, wt2, "Worktrees must be completely isolated");

    // Start event subscriber to process completion events
    let sub_pool = pool.clone();
    let events_consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();
    let subscriber_handle = tokio::spawn(async move {
        use futures::StreamExt;
        let mut messages = events_consumer.messages().await.unwrap();
        while let Some(Ok(msg)) = messages.next().await {
            let _ = msg.ack().await;
            if let Ok(agent_msg) = serde_json::from_slice::<AgentMessage>(&msg.payload) {
                let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg).await;
            }
        }
    });

    // Write simulated work inside both isolated worktrees
    tokio::fs::write(wt1.join("src/feature_a.rs"), "// Feature A implementation\n")
        .await
        .unwrap();
    tokio::fs::write(wt2.join("src/feature_b.rs"), "// Feature B implementation\n")
        .await
        .unwrap();

    // Agent 1 reports completion over NATS
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: ag1,
            task_id: task_1.id,
            summary: "Completed feature A".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    // Agent 2 reports completion over NATS
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: ag2,
            task_id: task_2.id,
            summary: "Completed feature B".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    // Allow completion events to settle
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    subscriber_handle.abort();

    // Verify both tasks Completed in PostgreSQL
    let t1_done = TaskRepository::find_by_id(&pool, task_1.id).await.unwrap().unwrap();
    let t2_done = TaskRepository::find_by_id(&pool, task_2.id).await.unwrap().unwrap();
    assert_eq!(t1_done.status, TaskStatus::Completed);
    assert_eq!(t2_done.status, TaskStatus::Completed);

    // Verify both worktrees were cleanly cleaned up
    assert!(!wt1.exists(), "Worktree 1 must be cleaned up");
    assert!(!wt2.exists(), "Worktree 2 must be cleaned up");

    // Clean up branches
    let b1 = BranchStrategy::task_branch_name("DIST-001");
    let b2 = BranchStrategy::task_branch_name("DIST-002");
    let _ = tokio::process::Command::new("git")
        .args(["branch", "-D", &b1, &b2])
        .current_dir(&identity.repo_root)
        .output()
        .await;

    // Verify main repository clean
    let clean_after = identity.is_clean().await.unwrap();
    assert!(clean_after, "Repository must remain clean after distributed execution");

    println!("✔ Test 14: Distributed / Two-Agent Concurrent Execution PASSED\n");
}

