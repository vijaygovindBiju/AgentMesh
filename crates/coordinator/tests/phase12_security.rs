use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use agent_protocol::security::{AgentRole, PermissionBoundary};
use agent_protocol::{AgentMessage, CoordinatorMessage};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, AuditRepository, ProjectRepository, ProposalRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, NewAgent, NewProject, NewProposal, NewTask,
};
use coordinator::messaging::{connect, ensure_streams, RegistrationHandler};
use coordinator::security::audit::AuditLogger;
use coordinator::security::auth::ApiKeyManager;
use coordinator::security::nats_security::NatsSubjectAuthorizer;
use coordinator::security::permissions::PermissionEnforcer;
use coordinator::security::secrets::{SecretRedactor, SecretScoper};
use coordinator::security::task_auth::TaskAuthorizer;
use coordinator::security::SecurityError;

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

#[tokio::test]
async fn test_phase12_1_agent_authentication_and_api_key_hashing() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let (raw_key, key_hash) = ApiKeyManager::generate_key();
    assert!(raw_key.starts_with("am_ak_"));
    assert_eq!(key_hash.len(), 64);

    // Register agent with raw key presented over protocol
    let reg_msg = AgentMessage::Register {
        agent_id,
        human_owner: "AliceSecurity".to_string(),
        adapter_type: "Mock".to_string(),
        capabilities: vec!["rust".to_string()],
        profile: None,
        api_key: raw_key.clone(),
    };

    let resp = RegistrationHandler::process_registration(&pool, reg_msg)
        .await
        .expect("Registration should succeed");

    match resp {
        CoordinatorMessage::RegisterResponse { status, .. } => assert_eq!(status, "ok"),
        other => panic!("Expected RegisterResponse, got {:?}", other),
    }

    // Verify stored agent in DB has SHA-256 hashed key, never raw key
    let agent = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert_eq!(agent.api_key_hash, key_hash);
    assert_ne!(agent.api_key_hash, raw_key);
    assert!(ApiKeyManager::verify_key(&raw_key, &agent.api_key_hash));

    // Re-registration with invalid key fails and is audited
    let bad_reg_msg = AgentMessage::Register {
        agent_id,
        human_owner: "AliceSecurity".to_string(),
        adapter_type: "Mock".to_string(),
        capabilities: vec!["rust".to_string()],
        profile: None,
        api_key: "am_ak_invalid_tampered_key".to_string(),
    };

    let bad_resp = RegistrationHandler::process_registration(&pool, bad_reg_msg)
        .await
        .expect("Handler returns error response");

    match bad_resp {
        CoordinatorMessage::RegisterResponse { status, error, .. } => {
            assert_eq!(status, "error");
            assert_eq!(error, Some("Invalid API key".to_string()));
        }
        other => panic!("Expected RegisterResponse error, got {:?}", other),
    }

    // Clean up
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}

#[tokio::test]
async fn test_phase12_2_agent_authorization_and_roles() {
    let worker_id = Uuid::new_v4();
    let reviewer_id = Uuid::new_v4();
    let readonly_id = Uuid::new_v4();

    let standard_boundary = PermissionBoundary::new();

    // 1. Worker can be assigned write tasks
    let res_worker = PermissionEnforcer::validate_task_assignment(
        worker_id,
        AgentRole::Worker,
        &standard_boundary,
        false,
        &["src/main.rs".to_string()],
    );
    assert!(res_worker.is_ok());

    // 2. ReadOnly agent is rejected for tasks with affected files
    let res_readonly = PermissionEnforcer::validate_task_assignment(
        readonly_id,
        AgentRole::ReadOnly,
        &standard_boundary,
        false,
        &["src/main.rs".to_string()],
    );
    assert!(matches!(res_readonly, Err(SecurityError::UnauthorizedAction { .. })));

    // 3. Reviewer without code modification rights is rejected for write tasks
    let reviewer_boundary = PermissionBoundary::new().with_code_modification(false);
    let res_reviewer = PermissionEnforcer::validate_task_assignment(
        reviewer_id,
        AgentRole::Reviewer,
        &reviewer_boundary,
        false,
        &["src/lib.rs".to_string()],
    );
    assert!(matches!(res_reviewer, Err(SecurityError::UnauthorizedAction { .. })));
}

