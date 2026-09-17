use std::sync::Arc;
use sqlx::PgPool;

use agent_protocol::AgentMessage;
use coordinator::coordinator::{
    ApprovalGateError, AssignmentService, CommandHandler, CoordinatorCore, CoordinatorState,
};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, OverlapWarningRepository, ProjectRepository, ProposalRepository,
    TaskDeliveryRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, DependencyKind, NewAgent, NewOverlapWarning, NewProject,
    NewProposal, NewTask, NewTaskDependency, OverlapSeverity, TaskStatus,
};
use coordinator::messaging::{connect, ensure_streams};

async fn setup_test_env() -> Option<(PgPool, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (_client, jetstream) = connect(&nats_url, Some(&nats_token)).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, jetstream))
}

#[tokio::test]
async fn test_approval_gating_unapproved_task_cannot_be_assigned() {
    let Some((pool, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Approval Gate Project".to_string(),
            description: "Testing human approval gate".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Alice".to_string(),
            api_key_hash: "hash".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: "agents.alice.events".to_string(),
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle).await.unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "GATE-001".to_string(),
            title: "Unapproved Task".to_string(),
            description: "Should not be assigned".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    // Task is in HumanReview
    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id)).await.unwrap();

    // 1. Attempt assignment without approval -> Must NOT assign!
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert!(assigned.is_empty(), "Unapproved task must NOT be assigned");

    let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::HumanReview);

    // 2. Human explicitly approves the task
    CommandHandler::execute_approve_task(&pool, task.id, "Alice")
        .await
        .expect("Approval should succeed");

    let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Approved);

    // 3. Now assignment cycle runs -> Must be assigned!
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].task_id, task.id);
    assert_eq!(assigned[0].agent_id, agent.id);

    let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Assigned);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}

#[tokio::test]
async fn test_dependency_tracking_b_waits_for_a_completed() {
    let Some((pool, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Dependency Chain Project".to_string(),
            description: "Testing Task B waits for Task A".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Bob".to_string(),
            api_key_hash: "hash".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec![],
            nats_subject: "agents.bob.events".to_string(),
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle).await.unwrap();

    // Task A (blocker)
    let task_a = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "DEP-A".to_string(),
            title: "Task A".to_string(),
            description: "Prerequisite".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Task B (dependent)
    let task_b = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "DEP-B".to_string(),
            title: "Task B".to_string(),
            description: "Dependent on Task A".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Add blocking dependency: B depends on A
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

    // Move both to HumanReview then Approve both
    TaskRepository::update_status(&pool, task_a.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task_b.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task_a.id, Some(agent.id)).await.unwrap();
    TaskRepository::assign_agent(&pool, task_b.id, Some(agent.id)).await.unwrap();

    CommandHandler::execute_approve_task(&pool, task_a.id, "Bob").await.unwrap();
    CommandHandler::execute_approve_task(&pool, task_b.id, "Bob").await.unwrap();

    // 1. Run assignment -> Only Task A should be assigned!
    let assigned = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].task_id, task_a.id);

    let tb = TaskRepository::find_by_id(&pool, task_b.id).await.unwrap().unwrap();
    assert_eq!(tb.status, TaskStatus::Approved, "Task B must remain Approved while Task A is not completed");

    // 2. Complete Task A and make agent Idle again
    TaskRepository::update_status(&pool, task_a.id, TaskStatus::Completed).await.unwrap();
    AgentRepository::set_current_task(&pool, agent.id, None, AgentStatus::Idle).await.unwrap();

    // 3. Run assignment again -> Now Task B must be assigned!
    let assigned2 = AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
        .await
        .unwrap();
    assert_eq!(assigned2.len(), 1);
    assert_eq!(assigned2[0].task_id, task_b.id);

    let tb_now = TaskRepository::find_by_id(&pool, task_b.id).await.unwrap().unwrap();
    assert_eq!(tb_now.status, TaskStatus::Assigned);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}

#[tokio::test]
async fn test_critical_overlap_blocks_approval_until_acknowledged() {
    let Some((pool, _)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Overlap Project".to_string(),
            description: "Testing critical overlap block".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "OVL-1".to_string(),
            title: "Task 1".to_string(),
            description: "Modifies schema".to_string(),
            affected_resources: vec!["migrations/001.sql".to_string()],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task1.id, TaskStatus::HumanReview).await.unwrap();

    let task2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "OVL-2".to_string(),
            title: "Task 2".to_string(),
            description: "Modifies same schema concurrently".to_string(),
            affected_resources: vec!["migrations/001.sql".to_string()],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task2.id, TaskStatus::HumanReview).await.unwrap();

    // Create an unacknowledged Critical Overlap
    let warning = OverlapWarningRepository::create(
        &pool,
        &NewOverlapWarning {
            project_id: project.id,
            task_ids: vec![task1.id, task2.id],
            resource: "migrations/001.sql".to_string(),
            severity: OverlapSeverity::Critical,
        },
    )
    .await
    .unwrap();
    assert!(!warning.acknowledged);

    // 1. Attempting to approve Task 1 must be BLOCKED
    let err = CommandHandler::execute_approve_task(&pool, task1.id, "Alice")
        .await
        .unwrap_err();

    match err {
        ApprovalGateError::BlockedByCriticalOverlap { task_id, resources } => {
            assert_eq!(task_id, task1.id);
            assert!(resources.contains(&"migrations/001.sql".to_string()));
        }
        other => panic!("Expected BlockedByCriticalOverlap, got {:?}", other),
    }

    let t1 = TaskRepository::find_by_id(&pool, task1.id).await.unwrap().unwrap();
    assert_eq!(t1.status, TaskStatus::HumanReview, "Task status must remain HumanReview when blocked");

    // 2. Operator acknowledges the overlap warning
    let ack_ok = CommandHandler::execute_acknowledge_overlap(&pool, warning.id).await.unwrap();
    assert!(ack_ok);

    // 3. Now approval must SUCCEED!
    let approved_task = CommandHandler::execute_approve_task(&pool, task1.id, "Alice")
        .await
        .expect("Approval should now succeed");
    assert_eq!(approved_task.status, TaskStatus::Approved);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

