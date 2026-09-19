use sqlx::PgPool;
use uuid::Uuid;

use agent_protocol::{
    AgentCapabilities, AgentMessage, CapabilityDetector, CoordinatorMessage, HealthStatus,
    LanguageCapability, RuntimeCapability, TaskRequirements, ToolCapability,
};
use coordinator::ai::matcher::{AgentCapabilityMatcher, CandidateAgent};
use coordinator::coordinator::AssignmentService;
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, ProjectRepository, ProposalRepository, TaskRepository,
};
use coordinator::domain::{
    AdapterType, AgentStatus, NewAgent, NewProject, NewProposal, NewTask, TaskStatus,
};
use coordinator::messaging::{connect, ensure_streams, RegistrationHandler};

async fn setup_test_env() -> Option<(PgPool, async_nats::Client, async_nats::jetstream::Context)> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
    });
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;

    let (client, jetstream) = connect(&nats_url, None).await.ok()?;
    ensure_streams(&jetstream).await.ok()?;

    Some((pool, client, jetstream))
}

#[tokio::test]
async fn test_phase11_1_capability_model_structure_and_serde() {
    let runtime = RuntimeCapability {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        adapter_type: "Agy".to_string(),
        agy_version: Some("0.4.1".to_string()),
        cpu_count: 16,
        memory_mb: Some(32768),
    };

    let profile = AgentCapabilities::new(runtime)
        .with_language(LanguageCapability::new(
            "rust",
            Some("1.80.0".to_string()),
            vec!["tokio".to_string(), "axum".to_string()],
        ))
        .with_language(LanguageCapability::new(
            "python",
            Some("3.12.1".to_string()),
            vec!["fastapi".to_string()],
        ))
        .with_tool(ToolCapability::new(
            "cargo",
            Some("1.80.0".to_string()),
            Some("/usr/bin/cargo".to_string()),
        ))
        .with_tool(ToolCapability::new("git", Some("2.43.0".to_string()), None))
        .with_tag("backend")
        .with_tag("distributed");

    assert!(profile.has_language("rust"));
    assert!(profile.has_language("python"));
    assert!(profile.has_framework("tokio"));
    assert!(profile.has_framework("fastapi"));
    assert!(profile.has_tool("cargo"));
    assert!(profile.has_tool("git"));
    assert!(profile.has_tag("backend"));
    assert!(profile.has_tag("distributed"));
    assert!(!profile.has_language("dart"));

    let tags = profile.all_tags();
    assert!(tags.contains(&"rust".to_string()));
    assert!(tags.contains(&"tokio".to_string()));
    assert!(tags.contains(&"backend".to_string()));
    assert!(tags.contains(&"linux".to_string()));

    let json = serde_json::to_string_pretty(&profile).expect("serialize");
    let recovered: AgentCapabilities = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(profile, recovered);
}

#[tokio::test]
async fn test_phase11_2_detect_runtime_languages_tools() {
    let runtime = CapabilityDetector::detect_runtime("Agy");
    assert!(!runtime.os.is_empty());
    assert!(!runtime.arch.is_empty());
    assert_eq!(runtime.adapter_type, "Agy");
    assert!(runtime.cpu_count >= 1);

    let languages = CapabilityDetector::detect_languages();
    // System running tests has rust installed
    assert!(languages.iter().any(|l| l.name == "rust"));

    let tools = CapabilityDetector::detect_tools();
    // Host has git and cargo installed
    assert!(tools.iter().any(|t| t.name == "git" || t.name == "cargo"));

    let profile = CapabilityDetector::detect_all("Agy", &["custom_worker".to_string()]);
    assert!(profile.has_tag("custom_worker"));
    assert!(
        profile.has_tag("backend") || profile.has_tag("systems") || profile.has_language("rust")
    );
}

