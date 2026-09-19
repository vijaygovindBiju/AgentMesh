use std::path::PathBuf;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::AgentMessage;
use coordinator::ai::mock::MockLlmProvider;
use coordinator::coordinator::{
    AssignmentService, CommandHandler, CoordinatorCommand, CoordinatorCore, CoordinatorState,
};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, ProjectRepository, ProposalRepository, TaskDeliveryRepository,
    TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, DeliveryStatus, DependencyKind, NewAgent, NewProject,
    NewProposal, NewTask, NewTaskDelivery, NewTaskDependency, TaskStatus,
};
use coordinator::git::{AgentWorkspace, RepositoryIdentity};
use coordinator::messaging::{connect, ensure_streams, EventSubscriber};
use coordinator::observability::{CoordinatorEventRepository, MetricsCollector};
use coordinator::reliability::{CoordinatorRecoveryService, StaleTaskSweeper};

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

// ----------------------------------------------------------------------------
// Scenario A: Startup Recovery
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_a_startup_recovery() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Startup Recovery Proj".to_string(),
            description: "Testing startup recovery".to_string(),
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
            raw_prompt: "startup recovery".to_string(),
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
            short_id: "RECOV-001".to_string(),
            title: "Task awaiting recovery".to_string(),
            description: "Simulating coordinator restart during task".to_string(),
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
            human_owner: "Crashed Agent Owner".to_string(),
            api_key_hash: "key_crashed".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Mark agent offline and last seen 60s ago
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Offline).await.unwrap();

    // Simulate task assigned to this agent before crash
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

    // Insert pending delivery that was unconfirmed
    let idempotency_key = format!("{}:1", task.id);
    TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id: task.id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{agent_id}"),
            idempotency_key: idempotency_key.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(60),
        },
    )
    .await
    .unwrap();

    // 1. First recovery pass
    let report1 = CoordinatorRecoveryService::recover_on_startup(&pool, Some(&jetstream))
        .await
        .unwrap();
    assert!(
        report1.sweep_summary.tasks_reclaimed.contains(&task.id),
        "Orphan task must be reclaimed on startup"
    );

    let reclaimed_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(
        reclaimed_task.status,
        TaskStatus::Approved,
        "Task with attempt 1 must be reverted to Approved"
    );
    assert_eq!(reclaimed_task.assigned_agent_id, None, "Agent must be unassigned");

    // 2. Second recovery pass: verify full idempotency
    let report2 = CoordinatorRecoveryService::recover_on_startup(&pool, Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(
        report2.pending_deliveries_republished, 0,
        "Idempotent recovery should have no remaining pending deliveries to republish"
    );
}

