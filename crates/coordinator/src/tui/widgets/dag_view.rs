use std::collections::{HashMap, HashSet};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use uuid::Uuid;

use crate::domain::{ApprovalStatus, DependencyKind, TaskStatus};
use crate::tui::state::{AppState, ReviewTaskState};

pub struct DagViewWidget;

impl DagViewWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Task Dependency Graph (DAG) & Execution Pipeline [G: Task Cards, [/]: Jump Blocker/Dep] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if state.review_tasks.is_empty() {
            let empty = Paragraph::new("No tasks proposed yet. Go to [2: Proj] to submit a project.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, inner_area);
            return;
        }

        let lines = Self::build_dag_lines(state);

        // Compute vertical scroll if content exceeds inner_area.height
        let total_lines = lines.len();
        let visible_height = inner_area.height as usize;
        let scroll_offset = if total_lines > visible_height {
            let max_offset = total_lines.saturating_sub(visible_height);
            if state.dag_scroll_offset > 0 {
                state.dag_scroll_offset.min(max_offset)
            } else if let Some(target_line) = lines
                .iter()
                .position(|l| l.spans.iter().any(|s| s.content.contains('▶')))
            {
                let half_height = visible_height / 2;
                target_line.saturating_sub(half_height).min(max_offset)
            } else if !state.review_tasks.is_empty() {
                let progress = state.selected_task_index as f64 / state.review_tasks.len() as f64;
                ((max_offset as f64) * progress).round() as usize
            } else {
                0
            }
        } else {
            0
        };

        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(scroll_offset)
            .take(visible_height)
            .collect();