#[tokio::test]
async fn test_phase12_3_agent_revocation_and_expiration() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let (raw_key, _) = ApiKeyManager::generate_key();

    let reg_msg = AgentMessage::Register {
        agent_id,
        human_owner: "RevokeTester".to_string(),
        adapter_type: "Mock".to_string(),
        capabilities: vec!["rust".to_string()],
        profile: None,
        api_key: raw_key.clone(),
    };
    RegistrationHandler::process_registration(&pool, reg_msg).await.unwrap();

    // Update agent to Idle
    AgentRepository::update_status(&pool, agent_id, AgentStatus::Idle).await.unwrap();
    let available = AgentRepository::list_available(&pool).await.unwrap();
    assert!(available.iter().any(|a| a.id == agent_id));

    // Revoke agent
    AgentRepository::revoke_agent(&pool, agent_id).await.unwrap();
    let revoked = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert!(revoked.is_revoked);
    assert!(!revoked.is_available());

    // Revoked agent must NOT appear in available list
    let available_after_revoke = AgentRepository::list_available(&pool).await.unwrap();
    assert!(!available_after_revoke.iter().any(|a| a.id == agent_id));

    // Attempting registration with revoked agent returns error
    let reg_revoked = AgentMessage::Register {
        agent_id,
        human_owner: "RevokeTester".to_string(),
        adapter_type: "Mock".to_string(),
        capabilities: vec!["rust".to_string()],
        profile: None,
        api_key: raw_key.clone(),
    };
    let resp = RegistrationHandler::process_registration(&pool, reg_revoked).await.unwrap();
    match resp {
        CoordinatorMessage::RegisterResponse { status, error, .. } => {
            assert_eq!(status, "error");
            assert_eq!(error, Some("Agent key is revoked".to_string()));
        }
        other => panic!("Expected error response, got {:?}", other),
    }

    // Un-revoke and verify it can be restored
    AgentRepository::unrevoke_agent(&pool, agent_id).await.unwrap();
    let unrevoked = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert!(!unrevoked.is_revoked);

    // Rotate API key
    let (new_key, new_hash) = ApiKeyManager::generate_key();
    AgentRepository::rotate_api_key(&pool, agent_id, &new_hash).await.unwrap();
    let rotated = AgentRepository::find_by_id(&pool, agent_id).await.unwrap().unwrap();
    assert_eq!(rotated.api_key_hash, new_hash);
    assert!(ApiKeyManager::verify_key(&new_key, &rotated.api_key_hash));

    // Clean up
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}

#[tokio::test]
async fn test_phase12_4_permission_boundaries_and_denied_paths() {
    let agent_id = Uuid::new_v4();

    // Boundary restricted to frontend code, forbidden secrets
    let boundary = PermissionBoundary::new()
        .with_allowed_path("frontend/*")
        .with_denied_path("frontend/.env*")
        .with_denied_path("*credentials*");

    // 1. Allowed frontend source path
    let ok_res = PermissionEnforcer::validate_task_assignment(
        agent_id,
        AgentRole::Worker,
        &boundary,
        false,
        &["frontend/src/App.tsx".to_string()],
    );
    assert!(ok_res.is_ok());

    // 2. Denied secret path inside allowed tree
    let denied_secret = PermissionEnforcer::validate_task_assignment(
        agent_id,
        AgentRole::Worker,
        &boundary,
        false,
        &["frontend/.env.production".to_string()],
    );
    match denied_secret {
        Err(SecurityError::PermissionBoundaryViolation { paths, .. }) => {
            assert_eq!(paths, vec!["frontend/.env.production"]);
        }
        other => panic!("Expected PermissionBoundaryViolation, got {:?}", other),
    }

    // 3. Path outside allowed boundary (e.g. backend source)
    let outside_boundary = PermissionEnforcer::validate_task_assignment(
        agent_id,
        AgentRole::Worker,
        &boundary,
        false,
        &["backend/src/main.rs".to_string()],
    );
    assert!(matches!(outside_boundary, Err(SecurityError::PermissionBoundaryViolation { .. })));
}

