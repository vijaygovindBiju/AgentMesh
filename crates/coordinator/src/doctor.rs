//! Read-only environment and readiness diagnostics.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

use crate::db::repositories::AgentRepository;
use crate::domain::{AgentStatus, HealthStatus};
use crate::messaging::streams::{AGENT_EVENTS_STREAM, TASK_ASSIGNMENTS_STREAM};

const CHECK_TIMEOUT: Duration = Duration::from_secs(3);
const HEARTBEAT_TIMEOUT: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoctorCategory {
    Application,
    Infrastructure,
    CodingAgent,
    WorkerFleet,
    Repository,
    Safety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: String,
    pub category: DoctorCategory,
    pub status: DoctorStatus,
    pub summary: String,
    pub evidence: String,
    pub remediation: Option<String>,
    pub skipped: bool,
    pub duration_ms: u128,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Readiness {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub readiness: Readiness,
    pub required_failures: usize,
    pub warnings: usize,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DoctorConfig {
    pub database_url: String,
    pub nats_url: String,
    pub nats_token: String,
    pub repository_path: PathBuf,
}

impl DoctorConfig {
    pub fn from_environment() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
            }),
            nats_url: std::env::var("NATS_URL")
                .unwrap_or_else(|_| "nats://localhost:4222".to_string()),
            nats_token: std::env::var("NATS_AUTH_TOKEN")
                .unwrap_or_else(|_| "agentmesh_dev_token".to_string()),
            repository_path: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl DoctorReport {
    pub fn required_failures(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == DoctorStatus::Fail)
            .count()
    }
}

pub async fn run_doctor(config: DoctorConfig) -> DoctorReport {
    let started = Instant::now();
    let mut checks = Vec::new();

    checks.push(check_application());
    checks.push(check_configuration());

    let db = check_database(&config.database_url).await;
    let db_pool = db.0;
    checks.push(db.1);

    let nats = check_nats(&config.nats_url, &config.nats_token).await;
    checks.push(nats.1);

    checks.push(check_runtime().await);
    checks.push(check_repository(&config.repository_path).await);
    checks.push(check_safety());

    if let Some(pool) = db_pool {
        checks.push(check_schema(&pool).await);
        checks.extend(check_workers(&pool).await);
    } else {
        checks.push(DoctorCheck::skip(
            "worker-fleet.available",
            DoctorCategory::WorkerFleet,
            "Worker fleet could not be inspected",
            "PostgreSQL is unavailable",
        ));
    }

    if !nats.0 {
        checks.push(DoctorCheck::skip(
            "infrastructure.streams",
            DoctorCategory::Infrastructure,
            "JetStream streams could not be inspected",
            "NATS is unavailable",
        ));
    } else if let Some(js) = nats.2 {
        checks.push(check_streams(&js).await);
    }

    let required_failures = checks
        .iter()
        .filter(|c| c.status == DoctorStatus::Fail)
        .count();
    let warnings = checks
        .iter()
        .filter(|c| c.status == DoctorStatus::Warn)
        .count();

    let readiness = if required_failures > 0 {
        Readiness::Blocked
    } else if warnings > 0 {
        Readiness::Degraded
    } else {
        Readiness::Ready
    };

    let report = DoctorReport {
        checks,
        readiness,
        required_failures,
        warnings,
        generated_at: Utc::now(),
    };
    tracing::debug!(elapsed_ms = started.elapsed().as_millis(), ?report.readiness, "Doctor run completed");
    report
}

fn check_application() -> DoctorCheck {
    DoctorCheck::pass(
        "application.version",
        DoctorCategory::Application,
        format!("AgentMesh coordinator v{}", env!("CARGO_PKG_VERSION")),
        "Coordinator diagnostic subsystem initialized",
    )
}

