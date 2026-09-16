use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Project lifecycle status.
///
/// `#[sqlx(rename_all = "snake_case")]` maps Rust PascalCase variants to the
/// corresponding PostgreSQL `project_status` enum values:
///   Draft → "draft",  Planning → "planning",  etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "project_status", rename_all = "snake_case")]
pub enum ProjectStatus {
    Draft,
    Planning,
    Active,
    Paused,
    Completed,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        Self::Draft
    }
}

/// A coordinated software project.
///
/// `id`, `status`, `created_at`, and `updated_at` are set by the database.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Data required to create a new project.
/// The database generates `id`, `status` (defaults to `Draft`), and timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_status_serde_round_trip() {
        for status in [
            ProjectStatus::Draft,
            ProjectStatus::Planning,
            ProjectStatus::Active,
            ProjectStatus::Paused,
            ProjectStatus::Completed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let recovered: ProjectStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, recovered);
        }
    }

    #[test]
    fn project_status_default_is_draft() {
        assert_eq!(ProjectStatus::default(), ProjectStatus::Draft);
    }
}
