use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ─── OverlapSeverity ──────────────────────────────────────────────────────────

/// How serious a detected resource overlap is.
///
/// PostgreSQL enum type: `overlap_severity`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "overlap_severity", rename_all = "snake_case")]
pub enum OverlapSeverity {
    /// Informational — same file mentioned, but tasks are sequential.
    Info,
    /// Concurrent modification likely — human should review assignments.
    Warning,
    /// Same primary interface or model modified by concurrent tasks.
    Critical,
}

// ─── OverlapWarning ───────────────────────────────────────────────────────────

/// A detected conflict where two or more tasks share a resource
/// (file path, module name, or API name).
///
/// Generated during proposal analysis, before human approval.
/// Displayed on the affected task cards in the plan review TUI screen.
/// Must be acknowledged by the human team (or resolved by reassigning tasks).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OverlapWarning {
    pub id: Uuid,
    pub project_id: Uuid,
    /// JSONB array of `Uuid` values — the tasks that share `resource`.
    /// Use `OverlapWarning::task_id_list()` to extract as `Vec<Uuid>`.
    pub task_ids: Value,
    /// The shared resource: a file path, module name, or API endpoint string.
    pub resource: String,
    pub severity: OverlapSeverity,
    /// `true` once the human team has acknowledged this warning in the TUI.
    pub acknowledged: bool,
    pub created_at: DateTime<Utc>,
}

impl OverlapWarning {
    /// Extracts `task_ids` as a `Vec<Uuid>`.
    /// Silently skips entries that cannot be parsed as UUIDs.
    pub fn task_id_list(&self) -> Vec<Uuid> {
        match &self.task_ids {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| s.parse().ok()))
                .collect(),
            _ => vec![],
        }
    }
}

// ─── NewOverlapWarning ────────────────────────────────────────────────────────

/// Data required to record a new overlap warning.
/// Id, `acknowledged` (defaults to `false`), and `created_at` are DB-generated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOverlapWarning {
    pub project_id: Uuid,
    /// The tasks that share the resource.
    pub task_ids: Vec<Uuid>,
    pub resource: String,
    pub severity: OverlapSeverity,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_severity_serde_round_trip() {
        for severity in [
            OverlapSeverity::Info,
            OverlapSeverity::Warning,
            OverlapSeverity::Critical,
        ] {
            let json = serde_json::to_string(&severity).unwrap();
            let recovered: OverlapSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(severity, recovered);
        }
    }

    #[test]
    fn task_id_list_extracts_uuids() {
        use serde_json::json;
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let task_ids = json!([id1.to_string(), id2.to_string()]);

        let list: Vec<Uuid> = match &task_ids {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| s.parse().ok()))
                .collect(),
            _ => vec![],
        };

        assert_eq!(list.len(), 2);
        assert!(list.contains(&id1));
        assert!(list.contains(&id2));
    }
}