// ----------------------------------------------------------------------------
// Scenario B: Stale Task Sweeper and Reclamation
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_b_stale_task_sweep_and_reclamation() {
    let Some((pool, _client, _js)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Stale Sweeper Proj".to_string(),
            description: "Testing stale task sweeping".to_string(),
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
            raw_prompt: "stale sweep".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // Task 1: 1 attempt (should revert to Approved)
    let t1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "SWEEP-001".to_string(),
            title: "Task with attempt 1".to_string(),
            description: "Reverts to Approved".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    // Task 2: 3 attempts (should revert to HumanReview)
    let t2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "SWEEP-002".to_string(),
            title: "Task with attempt 3".to_string(),
            description: "Reverts to HumanReview".to_string(),
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
            human_owner: "Unresponsive Agent".to_string(),
            api_key_hash: "key_unresponsive".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Mark agent offline and last seen long ago
    sqlx::query!(
        "UPDATE agents SET status = 'offline', last_seen = NOW() - INTERVAL '120 seconds' WHERE id = $1",
        agent_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Set up T1 with 1 delivery
    TaskRepository::update_status(&pool, t1.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, t1.id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, t1.id, TaskStatus::Executing).await.unwrap();
    TaskDeliveryRepository::create(
        &pool,
        &NewTaskDelivery {
            task_id: t1.id,
            agent_id,
            attempt: 1,
            nats_stream: "TASK_ASSIGNMENTS".to_string(),
            nats_subject: format!("coordinator.tasks.assign.{agent_id}"),
            idempotency_key: format!("{}:1", t1.id),
            expires_at: Utc::now() + chrono::Duration::seconds(60),
        },
    )
    .await
    .unwrap();

    // Set up T2 with 3 deliveries
    TaskRepository::update_status(&pool, t2.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, t2.id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, t2.id, TaskStatus::Executing).await.unwrap();
    for att in 1..=3 {
        TaskDeliveryRepository::create(
            &pool,
            &NewTaskDelivery {
                task_id: t2.id,
                agent_id,
                attempt: att,
                nats_stream: "TASK_ASSIGNMENTS".to_string(),
                nats_subject: format!("coordinator.tasks.assign.{agent_id}"),
                idempotency_key: format!("{}:{}", t2.id, att),
                expires_at: Utc::now() + chrono::Duration::seconds(60),
            },
        )
        .await
        .unwrap();
    }

    // Run stale task sweeper with 30s timeout
    let sweep_res = StaleTaskSweeper::sweep(&pool, chrono::Duration::seconds(30)).await.unwrap();
    assert!(sweep_res.tasks_reclaimed.contains(&t1.id));
    assert!(sweep_res.tasks_reclaimed.contains(&t2.id));

    let t1_after = TaskRepository::find_by_id(&pool, t1.id).await.unwrap().unwrap();
    assert_eq!(t1_after.status, TaskStatus::Approved, "Task with < 3 attempts goes to Approved");

    let t2_after = TaskRepository::find_by_id(&pool, t2.id).await.unwrap().unwrap();
    assert_eq!(t2_after.status, TaskStatus::HumanReview, "Task with >= 3 attempts goes to HumanReview");
}

// ----------------------------------------------------------------------------
// Scenario C: Git Workspace Preparation & Finalization
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_c_git_workspace_preparation_and_finalization() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let repo_dir = create_temp_repo("agentmesh_phase16_git");
    let identity = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    // Add initial commit so HEAD exists
    tokio::fs::write(repo_dir.join("README.md"), "# Initial Commit\n").await.unwrap();
    let _ = tokio::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&repo_dir)
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&repo_dir)
        .output()
        .await;

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Git Workspace Proj".to_string(),
            description: "Testing git workspace preparation".to_string(),
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
            raw_prompt: "git workspace".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "GIT-001".to_string(),
            title: "First feature task".to_string(),
            description: "Implement feature 1".to_string(),
            affected_resources: vec!["feature1.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "GIT-002".to_string(),
            title: "Second feature task".to_string(),
            description: "Implement feature 2".to_string(),
            affected_resources: vec!["feature2.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    // Register two idle agents
    let a1_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a1_id,
        &NewAgent {
            human_owner: "Agent 1".to_string(),
            api_key_hash: "key_a1".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a1_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, a1_id, AgentStatus::Idle).await.unwrap();

    let a2_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a2_id,
        &NewAgent {
            human_owner: "Agent 2".to_string(),
            api_key_hash: "key_a2".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a2_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, a2_id, AgentStatus::Idle).await.unwrap();

    // Approve both tasks
    CommandHandler::execute_approve_task(&pool, task1.id, "Operator").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task2.id, "Operator").await.unwrap();

    // Run assignment cycle: should automatically prepare isolated worktrees!
    let assignments = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assignments.len(), 2, "Both tasks must be assigned");

    let expected_ws1 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "GIT-001");
    let expected_ws2 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "GIT-002");

    assert!(expected_ws1.exists(), "Worktree for GIT-001 must exist on disk");
    assert!(expected_ws2.exists(), "Worktree for GIT-002 must exist on disk");
    assert_ne!(expected_ws1, expected_ws2, "Worktrees must be in isolated directories");

    // Simulate Agent 1 doing work in worktree 1
    tokio::fs::write(expected_ws1.join("feature1.rs"), "pub fn f1() -> bool { true }\n").await.unwrap();

    // Report Completed for task 1
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: a1_id,
            task_id: task1.id,
            summary: "Finished feature 1".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    // Worktree 1 should now be finalized and cleaned up
    assert!(!expected_ws1.exists(), "Worktree 1 must be cleaned up on completion");

    let task1_after = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    assert_eq!(task1_after.status, TaskStatus::Completed);

    let git_ctx = TaskRepository::find_git_context(&pool, task1.id).await.unwrap().unwrap();
    assert!(git_ctx.completion_commit_sha.is_some(), "Completion commit SHA must be recorded");

    // Cleanup worktree 2
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