#[tokio::test]
async fn test_human_override_reassign_task() {
    let Some((pool, _)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Reassign Project".to_string(),
            description: "Testing human reassignment override".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_a = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Agent A".to_string(),
            api_key_hash: "hash_a".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec![],
            nats_subject: "agents.a.events".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_b = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Agent B".to_string(),
            api_key_hash: "hash_b".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec![],
            nats_subject: "agents.b.events".to_string(),
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent_b.id, AgentStatus::Idle).await.unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "REASSIGN-01".to_string(),
            title: "Task initially for A".to_string(),
            description: "Reassigning to B".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_a.id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Failed).await.unwrap();

    // Human operator reassigns to Agent B
    let updated = CommandHandler::execute_reassign_task(&pool, task.id, agent_b.id, "Alice")
        .await
        .expect("Reassign should succeed");

    assert_eq!(updated.assigned_agent_id, Some(agent_b.id));
    assert_eq!(updated.status, TaskStatus::Approved);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent_a.id).await.unwrap();
    AgentRepository::delete(&pool, agent_b.id).await.unwrap();
}

#[tokio::test]
async fn test_concurrency_locking_prevents_double_assignment() {
    let Some((pool, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Concurrency Test Project".to_string(),
            description: "Testing FOR UPDATE SKIP LOCKED".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Solo Agent".to_string(),
            api_key_hash: "hash_solo".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec![],
            nats_subject: "agents.solo.events".to_string(),
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle).await.unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "RACE-001".to_string(),
            title: "Single Task".to_string(),
            description: "Only one worker can assign this".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id)).await.unwrap();
    CommandHandler::execute_approve_task(&pool, task.id, "Alice").await.unwrap();

    // Spawn 5 concurrent workers attempting assignment simultaneously
    let mut handles = Vec::new();
    let pool_arc = Arc::new(pool.clone());
    let js_arc = Arc::new(jetstream.clone());
    let proj_id = project.id;

    for _ in 0..5 {
        let p = pool_arc.clone();
        let js = js_arc.clone();
        handles.push(tokio::spawn(async move {
            AssignmentService::assign_ready_tasks(&p, Some(proj_id), Some(&js)).await
        }));
    }

    let mut total_assigned = 0;
    for handle in handles {
        let res = handle.await.unwrap().unwrap();
        total_assigned += res.len();
    }

    assert_eq!(total_assigned, 1, "Exactly one worker must have successfully claimed and assigned the task");

    let deliveries = TaskDeliveryRepository::list_by_task(&pool, task.id).await.unwrap();
    assert_eq!(deliveries.len(), 1, "There must be exactly one delivery record");

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}

#[tokio::test]
async fn test_coordinator_core_engine_lifecycle_and_blocked_event() {
    let Some((pool, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Core Engine Project".to_string(),
            description: "Testing coordinator core engine".to_string(),
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
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let agent = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Agent Charlie".to_string(),
            api_key_hash: "hash_c".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec![],
            nats_subject: "agents.charlie.events".to_string(),
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle).await.unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "ENG-001".to_string(),
            title: "Engine Task".to_string(),
            description: "Testing state transitions".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id)).await.unwrap();

    let mut core = CoordinatorCore::new(pool.clone(), Some(jetstream));
    core.set_active_project(project.id);
    assert_eq!(core.state(), CoordinatorState::Idle);

    // Advance to HumanReview
    core.transition_state(CoordinatorState::ProjectInput).unwrap();
    core.transition_state(CoordinatorState::Planning).unwrap();
    core.transition_state(CoordinatorState::HumanReview).unwrap();

    // Approve task via command
    let evt = core.handle_command(coordinator::coordinator::CoordinatorCommand::ApproveTask {
        task_id: task.id,
        approved_by: "Alice".to_string(),
    })
    .await
    .unwrap();
    assert_eq!(evt, coordinator::coordinator::CoordinatorEvent::TaskApproved { task_id: task.id });

    // Run assignment cycle -> moves state to Executing
    let assigned = core.run_assignment_cycle().await.unwrap();
    assert_eq!(assigned.len(), 1);
    assert_eq!(core.state(), CoordinatorState::Executing);

    // Agent sends Blocked message
    core.handle_agent_message(AgentMessage::Blocked {
        agent_id: agent.id,
        task_id: task.id,
        reason: "Waiting for resource lock".to_string(),
        blocking_task_id: None,
        timestamp: chrono::Utc::now(),
    })
    .await
    .unwrap();

    let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Blocked, "Blocked message must update task to Blocked in DB");

    let a = AgentRepository::find_by_id(&pool, agent.id).await.unwrap().unwrap();
    assert_eq!(a.status, AgentStatus::Blocked, "Blocked message must update agent to Blocked in DB");

    // Agent sends Completed message
    core.handle_agent_message(AgentMessage::Completed {
        agent_id: agent.id,
        task_id: task.id,
        summary: "Task finished".to_string(),
        timestamp: chrono::Utc::now(),
    })
    .await
    .unwrap();

    let t = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(t.status, TaskStatus::Completed);
    assert_eq!(core.state(), CoordinatorState::Done, "All tasks completed -> Coordinator reaches Done");

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}