#[tokio::test]
async fn test_phase12_5_task_impersonation_prevention() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    // Create project, proposal, and two agents
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Security Impersonation Test".to_string(),
            description: "Test task impersonation prevention".to_string(),
        },
    )
    .await
    .unwrap();

    let proposal = ProposalRepository::create(
        &pool,
        &NewProposal {
            project_id: project.id,
            ai_provider: "anthropic".to_string(),
            ai_model: "claude-3-5-sonnet-20241022".to_string(),
            raw_prompt: "prompt".to_string(),
            raw_response: "response".to_string(),
        },
    )
    .await
    .unwrap();

    let agent_a_id = Uuid::new_v4();
    let agent_b_id = Uuid::new_v4();

    AgentRepository::create_with_id(
        &pool,
        agent_a_id,
        &NewAgent {
            human_owner: "AgentA".to_string(),
            api_key_hash: "hash_a".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_a_id}.events"),
            profile: None,
            max_concurrency: Some(1),
            role: Some("worker".to_string()),
            permissions: None,
            api_key_expires_at: None,
        },
    )
    .await
    .unwrap();

    AgentRepository::create_with_id(
        &pool,
        agent_b_id,
        &NewAgent {
            human_owner: "AgentB".to_string(),
            api_key_hash: "hash_b".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_b_id}.events"),
            profile: None,
            max_concurrency: Some(1),
            role: Some("worker".to_string()),
            permissions: None,
            api_key_expires_at: None,
        },
    )
    .await
    .unwrap();

    // Create task assigned to Agent A
    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            proposal_id: proposal.id,
            short_id: "SEC-1".to_string(),
            title: "Task assigned to Agent A".to_string(),
            description: "Only Agent A may execute".to_string(),
            affected_resources: vec!["src/lib.rs".to_string()],
            estimated_size: Some("S".to_string()),
        },
    )
    .await
    .unwrap();

    TaskRepository::assign_agent(&pool, task.id, Some(agent_a_id)).await.unwrap();

    // 1. Agent A is authorized to act on this task
    let auth_ok = TaskAuthorizer::authorize_agent_for_task(&pool, agent_a_id, task.id, "task_started").await;
    assert!(auth_ok.is_ok());

    // 2. Agent B attempts to act on Agent A's task -> Rejected with TaskImpersonation
    let auth_rogue = TaskAuthorizer::authorize_agent_for_task(&pool, agent_b_id, task.id, "task_completed").await;
    match auth_rogue {
        Err(SecurityError::TaskImpersonation { actor_agent_id, task_id, assigned_to, .. }) => {
            assert_eq!(actor_agent_id, agent_b_id);
            assert_eq!(task_id, task.id);
            assert_eq!(assigned_to, Some(agent_a_id));
        }
        other => panic!("Expected TaskImpersonation error, got {:?}", other),
    }

    // Clean up
    AgentRepository::delete(&pool, agent_a_id).await.unwrap();
    AgentRepository::delete(&pool, agent_b_id).await.unwrap();
    ProjectRepository::delete(&pool, project.id).await.unwrap();
}

