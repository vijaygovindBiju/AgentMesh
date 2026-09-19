use anyhow::{Context, Result};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use tracing::info;
use uuid::Uuid;

use crate::db::repositories::{OverlapWarningRepository, TaskRepository};
use crate::domain::{
    DependencyKind, NewOverlapWarning, OverlapSeverity, OverlapWarning, Task, TaskDependency,
};
use crate::tui::state::ReviewTaskState;

/// Resource-level overlap detector comparing affected resources across tasks.
///
/// Distinguishes Critical (concurrent conflict) from Info (sequential modification)
/// by computing the transitive closure of blocking dependencies.
pub struct OverlapDetector;

impl OverlapDetector {
    /// Detects resource overlaps among a set of tasks and their dependencies.
    ///
    /// Algorithm:
    /// 1. Maps each normalized resource path to the tasks modifying it.
    /// 2. Builds a directed reachability graph from `DependencyKind::Blocks` prerequisites.
    /// 3. Computes transitive reachability (if A blocks B and B blocks C, then A precedes C).
    /// 4. For any resource shared by > 1 task:
    ///    - If any pair has no dependency ordering (neither reaches the other), concurrent
    ///      execution is possible -> `OverlapSeverity::Critical`.
    ///    - If all pairs are strictly ordered in the dependency graph -> `OverlapSeverity::Info`.
    pub fn detect_overlaps(
        project_id: Uuid,
        tasks: &[Task],
        dependencies: &[TaskDependency],
    ) -> Vec<NewOverlapWarning> {
        let mut resource_to_tasks: HashMap<String, Vec<Uuid>> = HashMap::new();

        for task in tasks {
            for res in task.resources() {
                let norm = res.trim().trim_matches('/').to_string();
                if !norm.is_empty() {
                    resource_to_tasks.entry(norm).or_default().push(task.id);
                }
            }
        }

        // Build reachability set: (depends_on_id, dependent_id) means depends_on precedes dependent
        let mut reachability: HashSet<(Uuid, Uuid)> = HashSet::new();
        for dep in dependencies {
            if dep.kind == DependencyKind::Blocks {
                reachability.insert((dep.depends_on_id, dep.dependent_id));
            }
        }

        // Transitive closure (Floyd-Warshall reachability)
        let task_ids: Vec<Uuid> = tasks.iter().map(|t| t.id).collect();
        for &k in &task_ids {
            for &i in &task_ids {
                for &j in &task_ids {
                    if reachability.contains(&(i, k)) && reachability.contains(&(k, j)) {
                        reachability.insert((i, j));
                    }
                }
            }
        }

        let mut warnings = Vec::new();
        for (resource, mut sharing_tasks) in resource_to_tasks {
            if sharing_tasks.len() > 1 {
                sharing_tasks.sort();
                sharing_tasks.dedup();

                let mut has_concurrent_pair = false;
                for i in 0..sharing_tasks.len() {
                    for j in (i + 1)..sharing_tasks.len() {
                        let t1 = sharing_tasks[i];
                        let t2 = sharing_tasks[j];
                        let is_sequential =
                            reachability.contains(&(t1, t2)) || reachability.contains(&(t2, t1));
                        if !is_sequential {
                            has_concurrent_pair = true;
                            break;
                        }
                    }
                    if has_concurrent_pair {
                        break;
                    }
                }

                let severity = if has_concurrent_pair {
                    OverlapSeverity::Critical
                } else {
                    OverlapSeverity::Info
                };

                warnings.push(NewOverlapWarning {
                    project_id,
                    task_ids: sharing_tasks,
                    resource,
                    severity,
                });
            }
        }

        warnings.sort_by(|a, b| a.resource.cmp(&b.resource));
        warnings
    }

    /// Detects resource overlaps and persists them into the database.
    pub async fn detect_and_persist(
        pool: &PgPool,
        project_id: Uuid,
        tasks: &[Task],
        dependencies: &[TaskDependency],
    ) -> Result<Vec<OverlapWarning>> {
        let new_warnings = Self::detect_overlaps(project_id, tasks, dependencies);
        let mut persisted = Vec::new();

        for new_w in &new_warnings {
            let warning = OverlapWarningRepository::create(pool, new_w)
                .await
                .with_context(|| format!("Failed to record overlap for '{}'", new_w.resource))?;
            persisted.push(warning);
        }

        info!(
            project_id = %project_id,
            count = persisted.len(),
            "Resource overlap analysis completed and persisted"
        );

        Ok(persisted)
    }

    /// Detects overlaps for all tasks belonging to a given project by querying PostgreSQL.
    pub async fn detect_and_persist_for_project(
        pool: &PgPool,
        project_id: Uuid,
    ) -> Result<Vec<OverlapWarning>> {
        let tasks = TaskRepository::list_by_project(pool, project_id).await?;
        if tasks.is_empty() {
            return Ok(Vec::new());
        }

        let deps = sqlx::query_as!(
            TaskDependency,
            r#"
            SELECT td.dependent_id, td.depends_on_id, td.kind AS "kind: DependencyKind"
            FROM task_dependencies td
            JOIN tasks t ON td.dependent_id = t.id
            WHERE t.project_id = $1
            "#,
            project_id
        )
        .fetch_all(pool)
        .await
        .context("Failed to query project task dependencies")?;

        Self::detect_and_persist(pool, project_id, &tasks, &deps).await
    }