fn check_configuration() -> DoctorCheck {
    let provider = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "agy".to_string());
    let valid_provider = matches!(provider.to_lowercase().as_str(), "agy" | "anthropic");
    if !valid_provider {
        return DoctorCheck::fail(
            "application.configuration",
            DoctorCategory::Application,
            "Unsupported AI_PROVIDER",
            format!("Configured provider: {provider}"),
            "Set AI_PROVIDER to agy or anthropic",
        );
    }

    if provider.eq_ignore_ascii_case("anthropic")
        && std::env::var("ANTHROPIC_API_KEY").map_or(true, |v| v.trim().is_empty())
    {
        return DoctorCheck::fail(
            "application.configuration",
            DoctorCategory::Application,
            "Anthropic planner is missing its API key",
            "AI_PROVIDER=anthropic; key presence: no",
            "Set ANTHROPIC_API_KEY in the coordinator environment",
        );
    }

    DoctorCheck::pass(
        "application.configuration",
        DoctorCategory::Application,
        "Configuration values are compatible",
        format!("AI_PROVIDER={provider} (credentials redacted)"),
    )
}

async fn check_database(url: &str) -> (Option<PgPool>, DoctorCheck) {
    let started = Instant::now();
    let result = timeout(CHECK_TIMEOUT, crate::db::pool::create_pool(url)).await;
    match result {
        Ok(Ok(pool)) => (
            Some(pool),
            DoctorCheck::from_timing(
                "infrastructure.postgres",
                DoctorCategory::Infrastructure,
                DoctorStatus::Pass,
                "PostgreSQL is reachable",
                "Connection established; URL credentials redacted",
                None,
                started.elapsed(),
            ),
        ),
        Ok(Err(error)) => (
            None,
            DoctorCheck::from_timing(
                "infrastructure.postgres",
                DoctorCategory::Infrastructure,
                DoctorStatus::Fail,
                "PostgreSQL is unavailable",
                redact_error(&error.to_string()),
                Some("Start PostgreSQL and verify DATABASE_URL".to_string()),
                started.elapsed(),
            ),
        ),
        Err(_) => (
            None,
            DoctorCheck::from_timing(
                "infrastructure.postgres",
                DoctorCategory::Infrastructure,
                DoctorStatus::Fail,
                "PostgreSQL connectivity timed out",
                format!("Timeout after {} seconds", CHECK_TIMEOUT.as_secs()),
                Some("Check PostgreSQL health and network reachability".to_string()),
                started.elapsed(),
            ),
        ),
    }
}

async fn check_schema(pool: &PgPool) -> DoctorCheck {
    let started = Instant::now();
    let result = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'projects')",
    )
    .fetch_one(pool)
    .await;
    match result {
        Ok(true) => DoctorCheck::from_timing(
            "infrastructure.postgres-schema",
            DoctorCategory::Infrastructure,
            DoctorStatus::Pass,
            "PostgreSQL schema is ready",
            "Required projects table is present; no migration was run",
            None,
            started.elapsed(),
        ),
        Ok(false) => DoctorCheck::from_timing(
            "infrastructure.postgres-schema",
            DoctorCategory::Infrastructure,
            DoctorStatus::Fail,
            "PostgreSQL schema is incomplete",
            "Required projects table is missing",
            Some("Start the coordinator once to apply migrations".to_string()),
            started.elapsed(),
        ),
        Err(error) => DoctorCheck::from_timing(
            "infrastructure.postgres-schema",
            DoctorCategory::Infrastructure,
            DoctorStatus::Fail,
            "PostgreSQL schema could not be inspected",
            redact_error(&error.to_string()),
            Some("Check database permissions and migrations".to_string()),
            started.elapsed(),
        ),
    }
}

async fn check_nats(
    url: &str,
    token: &str,
) -> (bool, DoctorCheck, Option<async_nats::jetstream::Context>) {
    let started = Instant::now();
    let result = timeout(CHECK_TIMEOUT, crate::messaging::connect(url, Some(token))).await;
    match result {
        Ok(Ok((_client, js))) => (
            true,
            DoctorCheck::from_timing(
                "infrastructure.nats",
                DoctorCategory::Infrastructure,
                DoctorStatus::Pass,
                "NATS is reachable",
                "Authenticated connection established; token redacted",
                None,
                started.elapsed(),
            ),
            Some(js),
        ),
        Ok(Err(error)) => (
            false,
            DoctorCheck::from_timing(
                "infrastructure.nats",
                DoctorCategory::Infrastructure,
                DoctorStatus::Fail,
                "NATS is unavailable",
                redact_error(&error.to_string()),
                Some("Start NATS and verify NATS_URL and NATS_AUTH_TOKEN".to_string()),
                started.elapsed(),
            ),
            None,
        ),
        Err(_) => (
            false,
            DoctorCheck::from_timing(
                "infrastructure.nats",
                DoctorCategory::Infrastructure,
                DoctorStatus::Fail,
                "NATS connectivity timed out",
                format!("Timeout after {} seconds", CHECK_TIMEOUT.as_secs()),
                Some("Check NATS health and network reachability".to_string()),
                started.elapsed(),
            ),
            None,
        ),
    }
}