// ----------------------------------------------------------------------------
// Scenario D: Dependency Waiting & Automatic Unblocking
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_d_dependency_waiting_and_automatic_unblocking() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Dep Unblock Proj".to_string(),
            description: "Testing dependency unblocking".to_string(),
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
            raw_prompt: "dep unblock".to_string(),
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
            short_id: "DEP-A".to_string(),
            title: "Task A (Prerequisite)".to_string(),
            description: "Must complete first".to_string(),
            affected_resources: vec![],
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
            short_id: "DEP-B".to_string(),
            title: "Task B (Dependent)".to_string(),
            description: "Blocked on Task A".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

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

    // Approve both tasks
    CommandHandler::execute_approve_task(&pool, task_a.id, "Operator").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task_b.id, "Operator").await.unwrap();

    // Register 2 agents
    let a1 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a1,
        &NewAgent {
            human_owner: "Agent 1".to_string(),
            api_key_hash: "key_a1".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a1}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, a1, AgentStatus::Idle).await.unwrap();

    let a2 = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a2,
        &NewAgent {
            human_owner: "Agent 2".to_string(),
            api_key_hash: "key_a2".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a2}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, a2, AgentStatus::Idle).await.unwrap();

    // First assignment cycle: only Task A should be assigned, Task B has uncompleted blocker
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].task_id, task_a.id, "Task A must be assigned first");

    // Simulate Agent 2 reporting Blocked on Task B
    TaskRepository::update_status(&pool, task_b.id, TaskStatus::Blocked).await.unwrap();
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Blocked {
            agent_id: a2,
            task_id: task_b.id,
            reason: "Waiting on DEP-A".to_string(),
            blocking_task_id: Some(task_a.id),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    let task_b_before = TaskRepository::find_by_id(&pool, task_b.id).await.unwrap().unwrap();
    assert_eq!(task_b_before.status, TaskStatus::Blocked);

    // Agent 1 reports Task A Completed
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: a1,
            task_id: task_a.id,
            summary: "Task A finished".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    // Verify Task B was unblocked to Approved automatically!
    let task_b_after = TaskRepository::find_by_id(&pool, task_b.id).await.unwrap().unwrap();
    assert_eq!(
        task_b_after.status,
        TaskStatus::Approved,
        "Task B must automatically transition to Approved after Task A completes"
    );

    // Free agent 2 to idle and run next assignment cycle: Task B is now assigned!
    AgentRepository::set_current_task(&pool, a2, None, AgentStatus::Idle).await.unwrap();
    let assigned2 = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned2.len(), 1);
    assert_eq!(assigned2[0].task_id, task_b.id, "Task B must now be assigned");
}

// ----------------------------------------------------------------------------
// Scenario E: Task Cancellation Lifecycle
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_e_task_cancellation_lifecycle() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Cancel Proj".to_string(),
            description: "Testing cancellation".to_string(),
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
            description: "Cancellation test".to_string(),
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
            human_owner: "Agent Cancel Target".to_string(),
            api_key_hash: "key_cancel".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    CommandHandler::execute_approve_task(&pool, task.id, "Operator").await.unwrap();
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Idle).await.unwrap();

    // Assign task to agent
    let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));
    coordinator.set_active_project(project.id);
    let _ = coordinator.run_assignment_cycle().await.unwrap();

    // Verify task is Assigned and agent is Busy
    let t_assigned = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t_assigned.status, TaskStatus::Assigned);
    let a_busy = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert_eq!(a_busy.status, AgentStatus::Busy);

    // Operator cancels task via coordinator command
    let cancel_cmd = CoordinatorCommand::CancelTask {
        task_id: task.id,
        reason: "User requested task cancellation".to_string(),
    };
    coordinator.handle_command(cancel_cmd).await.unwrap();

    // Verify task is Cancelled
    let t_cancelled = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t_cancelled.status, TaskStatus::Cancelled);

    // Verify agent was freed to Idle
    let a_idle = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert_eq!(a_idle.status, AgentStatus::Idle);
    assert_eq!(a_idle.current_task_id, None);

    // Verify deliveries are marked Terminal
    let deliveries = TaskDeliveryRepository::list_by_task(&pool, task.id).await.unwrap();
    assert!(deliveries.iter().all(|d| d.status == DeliveryStatus::Terminal));

    // Verify cannot cancel an already cancelled task (terminal status rejection)
    let re_cancel = CommandHandler::execute_cancel_task(&pool, task.id, "Second cancel").await;
    assert!(re_cancel.is_err(), "Must reject cancellation on terminal task");
}

