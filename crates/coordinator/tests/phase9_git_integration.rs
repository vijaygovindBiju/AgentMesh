use std::path::PathBuf;
use chrono::Utc;
use futures::StreamExt;
use sqlx::PgPool;
use tokio::process::Command;
use uuid::Uuid;

use agent_protocol::{AgentMessage, TaskSpec};
use coordinator::db::pool::{create_pool, run_migrations};
use coordinator::db::repositories::{
    AgentRepository, GitConflictRepository, ProjectRepository, ProposalRepository, TaskRepository,
    UnexpectedResourceRepository,
};
use coordinator::domain::{AdapterType, NewAgent, NewProject, NewProposal, NewTask, TaskStatus};
use coordinator::git::{
    ConcurrencySafety, ConflictDetector, GitCoordinator, RepositoryIdentity,
};
use coordinator::messaging::{connect, ensure_streams, EventSubscriber, TaskPublisher};

async fn setup_test_pool() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let pool = create_pool(&db_url).await.ok()?;
    run_migrations(&pool).await.ok()?;
    Some(pool)
}

fn create_temp_repo_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}_{}", prefix, Uuid::new_v4()))
}

#[tokio::test]
async fn test_phase9_repository_identity_and_discovery() {
    let repo_dir = create_temp_repo_dir("agentmesh_p9_identity");
    let identity = RepositoryIdentity::init(&repo_dir, "main")
        .await
        .expect("Failed to initialize repository");

    assert_eq!(identity.base_branch, "main");
    assert_eq!(identity.head_commit_sha.len(), 40);
    assert!(identity.is_clean().await.expect("is_clean check failed"));

    // Discover repository identity from subfolder
    let subfolder = repo_dir.join("subdir");
    tokio::fs::create_dir_all(&subfolder).await.unwrap();
    let discovered = RepositoryIdentity::discover(&subfolder)
        .await
        .expect("Failed to discover repository identity");

    assert_eq!(discovered.repo_root, identity.repo_root);
    assert_eq!(discovered.base_branch, "main");
    assert_eq!(discovered.head_commit_sha, identity.head_commit_sha);

    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[tokio::test]
async fn test_phase9_parallel_agents_worktree_isolation_no_overwriting() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: PostgreSQL not available");
        return;
    };

    let git_coord = GitCoordinator::new(pool.clone());
    let repo_dir = create_temp_repo_dir("agentmesh_p9_parallel");
    let repo_id = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    // 1. Create project in DB
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 9 Parallel Project".to_string(),
            description: "Testing parallel agent git worktree isolation".to_string(),
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
            raw_prompt: "phase 9".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // 2. Create Task 1 (Backend) and Task 2 (Frontend)
    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-001".to_string(),
            title: "Implement Backend Module".to_string(),
            description: "Write src/backend.rs".to_string(),
            affected_resources: vec!["src/backend.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-002".to_string(),
            title: "Implement Frontend Module".to_string(),
            description: "Write src/frontend.rs".to_string(),
            affected_resources: vec!["src/frontend.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // 3. Prepare isolated workspaces for both agents simultaneously
    let ws_1 = git_coord
        .prepare_task_workspace(
            &repo_id.repo_root,
            project.id,
            task_1.id,
            &task_1.short_id,
            "main",
            None,
        )
        .await
        .expect("Workspace 1 creation must succeed");

    let ws_2 = git_coord
        .prepare_task_workspace(
            &repo_id.repo_root,
            project.id,
            task_2.id,
            &task_2.short_id,
            "main",
            None,
        )
        .await
        .expect("Workspace 2 creation must succeed");

    assert!(ws_1.is_valid());
    assert!(ws_2.is_valid());
    assert_ne!(ws_1.worktree_path, ws_2.worktree_path);
    assert_ne!(ws_1.task_branch, ws_2.task_branch);

    // Verify task rows in DB recorded git branch and base commit
    let ctx_1 = TaskRepository::find_git_context(&pool, task_1.id).await.unwrap().unwrap();
    assert_eq!(ctx_1.task_branch.as_deref(), Some("agentmesh/task-001"));
    assert_eq!(ctx_1.base_commit_sha.as_deref(), Some(repo_id.head_commit_sha.as_str()));

    let ctx_2 = TaskRepository::find_git_context(&pool, task_2.id).await.unwrap().unwrap();
    assert_eq!(ctx_2.task_branch.as_deref(), Some("agentmesh/task-002"));
    assert_eq!(ctx_2.base_commit_sha.as_deref(), Some(repo_id.head_commit_sha.as_str()));

    // 4. Simultaneous Parallel Agent Editing
    // Agent 1 creates src/backend.rs in Workspace 1
    tokio::fs::create_dir_all(ws_1.worktree_path.join("src")).await.unwrap();
    tokio::fs::write(
        ws_1.worktree_path.join("src/backend.rs"),
        "pub fn serve_backend() -> &'static str { \"ok\" }\n",
    )
    .await
    .unwrap();

    // Agent 2 creates src/frontend.rs in Workspace 2
    tokio::fs::create_dir_all(ws_2.worktree_path.join("src")).await.unwrap();
    tokio::fs::write(
        ws_2.worktree_path.join("src/frontend.rs"),
        "export const app = () => 'rendered';\n",
    )
    .await
    .unwrap();

    // 5. Verification of Zero Cross-Pollution:
    // Neither workspace sees the other's uncommitted work!
    assert!(!ws_1.worktree_path.join("src/frontend.rs").exists());
    assert!(!ws_2.worktree_path.join("src/backend.rs").exists());
    assert!(!repo_id.repo_root.join("src/backend.rs").exists());
    assert!(!repo_id.repo_root.join("src/frontend.rs").exists());

    // 6. Audit modified resources for both tasks
    let unexp_1 = git_coord
        .audit_task_resources(task_1.id, &ws_1.worktree_path, &ws_1.base_commit_sha, &["src/backend.rs".to_string()])
        .await
        .unwrap();
    assert!(unexp_1.is_empty(), "All changes in task 1 are expected");

    let unexp_2 = git_coord
        .audit_task_resources(task_2.id, &ws_2.worktree_path, &ws_2.base_commit_sha, &["src/frontend.rs".to_string()])
        .await
        .unwrap();
    assert!(unexp_2.is_empty(), "All changes in task 2 are expected");

    // 7. Finalize Task 1 and Task 2
    let fin_1 = git_coord
        .finalize_task(&ws_1, &task_1.short_id, &task_1.title, "main")
        .await
        .expect("Finalize task 1 must succeed");
    assert!(fin_1.can_merge_cleanly);
    assert!(fin_1.actual_modified_resources.contains(&"src/backend.rs".to_string()));

    let fin_2 = git_coord
        .finalize_task(&ws_2, &task_2.short_id, &task_2.title, "main")
        .await
        .expect("Finalize task 2 must succeed");
    assert!(fin_2.can_merge_cleanly);
    assert!(fin_2.actual_modified_resources.contains(&"src/frontend.rs".to_string()));

    // 8. Both worktrees are cleanly cleaned up
    assert!(!ws_1.worktree_path.exists());
    assert!(!ws_2.worktree_path.exists());

    // 9. Merge both task branches into main in repo root
    let merge_1 = Command::new("git")
        .args(["merge", "--no-ff", "-m", "Merge task 1", &fin_1.task_branch])
        .current_dir(&repo_id.repo_root)
        .output()
        .await
        .unwrap();
    assert!(merge_1.status.success(), "Merge of task 1 must succeed");

    let merge_2 = Command::new("git")
        .args(["merge", "--no-ff", "-m", "Merge task 2", &fin_2.task_branch])
        .current_dir(&repo_id.repo_root)
        .output()
        .await
        .unwrap();
    assert!(merge_2.status.success(), "Merge of task 2 must succeed without conflict");

    // 10. Verify both files exist simultaneously and intact on main!
    assert!(repo_id.repo_root.join("src/backend.rs").exists());
    assert!(repo_id.repo_root.join("src/frontend.rs").exists());

    // Clean up DB and filesystem
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[tokio::test]
async fn test_phase9_unexpected_resource_modification_detection() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: PostgreSQL not available");
        return;
    };

    let git_coord = GitCoordinator::new(pool.clone());
    let repo_dir = create_temp_repo_dir("agentmesh_p9_unexp");
    let repo_id = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Unexpected Resource Project".to_string(),
            description: "Testing unexpected resource detection".to_string(),
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
            raw_prompt: "phase 9 unexpected".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-003".to_string(),
            title: "Scoped Edit".to_string(),
            description: "Only modify src/auth.rs".to_string(),
            affected_resources: vec!["src/auth.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let ws = git_coord
        .prepare_task_workspace(
            &repo_id.repo_root,
            project.id,
            task.id,
            &task.short_id,
            "main",
            None,
        )
        .await
        .unwrap();

    // Agent modifies planned file AND unplanned secret file
    tokio::fs::create_dir_all(ws.worktree_path.join("src")).await.unwrap();
    tokio::fs::write(ws.worktree_path.join("src/auth.rs"), "// auth code").await.unwrap();
    tokio::fs::write(ws.worktree_path.join("credentials.env"), "SECRET=leak").await.unwrap();

    // Audit resources
    let unexpected = git_coord
        .audit_task_resources(
            task.id,
            &ws.worktree_path,
            &ws.base_commit_sha,
            &task.resources(),
        )
        .await
        .unwrap();

    assert_eq!(unexpected.len(), 1);
    assert_eq!(unexpected[0].resource_path, "credentials.env");

    // Verify stored in PostgreSQL unexpected_resource_changes table
    let records = UnexpectedResourceRepository::list_by_task(&pool, task.id).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].resource_path, "credentials.env");
    assert!(!records[0].acknowledged);

    // Human acknowledges warning
    UnexpectedResourceRepository::acknowledge(&pool, records[0].id).await.unwrap();
    let records_after = UnexpectedResourceRepository::list_by_task(&pool, task.id).await.unwrap();
    assert!(records_after[0].acknowledged);

    // Cleanup
    ws.cleanup().await.unwrap();
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[tokio::test]
async fn test_phase9_cross_agent_git_conflict_detection_and_recording() {
    let Some(pool) = setup_test_pool().await else {
        eprintln!("Skipping test: PostgreSQL not available");
        return;
    };

    let git_coord = GitCoordinator::new(pool.clone());
    let repo_dir = create_temp_repo_dir("agentmesh_p9_conflict");
    let repo_id = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    // Create a base file in main
    let shared_file = repo_id.repo_root.join("shared_config.rs");
    tokio::fs::write(&shared_file, "pub const TIMEOUT: u64 = 30;\n").await.unwrap();
    repo_id.commit_all("feat: add shared_config.rs").await.unwrap();

    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Conflict Project".to_string(),
            description: "Testing cross-agent conflict detection".to_string(),
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
            raw_prompt: "phase 9 conflict".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    let task_a = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-004".to_string(),
            title: "Edit shared config to 60".to_string(),
            description: "Change timeout".to_string(),
            affected_resources: vec!["shared_config.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task_b = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-005".to_string(),
            title: "Edit shared config to 120".to_string(),
            description: "Change timeout".to_string(),
            affected_resources: vec!["shared_config.rs".to_string()],
            estimated_size: Some("S".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // Create workspace A and modify shared_config.rs to 60
    let ws_a = git_coord
        .prepare_task_workspace(
            &repo_id.repo_root,
            project.id,
            task_a.id,
            &task_a.short_id,
            "main",
            None,
        )
        .await
        .unwrap();
    tokio::fs::write(ws_a.worktree_path.join("shared_config.rs"), "pub const TIMEOUT: u64 = 60;\n")
        .await
        .unwrap();
    let fin_a = git_coord
        .finalize_task(&ws_a, &task_a.short_id, &task_a.title, "main")
        .await
        .unwrap();

    // Create workspace B and modify shared_config.rs to 120
    let ws_b = git_coord
        .prepare_task_workspace(
            &repo_id.repo_root,
            project.id,
            task_b.id,
            &task_b.short_id,
            "main",
            None,
        )
        .await
        .unwrap();
    tokio::fs::write(ws_b.worktree_path.join("shared_config.rs"), "pub const TIMEOUT: u64 = 120;\n")
        .await
        .unwrap();
    let fin_b = git_coord
        .finalize_task(&ws_b, &task_b.short_id, &task_b.title, "main")
        .await
        .unwrap();

    // Run cross-task conflict detection
    let conflict_report = git_coord
        .check_cross_task_conflicts(
            &repo_id.repo_root,
            project.id,
            task_a.id,
            &fin_a.task_branch,
            task_b.id,
            &fin_b.task_branch,
        )
        .await
        .expect("Conflict detection must run cleanly");

    assert!(conflict_report.is_some(), "Conflict between branch A and branch B must be detected");
    let rep = conflict_report.unwrap();
    assert!(rep.conflicting_files.iter().any(|f| f.contains("shared_config.rs") || f == "<conflict detected>"));

    // Verify persisted in PostgreSQL git_conflicts table
    let conflicts_in_db = GitConflictRepository::list_unresolved(&pool, project.id).await.unwrap();
    assert!(!conflicts_in_db.is_empty());
    assert_eq!(conflicts_in_db[0].project_id, project.id);
    assert_eq!(conflicts_in_db[0].task_id_a, task_a.id);
    assert_eq!(conflicts_in_db[0].task_id_b, task_b.id);

    // Resolve conflict
    GitConflictRepository::mark_resolved(&pool, conflicts_in_db[0].id).await.unwrap();
    let unresolved_after = GitConflictRepository::list_unresolved(&pool, project.id).await.unwrap();
    assert!(unresolved_after.is_empty());

    // Cleanup
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}

#[test]
fn test_phase9_concurrency_safety_guard() {
    let non_conflicting_a = vec!["src/routes/users.rs".to_string()];
    let non_conflicting_b = vec!["src/routes/orders.rs".to_string()];
    assert_eq!(
        ConflictDetector::check_concurrency_safety(&non_conflicting_a, &non_conflicting_b),
        ConcurrencySafety::Safe
    );

    let conflicting_a = vec!["src/database/schema.rs".to_string(), "Cargo.lock".to_string()];
    let conflicting_b = vec!["src/database/schema.rs".to_string(), "Cargo.toml".to_string()];
    match ConflictDetector::check_concurrency_safety(&conflicting_a, &conflicting_b) {
        ConcurrencySafety::OverlapRisk { shared_resources } => {
            assert_eq!(shared_resources, vec!["src/database/schema.rs"]);
        }
        ConcurrencySafety::Safe => panic!("Must identify overlap risk"),
    }
}

#[tokio::test]
async fn test_phase9_two_agents_parallel_git_workflow_over_jetstream() {
    let _ = dotenvy::dotenv();
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

    let Ok(pool) = create_pool(&db_url).await else {
        eprintln!("Skipping test: DB unreachable");
        return;
    };
    let _ = run_migrations(&pool).await;

    let Ok((client, jetstream)) = connect(&nats_url, Some(&nats_token)).await else {
        eprintln!("Skipping test: NATS unreachable");
        return;
    };
    let _ = ensure_streams(&jetstream).await;

    let git_coord = GitCoordinator::new(pool.clone());
    let repo_dir = create_temp_repo_dir("agentmesh_p9_e2e");
    let repo_id = RepositoryIdentity::init(&repo_dir, "main").await.unwrap();

    // 1. Create Project and Proposal
    let project = ProjectRepository::create(
        &pool,
        &NewProject {
            name: "Phase 9 E2E Project".to_string(),
            description: "Testing end-to-end parallel agents Git workflow".to_string(),
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
            raw_prompt: "e2e".to_string(),
            raw_response: "{}".to_string(),
        },
    )
    .await
    .unwrap();

    // 2. Register Agent 1 (Alice) and Agent 2 (Bob)
    let agent_1 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Alice".to_string(),
            api_key_hash: "test_hash_alice".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["rust".to_string(), "backend".to_string()],
            nats_subject: "agents.alice.events".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let agent_2 = AgentRepository::create(
        &pool,
        &NewAgent {
            human_owner: "Bob".to_string(),
            api_key_hash: "test_hash_bob".to_string(),
            adapter_type: AdapterType::Agy,
            capabilities: vec!["frontend".to_string(), "typescript".to_string()],
            nats_subject: "agents.bob.events".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // 3. Create Task 1 (API) and Task 2 (UI)
    let task_1 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-010".to_string(),
            title: "Build REST API".to_string(),
            description: "Implement src/api.rs".to_string(),
            affected_resources: vec!["src/api.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    let task_2 = TaskRepository::create(
        &pool,
        &NewTask {
            project_id: project.id,
            short_id: "TASK-011".to_string(),
            title: "Build Web UI".to_string(),
            description: "Implement src/ui.rs".to_string(),
            affected_resources: vec!["src/ui.rs".to_string()],
            estimated_size: Some("M".to_string()),
            proposal_id: proposal.id,
        },
    )
    .await
    .unwrap();

    // 4. Coordinator prepares worktrees for both tasks
    let ws_1 = git_coord
        .prepare_task_workspace(&repo_id.repo_root, project.id, task_1.id, &task_1.short_id, "main", None)
        .await
        .unwrap();

    let ws_2 = git_coord
        .prepare_task_workspace(&repo_id.repo_root, project.id, task_2.id, &task_2.short_id, "main", None)
        .await
        .unwrap();

    // Start event subscriber
    let sub_pool = pool.clone();
    let events_consumer = EventSubscriber::create_consumer(&jetstream).await.unwrap();
    let sub_handle = tokio::spawn(async move {
        let mut messages = events_consumer.messages().await.unwrap();
        while let Some(Ok(msg)) = messages.next().await {
            let _ = msg.ack().await;
            if let Ok(agent_msg) = serde_json::from_slice::<AgentMessage>(&msg.payload) {
                let _ = EventSubscriber::handle_agent_message(&sub_pool, agent_msg).await;
            }
        }
    });

    // 5. Build TaskSpecs carrying Git context
    let spec_1 = TaskSpec::new(
        task_1.id,
        &task_1.short_id,
        &task_1.title,
        &task_1.description,
        task_1.resources(),
        vec![],
        format!("{}:1", task_1.id),
    )
    .with_git_context(ws_1.worktree_path.to_string_lossy(), "main", &ws_1.task_branch);

    let spec_2 = TaskSpec::new(
        task_2.id,
        &task_2.short_id,
        &task_2.title,
        &task_2.description,
        task_2.resources(),
        vec![],
        format!("{}:1", task_2.id),
    )
    .with_git_context(ws_2.worktree_path.to_string_lossy(), "main", &ws_2.task_branch);

    // Update tasks to Assigned
    TaskRepository::update_status(&pool, task_1.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task_1.id, Some(agent_1.id)).await.unwrap();
    TaskRepository::update_status(&pool, task_1.id, TaskStatus::Assigned).await.unwrap();

    TaskRepository::update_status(&pool, task_2.id, TaskStatus::Approved).await.unwrap();
    TaskRepository::assign_agent(&pool, task_2.id, Some(agent_2.id)).await.unwrap();
    TaskRepository::update_status(&pool, task_2.id, TaskStatus::Assigned).await.unwrap();

    // Publish assignments to JetStream
    TaskPublisher::publish_assignment(&jetstream, agent_1.id, &spec_1).await.unwrap();
    TaskPublisher::publish_assignment(&jetstream, agent_2.id, &spec_2).await.unwrap();

    // 6. Simulate parallel agent execution working inside their respective workspaces
    let client_1 = client.clone();
    let agent_1_id = agent_1.id;
    let ws_1_path = ws_1.worktree_path.clone();
    let task_1_id = task_1.id;

    let agent_1_handle = tokio::spawn(async move {
        let ev_subject = format!("agents.{agent_1_id}.events");
        // Started
        let _ = client_1.publish(
            ev_subject.clone(),
            serde_json::to_vec(&AgentMessage::TaskStarted {
                agent_id: agent_1_id,
                task_id: task_1_id,
                idempotency_key: format!("{}:1", task_1_id),
                timestamp: Utc::now(),
            }).unwrap().into(),
        ).await;

        // Perform work in isolated workspace
        tokio::fs::create_dir_all(ws_1_path.join("src")).await.unwrap();
        tokio::fs::write(ws_1_path.join("src/api.rs"), "pub fn api_route() -> &'static str { \"v1\" }\n")
            .await
            .unwrap();

        // Completed
        let _ = client_1.publish(
            ev_subject,
            serde_json::to_vec(&AgentMessage::Completed {
                agent_id: agent_1_id,
                task_id: task_1_id,
                summary: "API implemented successfully".to_string(),
                timestamp: Utc::now(),
            }).unwrap().into(),
        ).await;
    });

    let client_2 = client.clone();
    let agent_2_id = agent_2.id;
    let ws_2_path = ws_2.worktree_path.clone();
    let task_2_id = task_2.id;

    let agent_2_handle = tokio::spawn(async move {
        let ev_subject = format!("agents.{agent_2_id}.events");
        // Started
        let _ = client_2.publish(
            ev_subject.clone(),
            serde_json::to_vec(&AgentMessage::TaskStarted {
                agent_id: agent_2_id,
                task_id: task_2_id,
                idempotency_key: format!("{}:1", task_2_id),
                timestamp: Utc::now(),
            }).unwrap().into(),
        ).await;

        // Perform work in isolated workspace
        tokio::fs::create_dir_all(ws_2_path.join("src")).await.unwrap();
        tokio::fs::write(ws_2_path.join("src/ui.rs"), "export const view = 'dashboard';\n")
            .await
            .unwrap();

        // Completed
        let _ = client_2.publish(
            ev_subject,
            serde_json::to_vec(&AgentMessage::Completed {
                agent_id: agent_2_id,
                task_id: task_2_id,
                summary: "UI implemented successfully".to_string(),
                timestamp: Utc::now(),
            }).unwrap().into(),
        ).await;
    });

    let (res_1, res_2) = tokio::join!(agent_1_handle, agent_2_handle);
    res_1.unwrap();
    res_2.unwrap();

    // Give subscriber a moment to process events
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    sub_handle.abort();

    // 7. Verify both tasks reached Completed status in DB
    let t1 = TaskRepository::find_by_id(&pool, task_1.id).await.unwrap().unwrap();
    let t2 = TaskRepository::find_by_id(&pool, task_2.id).await.unwrap().unwrap();
    assert_eq!(t1.status, TaskStatus::Completed);
    assert_eq!(t2.status, TaskStatus::Completed);

    // 8. Finalize both tasks in Git via coordinator
    let fin_1 = git_coord.finalize_task(&ws_1, &task_1.short_id, &task_1.title, "main").await.unwrap();
    let fin_2 = git_coord.finalize_task(&ws_2, &task_2.short_id, &task_2.title, "main").await.unwrap();

    assert!(fin_1.can_merge_cleanly);
    assert!(fin_2.can_merge_cleanly);

    // 9. Merge both task branches into main
    let m1 = Command::new("git")
        .args(["merge", "--no-ff", "-m", "Merge api", &fin_1.task_branch])
        .current_dir(&repo_id.repo_root)
        .output()
        .await
        .unwrap();
    assert!(m1.status.success());

    let m2 = Command::new("git")
        .args(["merge", "--no-ff", "-m", "Merge ui", &fin_2.task_branch])
        .current_dir(&repo_id.repo_root)
        .output()
        .await
        .unwrap();
    assert!(m2.status.success());

    // 10. Verify both files exist together on main!
    assert!(repo_id.repo_root.join("src/api.rs").exists());
    assert!(repo_id.repo_root.join("src/ui.rs").exists());

    // Cleanup
    ProjectRepository::delete(&pool, project.id).await.unwrap();
    AgentRepository::delete(&pool, agent_1.id).await.unwrap();
    AgentRepository::delete(&pool, agent_2.id).await.unwrap();
    let _ = tokio::fs::remove_dir_all(&repo_dir).await;
}