#[tokio::test]
async fn test_phase11_3_register_capabilities_via_protocol() {
    let Some((pool, _client, _jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let agent_id = Uuid::new_v4();
    let profile = AgentCapabilities::new(RuntimeCapability {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        adapter_type: "Agy".to_string(),
        agy_version: Some("0.4.0".to_string()),
        cpu_count: 8,
        memory_mb: Some(16384),
    })
    .with_language(LanguageCapability::new(
        "rust",
        Some("1.79.0".to_string()),
        vec!["tokio".to_string()],
    ))
    .with_tool(ToolCapability::new("cargo", None, None))
    .with_tag("backend");

    let reg_msg = AgentMessage::Register {
        agent_id,
        human_owner: "CapabilityTester".to_string(),
        adapter_type: "Agy".to_string(),
        capabilities: profile.all_tags(),
        profile: Some(profile.clone()),
        api_key: "cap_key_123".to_string(),
    };

    let resp = RegistrationHandler::process_registration(&pool, reg_msg)
        .await
        .expect("Registration must succeed");

    match resp {
        CoordinatorMessage::RegisterResponse {
            status,
            nats_subject,
            ..
        } => {
            assert_eq!(status, "ok");
            assert_eq!(nats_subject, Some(format!("agents.{agent_id}.events")));
        }
        other => panic!("Expected RegisterResponse, got: {:?}", other),
    }

    let agent = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .expect("Agent record must exist in DB");

    assert_eq!(agent.human_owner, "CapabilityTester");
    assert_eq!(agent.adapter_type, AdapterType::Agy);
    assert_eq!(
        agent.health_status,
        coordinator::domain::HealthStatus::Healthy
    );

    let stored_profile = agent
        .capabilities_profile()
        .expect("Profile must be parsed from JSONB");
    assert_eq!(stored_profile.runtime.os, "linux");
    assert!(stored_profile.has_language("rust"));
    assert!(stored_profile.has_tool("cargo"));
    assert!(agent.capabilities_list().contains(&"rust".to_string()));

    // Clean up
    AgentRepository::delete(&pool, agent_id).await.unwrap();
}

#[tokio::test]
async fn test_phase11_4_spec_example_capability_matching() {
    // Spec example from TODO.md:
    // Agent A: [rust, linux, backend]
    // Agent B: [flutter, dart, frontend]
    let agent_a_id = Uuid::new_v4();
    let agent_b_id = Uuid::new_v4();

    let profile_a = AgentCapabilities::new(RuntimeCapability {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        adapter_type: "Agy".to_string(),
        agy_version: Some("0.4.0".to_string()),
        cpu_count: 8,
        memory_mb: None,
    })
    .with_language(LanguageCapability::new(
        "rust",
        Some("1.80.0".to_string()),
        vec![],
    ))
    .with_tool(ToolCapability::new("cargo", None, None))
    .with_tag("backend");

    let profile_b = AgentCapabilities::new(RuntimeCapability {
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        adapter_type: "Agy".to_string(),
        agy_version: Some("0.4.0".to_string()),
        cpu_count: 8,
        memory_mb: None,
    })
    .with_language(LanguageCapability::new(
        "dart",
        Some("3.4.0".to_string()),
        vec!["flutter".to_string()],
    ))
    .with_tag("frontend");

    let candidates = vec![
        CandidateAgent {
            agent_id: agent_a_id,
            human_owner: "Agent A".to_string(),
            capabilities: profile_a.all_tags(),
            profile: Some(profile_a),
            is_available: true,
            health_status: HealthStatus::Healthy,
        },
        CandidateAgent {
            agent_id: agent_b_id,
            human_owner: "Agent B".to_string(),
            capabilities: profile_b.all_tags(),
            profile: Some(profile_b),
            is_available: true,
            health_status: HealthStatus::Healthy,
        },
    ];

    // 1. Rust backend task -> Assigns to Agent A
    let rust_reqs = TaskRequirements {
        required_languages: vec!["rust".to_string()],
        required_tools: vec!["cargo".to_string()],
        required_os: Some("linux".to_string()),
        required_tags: vec!["backend".to_string()],
        preferred_tags: vec![],
    };
    let rust_ranking = AgentCapabilityMatcher::rank_candidates(&candidates, &rust_reqs);
    assert_eq!(rust_ranking[0].agent_id, agent_a_id);
    assert!(rust_ranking[0].is_eligible);
    assert!(rust_ranking[0].score > 50);
    assert!(!rust_ranking[1].is_eligible, "Agent B lacks rust/cargo");

    // 2. Flutter frontend task -> Assigns to Agent B
    let flutter_reqs = TaskRequirements {
        required_languages: vec!["dart".to_string()],
        required_tools: vec![],
        required_os: None,
        required_tags: vec!["frontend".to_string()],
        preferred_tags: vec!["flutter".to_string()],
    };
    let flutter_ranking = AgentCapabilityMatcher::rank_candidates(&candidates, &flutter_reqs);
    assert_eq!(flutter_ranking[0].agent_id, agent_b_id);
    assert!(flutter_ranking[0].is_eligible);
    assert!(flutter_ranking[0].score > 50);
    assert!(
        !flutter_ranking[1].is_eligible,
        "Agent A lacks dart/flutter"
    );
}

#[tokio::test]
async fn test_phase11_5_availability_state_and_concurrency_gating() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Availability Mesh Proj".to_string(),
            description: "Testing availability and draining state".to_string(),
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

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "DrainTester".to_string(),
            api_key_hash: "hash".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            max_concurrency: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle)
        .await
        .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "DRAIN-01".to_string(),
            title: "Task while draining".to_string(),
            description: "Should not be assigned if agent is draining".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id))
        .await
        .unwrap();

    // 1. Set agent to draining
    AgentRepository::set_draining(&pool, agent.id, true)
        .await
        .unwrap();
    let available = AgentRepository::list_available(&pool).await.unwrap();
    assert!(
        !available.iter().any(|a| a.id == agent.id),
        "Draining agent must not be listed as available"
    );

    // Coordinator assignment attempt: draining agent must not be claimed
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();
    assert!(
        assignments.is_empty(),
        "Task must not be assigned to draining agent"
    );

    // 2. Disable draining -> now agent is eligible and assigned
    AgentRepository::set_draining(&pool, agent.id, false)
        .await
        .unwrap();
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();
    assert_eq!(
        assignments.len(),
        1,
        "Agent should now be assigned the approved task"
    );
    assert_eq!(assignments[0].agent_id, agent.id);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}

