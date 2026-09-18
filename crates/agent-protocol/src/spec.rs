use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Full specification of a task assigned to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub task_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub description: String,
    pub affected_resources: Vec<String>,
    pub depends_on: Vec<Uuid>,
    pub idempotency_key: String,
    pub assigned_at: DateTime<Utc>,
    #[serde(default)]
    pub task_branch: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

impl TaskSpec {
    pub fn new(
        task_id: Uuid,
        short_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        affected_resources: Vec<String>,
        depends_on: Vec<Uuid>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            task_id,
            short_id: short_id.into(),
            title: title.into(),
            description: description.into(),
            affected_resources,
            depends_on,
            idempotency_key: idempotency_key.into(),
            assigned_at: Utc::now(),
            task_branch: None,
            base_branch: None,
            repo_path: None,
        }
    }

    pub fn with_git_context(
        mut self,
        repo_path: impl Into<String>,
        base_branch: impl Into<String>,
        task_branch: impl Into<String>,
    ) -> Self {
        self.repo_path = Some(repo_path.into());
        self.base_branch = Some(base_branch.into());
        self.task_branch = Some(task_branch.into());
        self
    }
}