// ----------------------------------------------------------------------------
// Scenario F: Dynamic Replanning Behind Human Approval Gate
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_f_dynamic_replanning_behind_human_approval_gate() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Replanning Proj".to_string(),
            description: "Testing dynamic replanning".to_string(),
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
            raw_prompt: "initial plan".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let failed_task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "FAIL-001".to_string(),
            title: "Task that failed".to_string(),
            description: "Encountered critical issue".to_string(),
            affected_resources: vec!["api.rs".to_string()],
            estimated_size: Some("M".to_string()),
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, failed_task.id, TaskStatus::Failed).await.unwrap();

    let mut coordinator = CoordinatorCore::new(pool.clone(), Some(jetstream.clone()));
    coordinator.set_active_project(project.id);
    let _ = coordinator.transition_state(CoordinatorState::Executing);

    // Trigger replanning using mock provider
    let mock_provider = MockLlmProvider::new();
    let proposal_id = coordinator
        .trigger_replanning(project.id, "Task FAIL-001 failed", Some(&mock_provider))
        .await
        .unwrap();

    assert_eq!(
        coordinator.state(),
        CoordinatorState::HumanReview,
        "Coordinator state must transition to HumanReview after replanning"
    );

    // Check all tasks belonging to new proposal
    let tasks = TaskRepository::list_by_project(&pool, project.id).await.unwrap();
    let new_tasks: Vec<_> = tasks.into_iter().filter(|t| t.proposal_id == proposal_id).collect();

    assert!(!new_tasks.is_empty(), "Replanning must create new tasks");
    for nt in new_tasks {
        assert_eq!(
            nt.status,
            TaskStatus::HumanReview,
            "All new tasks must wait in HumanReview behind the human approval gate"
        );
    }
}

// ----------------------------------------------------------------------------
// Scenario G: Task Reassignment with Multi-Attempt Delivery Tracking
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_g_task_reassignment_with_multi_attempt_delivery() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Multi-Attempt Proj".to_string(),
            description: "Testing multi-attempt deliveries".to_string(),
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
            raw_prompt: "multi attempt".to_string(),
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
            short_id: "RETRY-001".to_string(),
            title: "Task to reassign".to_string(),
            description: "Retry test".to_string(),
            affected_resources: vec![],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    let a1_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a1_id,
        &NewAgent {
            human_owner: "Agent 1 (Fails)".to_string(),
            api_key_hash: "key_a1".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a1_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let a2_id = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        a2_id,
        &NewAgent {
            human_owner: "Agent 2 (Takes over)".to_string(),
            api_key_hash: "key_a2".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string()],
            nats_subject: format!("agents.{a2_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    CommandHandler::execute_approve_task(&pool, task.id, "Operator").await.unwrap();
    AgentRepository::update_status(&pool, a1_id, AgentStatus::Idle).await.unwrap();

    // First assignment to Agent 1 (Attempt 1)
    let as1 = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(as1.len(), 1);
    assert_eq!(as1[0].agent_id, a1_id);
    assert_eq!(as1[0].idempotency_key, format!("{}:1", task.id));

    // Agent 1 crashes -> stale sweep reclaims task to Approved
    sqlx::query!(
        "UPDATE agents SET status = 'offline', last_seen = NOW() - INTERVAL '120 seconds' WHERE id = $1",
        a1_id
    )
    .execute(&pool)
    .await
    .unwrap();
    let sweep = StaleTaskSweeper::sweep(&pool, chrono::Duration::seconds(30)).await.unwrap();
    assert!(sweep.tasks_reclaimed.contains(&task.id));

    // Free Agent 2 to Idle
    AgentRepository::update_status(&pool, a2_id, AgentStatus::Idle).await.unwrap();

    // Second assignment cycle: assigns to Agent 2 (Attempt 2)
    let as2 = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(as2.len(), 1);
    assert_eq!(as2[0].agent_id, a2_id);
    assert_eq!(as2[0].idempotency_key, format!("{}:2", task.id));

    // Verify both attempts exist in database
    let deliveries = TaskDeliveryRepository::list_by_task(&pool, task.id).await.unwrap();
    assert_eq!(deliveries.len(), 2, "Must preserve multi-attempt delivery history");
    assert_eq!(deliveries[0].attempt, 1);
    assert_eq!(deliveries[1].attempt, 2);
}

// ----------------------------------------------------------------------------
// Scenario H: Live Diagnostics and Metrics Collection
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_h_live_diagnostics_and_metrics_collection() {
    let Some((pool, _client, _js)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 16 Metrics Proj".to_string(),
            description: "Testing metrics collection".to_string(),
        },
    )
    .await
    .unwrap();

    // Record sample coordinator observability events
    CoordinatorEventRepository::record(
        &pool,
        "task.started",
        Some(project.id),
        None,
        None,
        "Task started test event".to_string(),
        serde_json::json!({ "test": true }),
    )
    .await
    .unwrap();

    CoordinatorEventRepository::record(
        &pool,
        "task.completed",
        Some(project.id),
        None,
        None,
        "Task completed test event".to_string(),
        serde_json::json!({ "test": true }),
    )
    .await
    .unwrap();

    let recent = CoordinatorEventRepository::find_by_project(&pool, project.id, 10).await.unwrap();
    assert_eq!(recent.len(), 2, "Must retrieve recorded coordinator events");

    // Collect system metrics
    let metrics = MetricsCollector::collect(&pool).await.unwrap();
    let _ = metrics.total_tasks;
    let _ = metrics.total_agents;
    assert!(metrics.delivery_success_rate_percent >= 0.0);
}