#[tokio::test]
async fn test_phase11_6_agent_health_metrics_and_unhealthy_gating() {
    let Some((pool, _client, jetstream)) = setup_test_env().await else {
        eprintln!("Skipping test: services not reachable");
        return;
    };

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Health Testing Proj".to_string(),
            description: "Testing health diagnostics and gating".to_string(),
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

    let agent_id = Uuid::new_v4();
    let agent = AgentRepository::create_with_id(
        &pool,
        agent_id,
        &NewAgent {
            human_owner: "HealthTester".to_string(),
            api_key_hash: "hash_h".to_string(),
            adapter_type: AdapterType::Mock,
            capabilities: vec!["rust".to_string()],
            nats_subject: format!("agents.{agent_id}.events"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    AgentRepository::update_status(&pool, agent.id, AgentStatus::Idle)
        .await
        .unwrap();

    // Verify initial health is Healthy
    let agent_init = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        agent_init.health_status,
        coordinator::domain::HealthStatus::Healthy
    );
    assert_eq!(agent_init.consecutive_failures, 0);

    // Record repeated task failures: 1 failure
    AgentRepository::record_task_failure(&pool, agent_id, "Compiler error")
        .await
        .unwrap();
    let agent_f1 = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agent_f1.consecutive_failures, 1);
    assert_eq!(
        agent_f1.health_status,
        coordinator::domain::HealthStatus::Healthy
    );

    // 2 consecutive failures -> Degraded
    AgentRepository::record_task_failure(&pool, agent_id, "Timeout error")
        .await
        .unwrap();
    let agent_f2 = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agent_f2.consecutive_failures, 2);
    assert_eq!(
        agent_f2.health_status,
        coordinator::domain::HealthStatus::Degraded
    );

    // 5 consecutive failures -> Unhealthy
    for _ in 0..3 {
        AgentRepository::record_task_failure(&pool, agent_id, "Critical failure")
            .await
            .unwrap();
    }
    let agent_f5 = AgentRepository::find_by_id(&pool, agent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agent_f5.consecutive_failures, 5);
    assert_eq!(
        agent_f5.health_status,
        coordinator::domain::HealthStatus::Unhealthy
    );
    assert!(
        !agent_f5.is_available(),
        "Unhealthy agent must not be available"
    );

    // Create an approved task targeted to this agent
    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "HLTH-01".to_string(),
            title: "Task for unhealthy agent".to_string(),
            description: "Must not assign to unhealthy agent".to_string(),
            affected_resources: vec![],
            estimated_size: None,
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();
    TaskRepository::update_status(&pool, task.id, TaskStatus::Approved)
        .await
        .unwrap();
    TaskRepository::assign_agent(&pool, task.id, Some(agent.id))
        .await
        .unwrap();

    // Attempt assignment: unhealthy agent must NOT be assigned the task
    let assignments =
        AssignmentService::assign_ready_tasks(&pool, Some(project.id), Some(&jetstream))
            .await
            .unwrap();
    assert!(
        assignments.is_empty(),
        "Coordinator must not assign tasks to unhealthy agents"
    );

    // Success resets consecutive failures and restores Healthy status
    AgentRepository::record_task_completion(&pool, agent.id)
        .await
        .unwrap();
    let agent_restored = AgentRepository::find_by_id(&pool, agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        agent_restored.health_status,
        coordinator::domain::HealthStatus::Healthy
    );
    assert_eq!(agent_restored.consecutive_failures, 0);
    assert_eq!(agent_restored.tasks_completed_count, 1);

    // Clean up
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent.id).await.unwrap();
}