    /// Distributes overlap warnings into the corresponding `ReviewTaskState`s for TUI display.
    pub fn populate_task_overlaps(
        review_tasks: &mut [ReviewTaskState],
        warnings: &[OverlapWarning],
    ) {
        for rt in review_tasks.iter_mut() {
            let task_id = rt.task.id;
            rt.overlap_warnings = warnings
                .iter()
                .filter(|w| w.task_id_list().contains(&task_id))
                .cloned()
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TaskStatus;
    use chrono::Utc;
    use serde_json::json;

    fn make_test_task(id: Uuid, project_id: Uuid, short_id: &str, resources: &[&str]) -> Task {
        Task {
            id,
            project_id,
            short_id: short_id.to_string(),
            title: format!("Task {short_id}"),
            description: "Test task description".to_string(),
            status: TaskStatus::HumanReview,
            assigned_agent_id: None,
            affected_resources: json!(resources),
            estimated_size: Some("M".to_string()),
            proposal_id: Uuid::new_v4(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_no_shared_resources_produces_zero_warnings() {
        let proj_id = Uuid::new_v4();
        let t1 = make_test_task(Uuid::new_v4(), proj_id, "T1", &["src/auth.rs"]);
        let t2 = make_test_task(Uuid::new_v4(), proj_id, "T2", &["src/db.rs"]);

        let warnings = OverlapDetector::detect_overlaps(proj_id, &[t1, t2], &[]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_concurrent_tasks_sharing_resource_produce_critical_warning() {
        let proj_id = Uuid::new_v4();
        let t1_id = Uuid::new_v4();
        let t2_id = Uuid::new_v4();
        let t1 = make_test_task(t1_id, proj_id, "T1", &["src/models/user.rs", "src/auth.rs"]);
        let t2 = make_test_task(
            t2_id,
            proj_id,
            "T2",
            &["src/models/user.rs", "src/billing.rs"],
        );

        // No dependency between T1 and T2 -> Concurrent!
        let warnings = OverlapDetector::detect_overlaps(proj_id, &[t1, t2], &[]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].resource, "src/models/user.rs");
        assert_eq!(warnings[0].severity, OverlapSeverity::Critical);
        assert_eq!(warnings[0].task_ids.len(), 2);
        assert!(warnings[0].task_ids.contains(&t1_id));
        assert!(warnings[0].task_ids.contains(&t2_id));
    }

    #[test]
    fn test_sequential_tasks_sharing_resource_produce_info_warning() {
        let proj_id = Uuid::new_v4();
        let t1_id = Uuid::new_v4();
        let t2_id = Uuid::new_v4();
        let t1 = make_test_task(t1_id, proj_id, "T1", &["src/schema.sql"]);
        let t2 = make_test_task(t2_id, proj_id, "T2", &["src/schema.sql"]);

        // T2 depends on T1 (T1 blocks T2)
        let dep = TaskDependency {
            dependent_id: t2_id,
            depends_on_id: t1_id,
            kind: DependencyKind::Blocks,
        };

        let warnings = OverlapDetector::detect_overlaps(proj_id, &[t1, t2], &[dep]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].resource, "src/schema.sql");
        assert_eq!(warnings[0].severity, OverlapSeverity::Info);
    }

    #[test]
    fn test_relates_to_dependency_does_not_prevent_critical_warning() {
        let proj_id = Uuid::new_v4();
        let t1_id = Uuid::new_v4();
        let t2_id = Uuid::new_v4();
        let t1 = make_test_task(t1_id, proj_id, "T1", &["src/schema.sql"]);
        let t2 = make_test_task(t2_id, proj_id, "T2", &["src/schema.sql"]);

        // Non-blocking relates_to relationship
        let dep = TaskDependency {
            dependent_id: t2_id,
            depends_on_id: t1_id,
            kind: DependencyKind::RelatesTo,
        };

        let warnings = OverlapDetector::detect_overlaps(proj_id, &[t1, t2], &[dep]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, OverlapSeverity::Critical);
    }

    #[test]
    fn test_transitive_dependency_ordering() {
        let proj_id = Uuid::new_v4();
        let t1_id = Uuid::new_v4();
        let t2_id = Uuid::new_v4();
        let t3_id = Uuid::new_v4();
        let t1 = make_test_task(t1_id, proj_id, "T1", &["src/core.rs"]);
        let t2 = make_test_task(t2_id, proj_id, "T2", &["other.rs"]);
        let t3 = make_test_task(t3_id, proj_id, "T3", &["src/core.rs"]);

        // T1 -> T2 -> T3
        let dep1 = TaskDependency {
            dependent_id: t2_id,
            depends_on_id: t1_id,
            kind: DependencyKind::Blocks,
        };
        let dep2 = TaskDependency {
            dependent_id: t3_id,
            depends_on_id: t2_id,
            kind: DependencyKind::Blocks,
        };

        let warnings = OverlapDetector::detect_overlaps(proj_id, &[t1, t2, t3], &[dep1, dep2]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].resource, "src/core.rs");
        // Transitive reachability makes T1 and T3 sequential!
        assert_eq!(warnings[0].severity, OverlapSeverity::Info);
    }
}
