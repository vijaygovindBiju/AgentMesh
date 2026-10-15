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
                " Task Dependency Graph (DAG) & Execution Pipeline [G: Task Cards] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if state.review_tasks.is_empty() {
            let empty = Paragraph::new("No tasks proposed yet. Go to [1: Input] to submit a project.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, inner_area);
            return;
        }

        let lines = Self::build_dag_lines(state);

        // Compute vertical scroll if content exceeds inner_area.height
        let total_lines = lines.len();
        let visible_height = inner_area.height as usize;
        let scroll_offset = if total_lines > visible_height {
            // Keep selected task visible if possible
            0
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
        // Set of tasks that have incoming dependencies
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

        // 3. Hierarchical Execution Tree
        lines.push(Line::from(vec![Span::styled(
            "─── Topological Execution Tree (Root ➔ Leaf) ──────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        for root in &root_tasks {
            let root_idx = id_to_idx[&root.task.id];
            let is_selected = root_idx == state.selected_task_index;
            let (status_str, status_style) = Self::task_status_style(root);

            let cursor = if is_selected { "▶ " } else { "  " };

            lines.push(Line::from(vec![
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("[{}: {}] ", root.task.short_id, status_str),
                    status_style,
                ),
                Span::styled(
                    root.task.title.clone(),
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::styled(
                    if root.dependencies.is_empty() {
                        " (Root: Immediate Execution)".to_string()
                    } else {
                        String::new()
                    },
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            // Print downstream dependents
            if let Some(children) = dependents_map.get(&root.task.id) {
                let count = children.len();
                for (c_idx, (child_id, kind)) in children.iter().enumerate() {
                    let branch = if c_idx + 1 == count {
                        "   └──"
                    } else {
                        "   ├──"
                    };

                    if let Some(&target_idx) = id_to_idx.get(child_id) {
                        let target = &state.review_tasks[target_idx];
                        let (child_status, child_style) = Self::task_status_style(target);
                        let child_selected = target_idx == state.selected_task_index;
                        let child_cursor = if child_selected { "▶" } else { " " };

                        let edge_text = match kind {
                            DependencyKind::Blocks => "blocks──▶",
                            DependencyKind::RelatesTo => "relates──▶",
                        };

                        lines.push(Line::from(vec![
                            Span::styled(branch, Style::default().fg(Color::DarkGray)),
                            Span::styled(edge_text, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                format!("{child_cursor}[{}: {}] ", target.task.short_id, child_status),
                                child_style,
                            ),
                            Span::styled(
                                target.task.title.clone(),
                                if child_selected {
                                    Style::default()
                                        .fg(Color::White)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(Color::White)
                                },
                            ),
                        ]));
                    }
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled("   └── ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "(No downstream dependents)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }

        // 4. Status Legend
        lines.push(Line::from(vec![Span::styled(
            "─── Status Color Legend ───────────────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));
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