async fn check_streams(js: &async_nats::jetstream::Context) -> DoctorCheck {
    let started = Instant::now();
    let assignments = js.get_stream(TASK_ASSIGNMENTS_STREAM).await;
    let events = js.get_stream(AGENT_EVENTS_STREAM).await;
    if assignments.is_ok() && events.is_ok() {
        DoctorCheck::from_timing(
            "infrastructure.streams",
            DoctorCategory::Infrastructure,
            DoctorStatus::Pass,
            "Required JetStream streams are available",
            format!("{TASK_ASSIGNMENTS_STREAM}, {AGENT_EVENTS_STREAM}"),
            None,
            started.elapsed(),
        )
    } else {
        DoctorCheck::from_timing(
            "infrastructure.streams",
            DoctorCategory::Infrastructure,
            DoctorStatus::Fail,
            "Required JetStream streams are missing or inaccessible",
            format!(
                "{TASK_ASSIGNMENTS_STREAM}: {}; {AGENT_EVENTS_STREAM}: {}",
                assignments.is_ok(),
                events.is_ok()
            ),
            Some("Start the coordinator once to create required streams".to_string()),
            started.elapsed(),
        )
    }
}

async fn check_runtime() -> DoctorCheck {
    let path = std::env::var("AGY_BIN_PATH")
        .ok()
        .filter(|p| !p.trim().is_empty());
    let executable = path
        .or_else(|| {
            let candidate = PathBuf::from(std::env::var("HOME").ok()?).join(".local/bin/agy");
            candidate.exists().then(|| candidate.display().to_string())
        })
        .unwrap_or_else(|| "agy".to_string());
    let started = Instant::now();
    let result = timeout(
        CHECK_TIMEOUT,
        Command::new(&executable)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await;
    match result {
        Ok(Ok(output)) if output.status.success() => DoctorCheck::from_timing(
            "coding-agent.agy",
            DoctorCategory::CodingAgent,
            DoctorStatus::Pass,
            "agy executable is available",
            format!("Executable: {executable}; version output detected"),
            None,
            started.elapsed(),
        ),
        Ok(Ok(output)) => DoctorCheck::from_timing(
            "coding-agent.agy",
            DoctorCategory::CodingAgent,
            DoctorStatus::Warn,
            "agy executable returned a non-success version result",
            format!("Executable: {executable}; exit status: {}", output.status),
            Some("Run the agy CLI directly and verify its installation".to_string()),
            started.elapsed(),
        ),
        Ok(Err(error)) => DoctorCheck::from_timing(
            "coding-agent.agy",
            DoctorCategory::CodingAgent,
            DoctorStatus::Warn,
            "agy executable is unavailable",
            format!(
                "Could not execute configured path: {executable} ({})",
                redact_error(&error.to_string())
            ),
            Some("Install and authenticate agy, or set AGY_BIN_PATH".to_string()),
            started.elapsed(),
        ),
        Err(_) => DoctorCheck::from_timing(
            "coding-agent.agy",
            DoctorCategory::CodingAgent,
            DoctorStatus::Warn,
            "agy version check timed out",
            format!("Timeout after {} seconds", CHECK_TIMEOUT.as_secs()),
            Some("Run agy --version directly and inspect the installation".to_string()),
            started.elapsed(),
        ),
    }
}

async fn check_repository(path: &Path) -> DoctorCheck {
    let started = Instant::now();
    let result = timeout(
        CHECK_TIMEOUT,
        Command::new("git")
            .args([
                "-C",
                &path.display().to_string(),
                "rev-parse",
                "--show-toplevel",
            ])
            .output(),
    )
    .await;
    match result {
        Ok(Ok(output)) if output.status.success() => DoctorCheck::from_timing(
            "repository.git",
            DoctorCategory::Repository,
            DoctorStatus::Pass,
            "Repository is accessible to Git",
            format!("Root: {}", String::from_utf8_lossy(&output.stdout).trim()),
            None,
            started.elapsed(),
        ),
        _ => DoctorCheck::from_timing(
            "repository.git",
            DoctorCategory::Repository,
            DoctorStatus::Fail,
            "Current repository is not accessible to Git",
            format!("Checked path: {}", path.display()),
            Some("Run Doctor from the target Git repository".to_string()),
            started.elapsed(),
        ),
    }
}

fn check_safety() -> DoctorCheck {
    DoctorCheck::pass(
        "safety.approval-gate",
        DoctorCategory::Safety,
        "Human approval gate is enabled",
        "Normal task flow requires explicit approval; Doctor performs no writes or execution probes",
    )
}

async fn check_workers(pool: &PgPool) -> Vec<DoctorCheck> {
    let agents = match AgentRepository::list(pool).await {
        Ok(agents) => agents,
        Err(error) => {
            return vec![DoctorCheck::fail(
                "worker-fleet.query",
                DoctorCategory::WorkerFleet,
                "Registered workers could not be inspected",
                redact_error(&error.to_string()),
                "Check database permissions and the agents table",
            )]
        }
    };
    if agents.is_empty() {
        return vec![DoctorCheck::warn(
            "worker-fleet.available",
            DoctorCategory::WorkerFleet,
            "No registered coding workers",
            "Agent registry contains zero workers; coding execution cannot be verified",
            "Start agent-agy with a valid NATS connection and API key",
        )];
    }

    let stale = agents.iter().filter(|agent| {
        agent.last_seen.map_or(true, |seen| {
            (Utc::now() - seen).num_seconds() > HEARTBEAT_TIMEOUT
        })
    });
    let stale_count = stale.count();
    let healthy_idle = agents
        .iter()
        .filter(|agent| {
            agent.status == AgentStatus::Idle
                && agent.health_status == HealthStatus::Healthy
                && agent.last_seen.map_or(false, |seen| {
                    (Utc::now() - seen).num_seconds() <= HEARTBEAT_TIMEOUT
                })
        })
        .count();

    let mut checks = classify_worker_readiness(agents.len(), stale_count, healthy_idle);
    checks.extend([
        DoctorCheck::skip(
            "worker-fleet.capabilities",
            DoctorCategory::WorkerFleet,
            "Requested capability matching was not evaluated",
            "Doctor was not given a task or required capability set",
        ),
        DoctorCheck::skip(
            "coding-agent.execution-probe",
            DoctorCategory::CodingAgent,
            "Real coding-agent execution probe was skipped",
            "Execution probes require explicit future opt-in and task scope",
        ),
    ]);
    checks
}

fn classify_worker_readiness(
    registered: usize,
    stale: usize,
    healthy_idle: usize,
) -> Vec<DoctorCheck> {
    if registered == 0 {
        return vec![DoctorCheck::warn(
            "worker-fleet.available",
            DoctorCategory::WorkerFleet,
            "No registered coding workers",
            "Agent registry contains zero workers; coding execution cannot be verified",
            "Start agent-agy with a valid NATS connection and API key",
        )];
    }
    vec![
        if stale == 0 {
            DoctorCheck::pass(
                "worker-fleet.heartbeats",
                DoctorCategory::WorkerFleet,
                "Registered workers have recent heartbeats",
                format!("{registered} worker(s); stale: 0"),
            )
        } else {
            DoctorCheck::warn(
                "worker-fleet.heartbeats",
                DoctorCategory::WorkerFleet,
                "Some registered workers have stale heartbeats",
                format!("{registered} worker(s); stale: {stale}"),
                "Inspect worker processes and NATS connectivity",
            )
        },
        if healthy_idle > 0 {
            DoctorCheck::pass(
                "worker-fleet.ready",
                DoctorCategory::WorkerFleet,
                "At least one healthy idle worker is registered",
                format!("Healthy and idle workers: {healthy_idle}"),
            )
        } else {
            DoctorCheck::warn(
                "worker-fleet.ready",
                DoctorCategory::WorkerFleet,
                "No healthy idle worker is currently ready",
                "Registered status is not proof of execution capability",
                "Start a worker or wait for it to become healthy and idle",
            )
        },
    ]
}

fn redact_error(error: &str) -> String {
    let mut redacted = error.to_string();
    for scheme in ["postgres://", "postgresql://", "nats://"] {
        while let Some(start) = redacted.find(scheme) {
            let end = redacted[start..]
                .find('@')
                .map(|offset| start + offset + 1)
                .unwrap_or_else(|| start + scheme.len());
            redacted.replace_range(start..end, "[redacted]");
        }
    }
    for key in ["NATS_AUTH_TOKEN=", "ANTHROPIC_API_KEY="] {
        if let Some(start) = redacted.find(key) {
            let value_start = start + key.len();
            let value_end = redacted[value_start..]
                .find(char::is_whitespace)
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(value_start..value_end, "[redacted]");
        }
    }
    redacted
}

impl DoctorCheck {
    fn from_timing(
        id: &str,
        category: DoctorCategory,
        status: DoctorStatus,
        summary: impl Into<String>,
        evidence: impl Into<String>,
        remediation: Option<String>,
        elapsed: Duration,
    ) -> Self {
        Self {
            id: id.to_string(),
            category,
            skipped: status == DoctorStatus::Skip,
            status,
            summary: summary.into(),
            evidence: evidence.into(),
            remediation,
            duration_ms: elapsed.as_millis(),
            timestamp: Utc::now(),
        }
    }

    fn pass(
        id: &str,
        category: DoctorCategory,
        summary: impl Into<String>,
        evidence: impl Into<String>,
    ) -> Self {
        Self::from_timing(
            id,
            category,
            DoctorStatus::Pass,
            summary,
            evidence,
            None,
            Duration::ZERO,
        )
    }

    fn warn(
        id: &str,
        category: DoctorCategory,
        summary: impl Into<String>,
        evidence: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self::from_timing(
            id,
            category,
            DoctorStatus::Warn,
            summary,
            evidence,
            Some(remediation.into()),
            Duration::ZERO,
        )
    }

    fn fail(
        id: &str,
        category: DoctorCategory,
        summary: impl Into<String>,
        evidence: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self::from_timing(
            id,
            category,
            DoctorStatus::Fail,
            summary,
            evidence,
            Some(remediation.into()),
            Duration::ZERO,
        )
    }

    fn skip(
        id: &str,
        category: DoctorCategory,
        summary: impl Into<String>,
        evidence: impl Into<String>,
    ) -> Self {
        Self::from_timing(
            id,
            category,
            DoctorStatus::Skip,
            summary,
            evidence,
            None,
            Duration::ZERO,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn redacts_urls_and_secret_names() {
        let result = redact_error("failed postgres://user:secret@host/db NATS_AUTH_TOKEN=abc");
        assert!(!result.contains("secret"));
        assert!(!result.contains("abc"));
        assert!(result.contains("[redacted]"));
    }

    #[test]
    fn report_classifies_required_failures_and_warnings() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck::fail("required", DoctorCategory::Application, "bad", "bad", "fix"),
                DoctorCheck::warn(
                    "optional",
                    DoctorCategory::CodingAgent,
                    "warn",
                    "warn",
                    "fix",
                ),
            ],
            readiness: Readiness::Blocked,
            required_failures: 1,
            warnings: 1,
            generated_at: Utc::now(),
        };
        assert_eq!(report.required_failures(), 1);
        assert_eq!(report.readiness, Readiness::Blocked);
    }

    #[test]
    fn missing_provider_configuration_is_a_required_failure() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AI_PROVIDER", "unsupported-provider");
        let check = check_configuration();
        std::env::remove_var("AI_PROVIDER");
        assert_eq!(check.status, DoctorStatus::Fail);
    }

    #[test]
    fn worker_readiness_distinguishes_stale_and_idle() {
        let checks = classify_worker_readiness(2, 1, 0);
        assert_eq!(checks[0].status, DoctorStatus::Warn);
        assert_eq!(checks[1].status, DoctorStatus::Warn);
        assert_eq!(
            classify_worker_readiness(0, 0, 0)[0].status,
            DoctorStatus::Warn
        );
    }

    #[tokio::test]
    async fn unavailable_nats_is_a_bounded_failure_without_secret_output() {
        let (_connected, check, _js) = check_nats("nats://127.0.0.1:1", "top-secret-token").await;
        assert_eq!(check.status, DoctorStatus::Fail);
        assert!(!check.evidence.contains("top-secret-token"));
    }
}
