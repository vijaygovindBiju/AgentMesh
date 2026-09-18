use std::io::Write;
use std::time::Duration;
use chrono::Utc;
use futures::StreamExt;
use sqlx::PgPool;
use uuid::Uuid;

use agent_agy::{AgyAgent, AgyAgentRunner};
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

struct TempScript {
    path: std::path::PathBuf,
}

impl TempScript {
    fn new(content: &str) -> Self {
        let path = std::env::temp_dir().join(format!("agy_test_subproc_{}.sh", Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { path }
    }
}

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (client, jetstream) = connect(&nats_url, Some(&nats_token)).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, client, jetstream))
}

#[tokio::test]
async fn test_agy_agent_registration() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let agy_agent = AgyAgent::new("AgyLead", "secret_agy_token")
        .with_id(agent_id)
        .with_capabilities(vec!["rust".to_string(), "backend".to_string()]);

    let reg_msg = AgentMessage::Register {
        agent_id: agy_agent.id,
        human_owner: agy_agent.human_owner.clone(),
        adapter_type: "Agy".to_string(),
        capabilities: agy_agent.capabilities.clone(),
        api_key: agy_agent.api_key.clone(),
    };

    let reg_resp = RegistrationHandler::process_registration(&pool, reg_msg)
        .await
        .expect("Agy registration failed");

    match reg_resp {
        CoordinatorMessage::RegisterResponse {
            status,
            nats_subject,
            ..
        } => {
            assert_eq!(status, "ok");
            assert_eq!(nats_subject, Some(format!("agents.{agent_id}.events")));
        }
        _ => panic!("Expected RegisterResponse"),
    }

    let agent_record = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .expect("Agent record must exist");

    assert_eq!(agent_record.human_owner, "AgyLead");
    assert_eq!(agent_record.adapter_type, AdapterType::Agy);
}