#[tokio::test]
async fn test_phase12_6_secret_isolation_and_redaction() {
    // 1. Test secret redactor on sensitive text
    let log_message = "Agent reported key am_ak_abcdef0123456789abcdef0123456789 with postgres://user:super_pass@db.internal:5432/main";
    let redacted = SecretRedactor::redact(log_message);
    assert!(!redacted.contains("am_ak_abcdef0123456789abcdef0123456789"));
    assert!(!redacted.contains("super_pass"));
    assert!(redacted.contains("[REDACTED_API_KEY]"));
    assert!(redacted.contains("[REDACTED]"));

    // 2. Test JSON payload redaction
    let payload = serde_json::json!({
        "username": "alice",
        "api_key": "am_ak_112233445566778899aabbccddeeff00",
        "password": "my_secret_password",
        "nested": {
            "token": "bearer_secret_tok",
            "message": "Public status message"
        }
    });

    let sanitized_json = SecretRedactor::redact_json(&payload);
    assert_eq!(sanitized_json["api_key"], "[REDACTED]");
    assert_eq!(sanitized_json["password"], "[REDACTED]");
    assert_eq!(sanitized_json["nested"]["token"], "[REDACTED]");
    assert_eq!(sanitized_json["nested"]["message"], "Public status message");

    // 3. Test environment isolation
    let mut coordinator_env = HashMap::new();
    coordinator_env.insert("PATH".to_string(), "/usr/local/bin:/usr/bin".to_string());
    coordinator_env.insert("DATABASE_URL".to_string(), "postgres://agentmesh:pass@localhost:5432".to_string());
    coordinator_env.insert("POSTGRES_PASSWORD".to_string(), "master_key".to_string());
    coordinator_env.insert("NATS_ADMIN_TOKEN".to_string(), "nats_secret".to_string());

    let mut task_env = HashMap::new();
    task_env.insert("TASK_ID".to_string(), "task_xyz".to_string());

    let isolated = SecretScoper::sanitize_env_for_agent(&coordinator_env, &task_env);
    assert!(isolated.contains_key("PATH"));
    assert!(isolated.contains_key("TASK_ID"));
    assert!(!isolated.contains_key("DATABASE_URL"));
    assert!(!isolated.contains_key("POSTGRES_PASSWORD"));
    assert!(!isolated.contains_key("NATS_ADMIN_TOKEN"));
}

#[tokio::test]
async fn test_phase12_7_nats_subject_authorization() {
    let agent_id = Uuid::new_v4();

    // 1. Authorized publish
    let pub_subj = format!("agents.{agent_id}.events");
    assert!(NatsSubjectAuthorizer::validate_agent_publish(agent_id, &pub_subj).is_ok());

    // 2. Unauthorized publish to another agent
    let rogue_pub = "agents.b472855a-0f99-4843-8c10-db7e973ac05e.events";
    assert!(NatsSubjectAuthorizer::validate_agent_publish(agent_id, rogue_pub).is_err());

    // 3. Authorized subscribe
    let sub_subj = format!("agents.{agent_id}.tasks");
    assert!(NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, &sub_subj).is_ok());

    // 4. Wildcard subscribe attempts strictly denied
    assert!(NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, "agents.>").is_err());
    assert!(NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, "coordinator.tasks.*").is_err());
}

#[tokio::test]
async fn test_phase12_8_persistent_audit_logging() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();

    // 1. Log an authentication failure
    AuditLogger::log_auth_failure(&pool, agent_id, "Invalid key presented").await.unwrap();

    // 2. Log a task impersonation attempt
    let task_id = Uuid::new_v4();
    AuditLogger::log_task_impersonation(&pool, agent_id, task_id, Some(Uuid::new_v4()), "task_completed")
        .await
        .unwrap();

    // 3. Log a permission boundary violation
    AuditLogger::log_permission_denied(&pool, agent_id, "modify_file", &[".env.production".to_string()])
        .await
        .unwrap();

    // 4. Query audit repository
    let recent = AuditRepository::find_recent(&pool, 10).await.unwrap();
    assert!(recent.iter().any(|e| e.action == "auth_failure"));
    assert!(recent.iter().any(|e| e.action == "task_impersonation_blocked"));
    assert!(recent.iter().any(|e| e.action == "permission_denied"));

    // 5. Query alerts
    let alerts = AuditRepository::find_alerts(&pool, 10).await.unwrap();
    assert!(alerts.iter().all(|e| e.status == "denied" || e.status == "failure"));
    assert!(alerts.iter().any(|e| e.actor_id == Some(agent_id.to_string())));

    // 6. Query by actor
    let agent_logs = AuditRepository::find_by_actor(&pool, "agent", &agent_id.to_string()).await.unwrap();
    assert_eq!(agent_logs.len(), 3);
}