// ----------------------------------------------------------------------------
// Scenario I: Real AGY Error Propagation and Secret Redaction
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_i_agy_error_propagation_and_secret_redaction() {
    use agent_protocol::security::SecretRedactor;

    // 1. Verify SecretRedactor redacts Postgres URLs with passwords and Anthropic/Gemini keys
    let sensitive_url = "postgres://admin:super_secret_pw@db.prod.internal:5432/main";
    let redacted_url = SecretRedactor::redact(sensitive_url);
    assert!(!redacted_url.contains("super_secret_pw"));
    assert!(redacted_url.contains("[REDACTED]"));

    let sensitive_key = "anthropic_api_key=sk-ant-api03-abcdef123456789012345678901234567890-XYZ123";
    let redacted_key = SecretRedactor::redact(sensitive_key);
    assert!(!redacted_key.contains("abcdef123456789012345678901234567890"));
    assert!(redacted_key.contains("[REDACTED_API_KEY]"));

    // 2. Verify AgyResultPayload error field deserialization
    use agent_agy::parser::AgyResultPayload;

    // A: Error as a string
    let json_str = r#"{"status": "ERROR", "error": "Database connection error to postgres://admin:secret@host:5432/db"}"#;
    let payload: AgyResultPayload = serde_json::from_str(json_str).unwrap();
    assert_eq!(payload.status, "ERROR");
    assert!(payload.error.unwrap().contains("Database connection error"));

    // B: Error as an object
    let json_obj = r#"{"status": "ERROR", "error": {"code": 429, "message": "RESOURCE_EXHAUSTED"}}"#;
    let payload_obj: AgyResultPayload = serde_json::from_str(json_obj).unwrap();
    assert_eq!(payload_obj.status, "ERROR");
    assert!(payload_obj.error.unwrap().contains("RESOURCE_EXHAUSTED"));
}

