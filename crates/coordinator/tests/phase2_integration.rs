use chrono::Utc;
use futures::StreamExt;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

use agent_mock::{MockAgent, MockAgentRunner};
use agent_protocol::{AgentMessage, CoordinatorMessage, TaskSpec};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentEventRepository, AgentRepository, ProjectRepository, ProposalRepository,
    TaskDeliveryRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, DeliveryStatus, NewProject, NewProposal, NewTask, NewTaskDelivery, TaskStatus,
};
use coordinator::messaging::{
    connect, ensure_streams, EventSubscriber, RegistrationHandler, TaskPublisher,
};

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token =
        std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (client, jetstream) = connect(&nats_url, Some(&nats_token)).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, client, jetstream))
}

#[tokio::test]
async fn test_mock_agent_registration_and_full_lifecycle() {
    let Some((pool, client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping integration test: services not reachable");
        return;
    };

    // 1. Create a Mock Agent
    let agent_id = Uuid::new_v4();
    let mut mock_agent =
        MockAgent::new("Alice", "test_key_123").with_delay(Duration::from_millis(20));
    mock_agent.id = agent_id;

    // 2. Mock agent registers with coordinator
    let reg_msg = AgentMessage::Register {
        agent_id: mock_agent.id,
        human_owner: mock_agent.human_owner.clone(),
        adapter_type: "Mock".to_string(),
        capabilities: mock_agent.capabilities.clone(),
        profile: None,
        api_key: mock_agent.api_key.clone(),
    };

    let reg_resp = RegistrationHandler::process_registration(&pool, reg_msg)
        .await
        .expect("Registration failed");

    let event_subject = match reg_resp {
        CoordinatorMessage::RegisterResponse {
            status,
            nats_subject,
            ..
        } => {
            assert_eq!(status, "ok");
            nats_subject.unwrap()
        }
        _ => panic!("Expected RegisterResponse"),
    };

    let registered_agent = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registered_agent.human_owner, "Alice");
    assert_eq!(registered_agent.adapter_type, AdapterType::Mock);

    // 3. Coordinator sets up Project, Proposal, Task
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 2 Integration Proj".to_string(),
            description: "End-to-end lifecycle verification".to_string(),
        },
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "anthropic".to_string(),
            ai_model: "claude-3-5-sonnet".to_string(),
            raw_prompt: "p2".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "P2-001".to_string(),
            title: "Phase 2 Core Task".to_string(),
            description: "Verify JetStream delivery and event reporting".to_string(),
            affected_resources: vec!["src/main.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Advance task through approval to Assigned
    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned)
        .await
        .unwrap();

    // 4. Create TaskDelivery record (Pending)
    let idempotency_key = format!("{}:1", task.id);
    let delivery = TaskDeliveryRepository::create(
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

    // 5. Create consumer on agent side BEFORE publishing so message is captured
    let consumer = MockAgentRunner::create_task_consumer(&jetstream, agent_id)
        .await
        .expect("Failed to create agent task consumer");

    // 6. Coordinator publishes TaskAssignment to JetStream
    let spec = TaskSpec::new(
        task.id,
        &task.short_id,
        &task.title,
        &task.description,
        task.resources(),
        vec![],
        &idempotency_key,
    );

    let seq = TaskPublisher::publish_assignment(&jetstream, agent_id, &spec)
        .await
        .expect("Publish assignment failed");
    assert!(seq > 0);

    // Update TaskDelivery to Delivered
    TaskDeliveryRepository::mark_delivered(&pool, delivery.id, seq as i64)
        .await
        .unwrap();

    // 7. Mock Agent consumes assignment from JetStream
    let mut messages = consumer.messages().await.unwrap();
    let msg = messages
        .next()
        .await
        .unwrap()
        .expect("Should receive assignment message");
    msg.ack().await.expect("JetStream transport ACK failed");

    let coord_msg: CoordinatorMessage = serde_json::from_slice(&msg.payload).unwrap();
    let received_spec = match coord_msg {
        CoordinatorMessage::TaskAssignment { spec } => spec,
        other => panic!("Unexpected message: {other:?}"),
    };
    assert_eq!(received_spec.task_id, task.id);

    // 8. Mock agent executes task lifecycle and emits events
    MockAgentRunner::execute_task(&client, &mock_agent, &received_spec, &event_subject)
        .await
        .expect("Task execution failed");

    // 9. Verify Coordinator processing of events:
    // We simulate subscriber handling the emitted events or process directly:
    // (A) TaskStarted
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::TaskStarted {
            agent_id,
            task_id: task.id,
            idempotency_key: idempotency_key.clone(),
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    let d_after_start = TaskDeliveryRepository::find_by_id(&pool, delivery.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(d_after_start.status, DeliveryStatus::Acknowledged);

    let t_after_start = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t_after_start.status, TaskStatus::Executing);

    // (B) ProgressUpdate
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::ProgressUpdate {
            agent_id,
            task_id: task.id,
            message: "Writing unit tests".to_string(),
            percent: 60,
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    // (C) Completed
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::Completed {
            agent_id,
            task_id: task.id,
            summary: "Done!".to_string(),
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    let t_after_complete = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t_after_complete.status, TaskStatus::Completed);

    let a_after_complete = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        a_after_complete.status,
        coordinator::domain::AgentStatus::Idle
    );

    // Verify all 3 events recorded in agent_events table
    let events = AgentEventRepository::list_by_task(&pool, task.id)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}

#[tokio::test]
async fn test_mock_agent_blocked_and_resumed_lifecycle() {
    let Some((pool, client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping integration test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let blocker_id = Uuid::new_v4();
    let mut mock_agent = MockAgent::new("Bob", "bob_key")
        .with_delay(Duration::from_millis(10))
        .with_blocker(blocker_id);
    mock_agent.id = agent_id;

    // Register agent so FK constraint on tasks.assigned_agent_id is satisfied
    let reg_msg = AgentMessage::Register {
        agent_id: mock_agent.id,
        human_owner: mock_agent.human_owner.clone(),
        adapter_type: "Mock".to_string(),
        capabilities: mock_agent.capabilities.clone(),
        profile: None,
        api_key: mock_agent.api_key.clone(),
    };
    RegistrationHandler::process_registration(&pool, reg_msg)
        .await
        .unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Blocked Lifecycle Proj".to_string(),
            description: "Testing Blocked event".to_string(),
        },
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "anthropic".to_string(),
            ai_model: "claude-3-5-sonnet".to_string(),
            raw_prompt: "p".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "BLOCK-001".to_string(),
            title: "Blocked Task".to_string(),
            description: "Will report Blocked".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview)
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id))
        .await
        .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned)
        .await
        .unwrap();

    let spec = TaskSpec::new(
        task.id,
        &task.short_id,
        &task.title,
        &task.description,
        vec![],
        vec![blocker_id],
        format!("{}:1", task.id),
    );

    let event_subject = format!("agents.{agent_id}.events");

    // Execute task with mock agent (emits Started -> Progress -> Blocked -> Completed)
    MockAgentRunner::execute_task(&client, &mock_agent, &spec, &event_subject)
        .await
        .expect("Task execution failed");

    // Process TaskStarted
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::TaskStarted {
            agent_id,
            task_id: task.id,
            idempotency_key: spec.idempotency_key.clone(),
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    // Process Blocked
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::Blocked {
            agent_id,
            task_id: task.id,
            reason: "Waiting for schema".to_string(),
            blocking_task_id: Some(blocker_id),
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    let t_blocked = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t_blocked.status, TaskStatus::Blocked);

    // Resume: coordinator unblocks task
    TaskRepository::update_status(&pool, task.id, TaskStatus::Executing)
        .await
        .unwrap();

    // Process Completed
    EventSubscriber::handle_agent_message(
        &pool,
        AgentMessage::Completed {
            agent_id,
            task_id: task.id,
            summary: "Completed after unblock".to_string(),
            timestamp: Utc::now(),
        },
    )
    .await
    .unwrap();

    let t_completed = TaskRepository::find_by_id(&pool, task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(t_completed.status, TaskStatus::Completed);

    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}

#[tokio::test]
async fn test_jetstream_durable_redelivery_on_nak() {
    let Some((_pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping integration test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let spec = TaskSpec::new(
        task_id,
        "REDELIV-001",
        "Redelivery Test",
        "Test NAK triggers redelivery",
        vec![],
        vec![],
        format!("{}:1", task_id),
    );

    // 1. Create consumer for this agent
    let consumer = MockAgentRunner::create_task_consumer(&jetstream, agent_id)
        .await
        .unwrap();

    // 2. Publish task assignment
    TaskPublisher::publish_assignment(&jetstream, agent_id, &spec)
        .await
        .unwrap();

    // 3. First fetch: receive message and NAK it
    let mut messages = consumer.messages().await.unwrap();
    let msg1 = messages
        .next()
        .await
        .unwrap()
        .expect("Should receive message first time");

    // NAK the message to request redelivery
    msg1.ack_with(async_nats::jetstream::message::AckKind::Nak(None))
        .await
        .expect("NAK should succeed");

    // 4. Second fetch: JetStream redelivers the message!
    let msg2 = messages
        .next()
        .await
        .unwrap()
        .expect("JetStream must redeliver NAKed message");

    let coord_msg: CoordinatorMessage = serde_json::from_slice(&msg2.payload).unwrap();
    match coord_msg {
        CoordinatorMessage::TaskAssignment {
            spec: redelivered_spec,
        } => {
            assert_eq!(redelivered_spec.task_id, task_id);
            assert_eq!(redelivered_spec.idempotency_key, format!("{}:1", task_id));
        }
        _ => panic!("Expected TaskAssignment"),
    }

    // Now ACK to remove it cleanly from WorkQueue
    msg2.ack().await.expect("ACK should succeed");
}