#[tokio::test]
async fn test_agy_task_lifecycle_execution() {
    let Some((pool, client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    // 1. Prepare simulated agy CLI script that streams NDJSON
    let script = TempScript::new(
        "#!/bin/bash\n\
         echo '{\"event\":\"init\",\"conversation_id\":\"conv-phase8-001\"}'\n\
         sleep 0.05\n\
         echo '{\"event\":\"step_update\",\"step_update\":{\"step_index\":1,\"text_delta\":\"Generating code\"}}'\n\
         sleep 0.05\n\
         echo '{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":\"Task finished clean\"}}'\n\
         exit 0",
    );

    let agent_id = Uuid::new_v4();
    let agy_agent = AgyAgent::new("AgyRunnerDev", "key_agy_test")
        .with_id(agent_id)
        .with_agy_path(script.path.clone())
        .with_timeout(Duration::from_secs(5));

    // Register agent
    let reg_msg = AgentMessage::Register {
        agent_id: agy_agent.id,
        human_owner: agy_agent.human_owner.clone(),
        adapter_type: "Agy".to_string(),
        capabilities: agy_agent.capabilities.clone(),
        api_key: agy_agent.api_key.clone(),
    };
    RegistrationHandler::process_registration(&pool, reg_msg).await.unwrap();

    // 2. Setup DB project, proposal, task
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 8 Agy Proj".to_string(),
            description: "Testing agy integration".to_string(),
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
            raw_prompt: "phase 8".to_string(),
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
            short_id: "AGY-001".to_string(),
            title: "Build agy integration adapter".to_string(),
            description: "Implement child process and protocol bridging".to_string(),
            affected_resources: vec!["crates/agent-agy/src/runner.rs".to_string()],
            estimated_size: Some("M".to_string()),
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

    let idempotency_key = format!("test-agy-{}", Uuid::new_v4());
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

    // 3. Coordinator publishes task assignment to NATS JetStream
    let spec = TaskSpec {
        task_id: task.id,
        short_id: task.short_id.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        affected_resources: task.resources(),
        depends_on: vec![],
        idempotency_key: idempotency_key.clone(),
        assigned_at: Utc::now(),
    };

    TaskPublisher::publish_assignment(&jetstream, agent_id, &spec)
        .await
        .expect("Failed to publish task assignment");

    // 4. Start EventSubscriber in background to process agent events into DB
    let sub_pool = pool.clone();
    let events_consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();
    let subscriber_handle = tokio::spawn(async move {
        let mut messages = events_consumer.messages().await.unwrap();
        while let Some(Ok(msg)) = messages.next().await {
            let _ = msg.ack().await;
            if let Ok(agent_msg) = serde_json::from_slice::<AgentMessage>(&msg.payload) {
                let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg).await;
            }
        }
    });

    // 5. agy agent consumer receives assignment and executes task
    let task_consumer = AgyAgentRunner::create_task_consumer(&jetstream, agent_id)
        .await
        .unwrap();

    let mut task_messages = task_consumer.messages().await.unwrap();
    let assignment_msg = task_messages.next().await.unwrap().unwrap();
    assignment_msg.ack().await.unwrap();

    let coord_msg: CoordinatorMessage = serde_json::from_slice(&assignment_msg.payload).unwrap();
    let received_spec = match coord_msg {
        CoordinatorMessage::TaskAssignment { spec } => spec,
        other => panic!("Expected TaskAssignment, got {other:?}"),
    };

    let event_subject = format!("agents.{agent_id}.events");
    AgyAgentRunner::execute_task(&client, &agy_agent, &received_spec, &event_subject)
        .await
        .expect("agy execution failed");

    // Allow background subscriber to digest events
    tokio::time::sleep(Duration::from_millis(150)).await;
    subscriber_handle.abort();

    // 6. Verify state in PostgreSQL
    let final_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(
        final_task.status,
        TaskStatus::Completed,
        "Task must transition to Completed after agy finishes"
    );

    let delivery = TaskDeliveryRepository::find_by_idempotency_key(&pool, &idempotency_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.status, DeliveryStatus::Acknowledged);

    let events = AgentEventRepository::list_by_task(&pool, task.id).await.unwrap();
    assert!(
        events.iter().any(|e| e.event_type == coordinator::domain::AgentEventType::TaskStarted),
        "Must record TaskStarted event in DB"
    );
    assert!(
        events.iter().any(|e| e.event_type == coordinator::domain::AgentEventType::Completed),
        "Must record Completed event in DB"
    );
}

#[tokio::test]
async fn test_agy_task_failure_lifecycle() {
    let Some((pool, client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    // Simulated failing script
    let script = TempScript::new(
        "#!/bin/bash\n\
         echo '{\"event\":\"init\",\"conversation_id\":\"conv-fail-001\"}'\n\
         sleep 0.05\n\
         >&2 echo 'Compile error in generated code'\n\
         exit 1",
    );

    let agent_id = Uuid::new_v4();
    let agy_agent = AgyAgent::new("AgyFailingDev", "key_fail_test")
        .with_id(agent_id)
        .with_agy_path(script.path.clone())
        .with_timeout(Duration::from_secs(5));

    // Register agent
    let reg_msg = AgentMessage::Register {
        agent_id: agy_agent.id,
        human_owner: agy_agent.human_owner.clone(),
        adapter_type: "Agy".to_string(),
        capabilities: agy_agent.capabilities.clone(),
        api_key: agy_agent.api_key.clone(),
    };
    RegistrationHandler::process_registration(&pool, reg_msg).await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 8 Fail Proj".to_string(),
            description: "Testing failure propagation".to_string(),
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
            raw_prompt: "phase 8 fail".to_string(),
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
            short_id: "AGY-FAIL-001".to_string(),
            title: "Task that triggers failure".to_string(),
            description: "Verify failure event propagation".to_string(),
            affected_resources: vec!["crates/agent-agy/src/runner.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    TaskRepository::update_status(&pool, task.id, TaskStatus::HumanReview).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent_id)).await.unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Assigned).await.unwrap();

    let idempotency_key = format!("test-fail-{}", Uuid::new_v4());
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

    let spec = TaskSpec {
        task_id: task.id,
        short_id: task.short_id.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        affected_resources: task.resources(),
        depends_on: vec![],
        idempotency_key: idempotency_key.clone(),
        assigned_at: Utc::now(),
    };

    TaskPublisher::publish_assignment(&jetstream, agent_id, &spec)
        .await
        .expect("Failed to publish assignment");

    let sub_pool = pool.clone();
    let events_consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();
    let subscriber_handle = tokio::spawn(async move {
        let mut messages = events_consumer.messages().await.unwrap();
        while let Some(Ok(msg)) = messages.next().await {
            let _ = msg.ack().await;
            if let Ok(agent_msg) = serde_json::from_slice::<AgentMessage>(&msg.payload) {
                let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg).await;
            }
        }
    });

    let task_consumer = AgyAgentRunner::create_task_consumer(&jetstream, agent_id)
        .await
        .unwrap();

    let mut task_messages = task_consumer.messages().await.unwrap();
    let assignment_msg = task_messages.next().await.unwrap().unwrap();
    assignment_msg.ack().await.unwrap();

    let coord_msg: CoordinatorMessage = serde_json::from_slice(&assignment_msg.payload).unwrap();
    let received_spec = match coord_msg {
        CoordinatorMessage::TaskAssignment { spec } => spec,
        other => panic!("Expected TaskAssignment, got {other:?}"),
    };

    let event_subject = format!("agents.{agent_id}.events");
    let _ = AgyAgentRunner::execute_task(&client, &agy_agent, &received_spec, &event_subject).await;

    tokio::time::sleep(Duration::from_millis(150)).await;
    subscriber_handle.abort();

    let final_task = TaskRepository::find_by_id(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(
        final_task.status,
        TaskStatus::Failed,
        "Task must transition to Failed after agy process error"
    );

    let events = AgentEventRepository::list_by_task(&pool, task.id).await.unwrap();
    assert!(
        events.iter().any(|e| e.event_type == coordinator::domain::AgentEventType::Failed),
        "Must record Failed event in DB"
    );
}