// ----------------------------------------------------------------------------
// Scenario 10 (16.12): Two-Agent Runtime Concurrent Worktrees
// ----------------------------------------------------------------------------
#[tokio::test]
async fn test_two_agent_runtime_concurrent_worktrees() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let repo_dir = create_temp_repo("agentmesh_concurrent_fleet");
    let identity = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    // Create initial commit
    tokio::fs::create_dir_all(repo_dir.join("src")).await.unwrap();
    tokio::fs::write(repo_dir.join("src/lib.rs"), "// Base lib\n").await.unwrap();
    let _ = tokio::process::Command::new("git")
        .args(["add", "src/lib.rs"])
        .current_dir(&repo_dir)
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .args(["commit", "-m", "Initial commit on base branch"])
        .current_dir(&repo_dir)
        .output()
        .await;

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Concurrent Fleet Project".to_string(),
            description: "Executing two concurrent tasks in isolated worktrees".to_string(),
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
            raw_prompt: "concurrent fleet".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "CONC-001".to_string(),
            title: "Implement auth service".to_string(),
            description: "Auth implementation".to_string(),
            affected_resources: vec!["src/auth.rs".to_string()],
            estimated_size: Some("M".to_string()),
        },
    )
    .await
    .unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "CONC-002".to_string(),
            title: "Implement billing service".to_string(),
            description: "Billing implementation".to_string(),
            affected_resources: vec!["src/billing.rs".to_string()],
            estimated_size: Some("M".to_string()),
        },
    )
    .await
    .unwrap();

    // Register Agent A and Agent B
    let agent_a = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agent_a,
        &NewAgent {
            human_owner: "Alice (Agent A)".to_string(),
            api_key_hash: "key_alice".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{agent_a}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_a, AgentStatus::Idle).await.unwrap();

    let agent_b = Uuid::new_v4();
    AgentRepository::create_with_id(
        &pool,
        agent_b,
        &NewAgent {
            human_owner: "Bob (Agent B)".to_string(),
            api_key_hash: "key_bob".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["backend".to_string(), "rust".to_string()],
            nats_subject: format!("agents.{agent_b}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_b, AgentStatus::Idle).await.unwrap();

    // Approve both tasks
    CommandHandler::execute_approve_task(&pool, task1.id, "Operator").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task2.id, "Operator").await.unwrap();

    // Assignment cycle automatically creates isolated worktrees for both
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned.len(), 2, "Both tasks must be assigned simultaneously");

    let ws1 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "CONC-001");
    let ws2 = AgentWorkspace::expected_worktree_path(&identity.repo_root, "CONC-002");

    assert!(ws1.exists(), "Worktree for CONC-001 must exist");
    assert!(ws2.exists(), "Worktree for CONC-002 must exist");

    // Both agents work concurrently in their worktrees
    tokio::fs::write(ws1.join("src/auth.rs"), "pub fn authenticate() -> bool { true }\n").await.unwrap();
    tokio::fs::write(ws2.join("src/billing.rs"), "pub fn charge() -> bool { true }\n").await.unwrap();

    // Both agents report completion
    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: agent_a,
            task_id: task1.id,
            summary: "Completed auth service".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    EventSubscriber::handle_agent_message_with_jetstream(
        &pool,
        AgentMessage::Completed {
            agent_id: agent_b,
            task_id: task2.id,
            summary: "Completed billing service".to_string(),
            timestamp: Utc::now(),
        },
        Some(&jetstream),
    )
    .await
    .unwrap();

    // Verify both tasks are Completed, both worktrees cleaned up, and completion SHAs recorded
    assert!(!ws1.exists(), "Worktree 1 must be cleaned up");
    assert!(!ws2.exists(), "Worktree 2 must be cleaned up");

    let t1_final = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    let t2_final = TaskRepository::find_by_id(&pool, task2.id).await.unwrap().unwrap();
    assert_eq!(t1_final.status, TaskStatus::Completed);
    assert_eq!(t2_final.status, TaskStatus::Completed);

    let gc1 = TaskRepository::find_git_context(&pool, task1.id).await.unwrap().unwrap();
    let gc2 = TaskRepository::find_git_context(&pool, task2.id).await.unwrap().unwrap();
    assert!(gc1.completion_commit_sha.is_some());
    assert!(gc2.completion_commit_sha.is_some());

    // Both agents returned to Idle
    let ag_a = AgentRepository::find_by_id(&pool, agent_a).await.unwrap().unwrap();
    let ag_b = AgentRepository::find_by_id(&pool, agent_b).await.unwrap().unwrap();
    assert_eq!(ag_a.status, AgentStatus::Idle);
    assert_eq!(ag_b.status, AgentStatus::Idle);

    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}