        let p = Paragraph::new(visible_lines);
        frame.render_widget(p, inner_area);
    }

    fn task_status_style(item: &ReviewTaskState) -> (String, Style) {
        match item.human_decision {
            Some(ApprovalStatus::Approved) => (
                "Approved".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Some(ApprovalStatus::EditedAndApproved) => (
                "Edited&Approved".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Some(ApprovalStatus::Rejected) => (
                "Rejected".to_string(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            None => match item.task.status {
                TaskStatus::Proposed => ("Proposed".to_string(), Style::default().fg(Color::DarkGray)),
                TaskStatus::HumanReview => (
                    "HumanReview".to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Approved => (
                    "Approved".to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Rejected => (
                    "Rejected".to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Assigned => (
                    "Assigned".to_string(),
                    Style::default().fg(Color::Cyan),
                ),
                TaskStatus::Executing => (
                    "Executing".to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Blocked => (
                    "Blocked".to_string(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Completed => (
                    "Completed".to_string(),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Failed => (
                    "Failed".to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                TaskStatus::Cancelled => (
                    "Cancelled".to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            },
        }
    }

    /// Compute topological tiers for the tasks:
    /// - Tier 0: Root tasks (0 blockers)
    /// - Tier 1: Waiting on Tier 0
    /// - Tier 2+: Waiting on previous tiers
    pub fn compute_topological_tiers(state: &AppState) -> Vec<Vec<Uuid>> {
        let mut tiers: Vec<Vec<Uuid>> = Vec::new();
        let mut assigned = HashSet::new();

        let id_set: HashSet<Uuid> = state.review_tasks.iter().map(|t| t.task.id).collect();

        // Tier 0: roots (no internal dependencies within the task set)
        let tier0: Vec<Uuid> = state
            .review_tasks
            .iter()
            .filter(|t| {
                t.dependencies
                    .iter()
                    .all(|dep| !id_set.contains(&dep.depends_on_id))
            })
            .map(|t| t.task.id)
            .collect();

        for id in &tier0 {
            assigned.insert(*id);
        }
        if !tier0.is_empty() {
            tiers.push(tier0);
        }

        // Iteratively assign subsequent tiers
        while assigned.len() < state.review_tasks.len() {
            let mut next_tier = Vec::new();
            for item in &state.review_tasks {
                let id = item.task.id;
                if !assigned.contains(&id) {
                    let all_deps_assigned = item
                        .dependencies
                        .iter()
                        .filter(|dep| id_set.contains(&dep.depends_on_id))
                        .all(|dep| assigned.contains(&dep.depends_on_id));

                    if all_deps_assigned {
                        next_tier.push(id);
                    }
                }
            }

            if next_tier.is_empty() {
                // Cycle or remaining unresolvable dependencies
                let mut remaining = Vec::new();
                for item in &state.review_tasks {
                    let id = item.task.id;
                    if !assigned.contains(&id) {
                        remaining.push(id);
                        assigned.insert(id);
                    }
                }
                if !remaining.is_empty() {
                    tiers.push(remaining);
                }
                break;
            }

            for id in &next_tier {
                assigned.insert(*id);
            }
            tiers.push(next_tier);
        }

        tiers
    }

    /// Recursively render a tree node and all its downstream dependents
    #[allow(clippy::too_many_arguments)]
    fn render_tree_node(
        task_id: Uuid,
        depth: usize,
        visited: &mut HashSet<Uuid>,
        lines: &mut Vec<Line<'static>>,
        prefix: &str,
        is_last: bool,
        kind: Option<DependencyKind>,
        state: &AppState,
        id_to_idx: &HashMap<Uuid, usize>,
        dependents_map: &HashMap<Uuid, Vec<(Uuid, DependencyKind)>>,
    ) {
        let Some(&idx) = id_to_idx.get(&task_id) else {
            return;
        };
        let item = &state.review_tasks[idx];
        let (status_str, status_style) = Self::task_status_style(item);
        let is_selected = idx == state.selected_task_index;

        if depth == 0 {
            // Root node formatting
            let cursor = if is_selected { "▶ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("[{}: {}] ", item.task.short_id, status_str),
                    status_style,
                ),
                Span::styled(
                    item.task.title.clone(),
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::styled(
                    if item.dependencies.is_empty() {
                        " (Root: Immediate Execution)".to_string()
                    } else {
                        String::new()
                    },
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        } else {
            // Child node formatting with branch characters
            let branch = if is_last { "└── " } else { "├── " };
            let edge_text = match kind {
                Some(DependencyKind::Blocks) => "blocks──▶ ",
                Some(DependencyKind::RelatesTo) => "relates──▶ ",
                None => "──▶ ",
            };

            let cursor = if is_selected { "▶" } else { " " };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{prefix}{branch}"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(edge_text, Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{cursor}[{}: {}] ", item.task.short_id, status_str),
                    status_style,
                ),
                Span::styled(
                    item.task.title.clone(),
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]));
        }

        // Cycle guard along this path
        if visited.contains(&task_id) {
            let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
            lines.push(Line::from(vec![
                Span::styled(child_prefix, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "└── ⚠ [Cycle/Repeated Reference Detected]",
                    Style::default().fg(Color::Red),
                ),
            ]));
            return;
        }

        visited.insert(task_id);

        if let Some(children) = dependents_map.get(&task_id) {
            let count = children.len();
            let next_prefix = if depth == 0 {
                "  ".to_string()
            } else {
                format!("{prefix}{}", if is_last { "    " } else { "│   " })
            };

            for (c_idx, (child_id, child_kind)) in children.iter().enumerate() {
                let child_is_last = c_idx + 1 == count;
                Self::render_tree_node(
                    *child_id,
                    depth + 1,
                    visited,
                    lines,
                    &next_prefix,
                    child_is_last,
                    Some(*child_kind),
                    state,
                    id_to_idx,
                    dependents_map,
                );
            }
        } else if depth == 0 {
            lines.push(Line::from(vec![
                Span::styled("   └── ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "(No downstream dependents)",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        visited.remove(&task_id);
    }

    fn build_dag_lines(state: &AppState) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // 1. Build index maps
        let id_to_idx: HashMap<Uuid, usize> = state
            .review_tasks
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.task.id, idx))
            .collect();

        // Map: task_id -> list of (dependent_task_id, kind)
        let mut dependents_map: HashMap<Uuid, Vec<(Uuid, DependencyKind)>> = HashMap::new();
        let mut non_roots: HashSet<Uuid> = HashSet::new();

        for item in &state.review_tasks {
            for dep in &item.dependencies {
                dependents_map
                    .entry(dep.depends_on_id)
                    .or_default()
                    .push((item.task.id, dep.kind));
                non_roots.insert(item.task.id);
            }
        }

        let total_tasks = state.review_tasks.len();
        let root_tasks: Vec<&ReviewTaskState> = state
            .review_tasks
            .iter()
            .filter(|t| !non_roots.contains(&t.task.id))
            .collect();

        // Header Summary
        lines.push(Line::from(vec![
            Span::styled(
                " Topology Summary: ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{total_tasks} Tasks"),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("{} Root(s) Ready", root_tasks.len()),
                Style::default().fg(Color::Green),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("{} Dependent(s)", non_roots.len()),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));

        // 2. Direct Dependency Blocking Relationships
        lines.push(Line::from(vec![Span::styled(
            "─── Dependency Relationships & Blocking Edges ────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        let mut edge_count = 0;
        for (parent_id, children) in &dependents_map {
            if let Some(&parent_idx) = id_to_idx.get(parent_id) {
                let parent_task = &state.review_tasks[parent_idx];
                let (parent_status, parent_style) = Self::task_status_style(parent_task);

                for (child_id, kind) in children {
                    edge_count += 1;
                    if let Some(&child_idx) = id_to_idx.get(child_id) {
                        let child_task = &state.review_tasks[child_idx];
                        let (child_status, child_style) = Self::task_status_style(child_task);

                        let relation_label = match kind {
                            DependencyKind::Blocks => "──blocks──▶",
                            DependencyKind::RelatesTo => "──relates──▶",
                        };

                        let parent_selected = parent_idx == state.selected_task_index;
                        let child_selected = child_idx == state.selected_task_index;

                        let prefix = if parent_selected || child_selected {
                            " ▶ "
                        } else {
                            "   "
                        };

                        lines.push(Line::from(vec![
                            Span::styled(prefix, Style::default().fg(Color::Cyan)),
                            Span::styled(
                                format!("[{}: {}] ", parent_task.task.short_id, parent_status),
                                parent_style,
                            ),
                            Span::styled(
                                format!("{} ", relation_label),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("[{}: {}] ", child_task.task.short_id, child_status),
                                child_style,
                            ),
                            Span::styled(
                                format!("(\"{}\")", child_task.task.title),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }
            }
        }

        if edge_count == 0 {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    "✔ No dependency blockers. All proposed tasks can execute in parallel.",
                    Style::default().fg(Color::Green),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // 3. Topological Execution Tiers
        lines.push(Line::from(vec![Span::styled(
            "─── Topological Execution Tiers ───────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        let tiers = Self::compute_topological_tiers(state);
        for (tier_idx, tier) in tiers.iter().enumerate() {
            let tier_label = match tier_idx {
                0 => "Tier 0 (Root Tasks: 0 blockers)".to_string(),
                1 => "Tier 1 (Waiting on Tier 0)".to_string(),
                n => format!("Tier {n} (Waiting on Tier {})", n - 1),
            };

            lines.push(Line::from(vec![Span::styled(
                format!("── {tier_label} ──────────────────────────────────────"),
                Style::default().fg(Color::DarkGray),
            )]));

            for task_id in tier {
                if let Some(&idx) = id_to_idx.get(task_id) {
                    let task_item = &state.review_tasks[idx];
                    let (status_str, status_style) = Self::task_status_style(task_item);
                    let is_selected = idx == state.selected_task_index;
                    let cursor = if is_selected { "▶ " } else { "  " };

                    let dep_desc = if task_item.dependencies.is_empty() {
                        " (Root / Unblocked)".to_string()
                    } else {
                        let names: Vec<String> = task_item
                            .dependencies
                            .iter()
                            .filter_map(|d| {
                                id_to_idx
                                    .get(&d.depends_on_id)
                                    .map(|&i| state.review_tasks[i].task.short_id.clone())
                            })
                            .collect();
                        format!(" (Waits on: {})", names.join(", "))
                    };

                    lines.push(Line::from(vec![
                        Span::styled(cursor, Style::default().fg(Color::Yellow)),
                        Span::styled(
                            format!("[{}: {}] ", task_item.task.short_id, status_str),
                            status_style,
                        ),
                        Span::styled(
                            task_item.task.title.clone(),
                            if is_selected {
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            },
                        ),
                        Span::styled(dep_desc, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));

        // 4. Hierarchical Execution Tree (Recursive Traversal: A -> B -> C -> D)
        lines.push(Line::from(vec![Span::styled(
            "─── Hierarchical Execution Tree (Recursive Traversal) ───────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        let mut visited_path = HashSet::new();
        for root in &root_tasks {
            Self::render_tree_node(
                root.task.id,
                0,
                &mut visited_path,
                &mut lines,
                "",
                true,
                None,
                state,
                &id_to_idx,
                &dependents_map,
            );
            lines.push(Line::from(""));
        }

        // 5. Status Legend & Navigation Shortcuts
        lines.push(Line::from(vec![Span::styled(
            "─── Navigation & Status Legend ────────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));
        lines.push(Line::from(vec![
            Span::styled("Keys: ", Style::default().fg(Color::DarkGray)),
            Span::styled("[[/p] Jump Blocker  ", Style::default().fg(Color::Cyan)),
            Span::styled("[]/d] Jump Dependent  ", Style::default().fg(Color::Cyan)),
            Span::styled("[PgUp/PgDn] Scroll  ", Style::default().fg(Color::Cyan)),
            Span::styled("[G] Cards View", Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [Approved] ", Style::default().fg(Color::Green)),
            Span::styled("  [HumanReview] ", Style::default().fg(Color::Yellow)),
            Span::styled("  [Executing] ", Style::default().fg(Color::Cyan)),
            Span::styled("  [Completed] ", Style::default().fg(Color::Blue)),
            Span::styled("  [Blocked] ", Style::default().fg(Color::Magenta)),
            Span::styled("  [Failed/Rejected] ", Style::default().fg(Color::Red)),
        ]));

        lines
    }
}
