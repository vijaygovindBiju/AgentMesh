use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::domain::{ApprovalStatus, OverlapSeverity};
use crate::tui::state::{AppState, InputMode, PlanReviewFocus};

pub struct DetailsPaneWidget;

impl DetailsPaneWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let is_focused = state.plan_review_focus == PlanReviewFocus::TaskInspector;
        let (border_type, border_color) = if is_focused {
            (BorderType::Thick, Color::Cyan)
        } else {
            (BorderType::Rounded, Color::DarkGray)
        };

        let title_text = match (is_focused, area.width) {
            (true, w) if w >= 59 => " [▶ FOCUSED] Task Details & Inspector [Enter to toggle] ",
            (true, w) if w >= 38 => " [▶ FOCUSED] Task Details [Enter] ",
            (true, _) => " [▶ FOCUSED] Inspector ",
            (false, w) if w >= 48 => " Task Details & Inspector [Enter to toggle] ",
            (false, w) if w >= 38 => " Task Details [Enter] ",
            (false, _) => " Inspector ",
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let Some(selected) = state.selected_task() else {
            let empty_p = Paragraph::new("No tasks available to inspect.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty_p, inner_area);
            return;
        };

        // Content-adaptive section height calculation
        let dep_count = selected.dependencies.len();
        let dep_len = if dep_count == 0 {
            3
        } else {
            (dep_count as u16 + 2).min(8)
        };

        let resources = selected.task.resources();
        let res_count = resources.len();
        let res_len = if res_count == 0 {
            3
        } else {
            (res_count as u16 + 2).min(8)
        };

        let warn_count = selected.overlap_warnings.len();
        let warn_len = if warn_count == 0 {
            3
        } else {
            (warn_count as u16 + 2).min(6)
        };

        let desc_text = if state.input_mode == InputMode::EditingDescription {
            let cursor = state.edit_cursor.min(state.edit_buffer.len());
            let safe_cursor = if state.edit_buffer.is_char_boundary(cursor) {
                cursor
            } else {
                state.edit_buffer.len()
            };
            let mut s = String::with_capacity(state.edit_buffer.len() + 1);
            s.push_str(&state.edit_buffer[..safe_cursor]);
            s.push('_');
            s.push_str(&state.edit_buffer[safe_cursor..]);
            s
        } else {
            selected.task.description.clone()
        };

        // Calculate natural lines needed across inner width (inner_area.width.saturating_sub(2))
        let desc_inner_width = (inner_area.width.saturating_sub(2) as usize).max(1);
        let _desc_natural_lines: usize = desc_text
            .lines()
            .map(|line| {
                let count = line.chars().count();
                if count == 0 {
                    1
                } else {
                    count.div_ceil(desc_inner_width)
                }
            })
            .sum::<usize>()
            .max(1);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),        // Metadata: 3 lines
                Constraint::Min(4), // Description: absorbs remaining vertical space (Min 4)
                Constraint::Length(dep_len), // Dependencies: 3 if empty, else (dep_count + 2).min(8)
                Constraint::Length(res_len), // Affected Resources: 3 if empty, else (res_count + 2).min(8)
                Constraint::Length(warn_len), // Overlap Warnings: 3 if empty, else (warn_count + 2).min(6)
            ])
            .split(inner_area);

        // 1. Task Metadata
        let mut meta_lines = Vec::new();
        meta_lines.push(Line::from(vec![
            Span::styled("Task ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("[{}] ", selected.task.short_id),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                selected.task.title.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let agent = selected
            .suggested_agent_name
            .as_deref()
            .unwrap_or("Unassigned");
        let decision_str = match selected.human_decision {
            Some(ApprovalStatus::Approved) => "Approved by Operator",
            Some(ApprovalStatus::EditedAndApproved) => "Edited & Approved by Operator",
            Some(ApprovalStatus::Rejected) => "Rejected by Operator",
            None => "Pending Human Review",
        };

        meta_lines.push(Line::from(vec![
            Span::styled("Suggested Agent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(agent, Style::default().fg(Color::Magenta)),
            Span::styled("  |  Review State: ", Style::default().fg(Color::DarkGray)),
            Span::styled(decision_str, Style::default().fg(Color::Yellow)),
        ]));

        let meta_p = Paragraph::new(meta_lines).wrap(Wrap { trim: true });
        frame.render_widget(meta_p, chunks[0]);

        // 2. Description / Inline Editor
        let desc_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if state.input_mode == InputMode::EditingDescription {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .title(if state.input_mode == InputMode::EditingDescription {
                " Description (EDITING - [Enter] confirm & approve, [Esc] cancel) "
            } else {
                " Description [Press 'E' to edit inline] "
            });

        let desc_p = Paragraph::new(desc_text)
            .block(desc_block)
            .wrap(Wrap { trim: true })
            .scroll((state.inspector_scroll_offset as u16, 0))
            .style(if state.input_mode == InputMode::EditingDescription {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            });
        frame.render_widget(desc_p, chunks[1]);

        // 3. Dependencies
        let deps_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Dependencies (Prerequisites) ");

        let dep_lines: Vec<Line> = if selected.dependencies.is_empty() {
            vec![Line::from(Span::styled(
                "  None (Task can execute immediately once approved)",
                Style::default().fg(Color::Green),
            ))]
        } else {
            selected
                .dependencies
                .iter()
                .map(|d| {
                    let (target_id_str, target_title) = if let Some(dep_task) = state
                        .review_tasks
                        .iter()
                        .find(|t| t.task.id == d.depends_on_id)
                    {
                        (
                            format!("[{}] ", dep_task.task.short_id),
                            dep_task.task.title.clone(),
                        )
                    } else {
                        (
                            format!("[{}] ", &d.depends_on_id.to_string()[..8]),
                            "External Prerequisite".to_string(),
                        )
                    };

                    let kind_label = match d.kind {
                        crate::domain::DependencyKind::Blocks => " [Blocks] ",
                        crate::domain::DependencyKind::RelatesTo => " [RelatesTo] ",
                    };

                    Line::from(vec![
                        Span::styled("  ↳ Depends on: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            target_id_str,
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(target_title, Style::default().fg(Color::White)),
                        Span::styled(
                            kind_label,
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ])
                })
                .collect()
        };

        let deps_p = Paragraph::new(dep_lines)
            .block(deps_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(deps_p, chunks[2]);

        // 4. Affected Resources
        let res_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Affected Resources ");

        let res_lines: Vec<Line> = if resources.is_empty() {
            vec![Line::from(Span::styled(
                "  No shared file/module resources specified",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            resources
                .iter()
                .map(|r| {
                    Line::from(vec![
                        Span::styled("  • ", Style::default().fg(Color::Cyan)),
                        Span::styled(r.clone(), Style::default().fg(Color::White)),
                    ])
                })
                .collect()
        };

        let res_p = Paragraph::new(res_lines)
            .block(res_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(res_p, chunks[3]);

        // 5. Overlap Warnings Banner
        let has_unacked = selected.overlap_warnings.iter().any(|w| !w.acknowledged);
        let warn_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if has_unacked {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else if !selected.overlap_warnings.is_empty() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .title(if has_unacked {
                " Resource Overlap Analysis [Press 'A' to acknowledge] "
            } else {
                " Resource Overlap Analysis "
            });

        let warn_lines: Vec<Line> = if selected.overlap_warnings.is_empty() {
            vec![Line::from(Span::styled(
                "  ✔ No concurrent resource collisions detected",
                Style::default().fg(Color::Green),
            ))]
        } else {
            selected
                .overlap_warnings
                .iter()
                .map(|w| {
                    if w.acknowledged {
                        Line::from(vec![
                            Span::styled("  [ACKNOWLEDGED] ", Style::default().fg(Color::Green)),
                            Span::styled(
                                format!("Resource '{}' is shared (conflict accepted)", w.resource),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ])
                    } else {
                        let sev_style = match w.severity {
                            OverlapSeverity::Critical => {
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                            }
                            OverlapSeverity::Warning => Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                            OverlapSeverity::Info => Style::default().fg(Color::Blue),
                        };
                        Line::from(vec![
                            Span::styled(format!("  [{:?}] ", w.severity), sev_style),
                            Span::styled(
                                format!("Resource '{}' is shared with other task(s)", w.resource),
                                Style::default().fg(Color::White),
                            ),
                        ])
                    }
                })
                .collect()
        };

        let warn_p = Paragraph::new(warn_lines)
            .block(warn_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(warn_p, chunks[4]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;
    use uuid::Uuid;

    use crate::domain::{DependencyKind, OverlapWarning, Task, TaskDependency, TaskStatus};
    use crate::tui::state::ReviewTaskState;

    fn sample_task(id: Uuid, short_id: &str, title: &str) -> Task {
        Task {
            id,
            project_id: Uuid::new_v4(),
            short_id: short_id.to_string(),
            title: title.to_string(),
            description: format!("Description for {short_id}"),
            affected_resources: json!(["crates/coordinator/src/main.rs"]),
            status: TaskStatus::HumanReview,
            assigned_agent_id: None,
            estimated_size: Some("M".to_string()),
            proposal_id: Uuid::new_v4(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_review_task(short_id: &str, title: &str) -> ReviewTaskState {
        let task_id = Uuid::new_v4();
        ReviewTaskState {
            task: sample_task(task_id, short_id, title),
            suggested_agent_name: Some("Agent-Alpha".to_string()),
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        }
    }

    fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn test_adaptive_block_title_breakpoints() {
        let mut state = AppState::new();
        state
            .review_tasks
            .push(sample_review_task("T-1", "Test Task"));

        // Unfocused - Width >= 48: Full title
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("Task Details & Inspector [Enter to toggle]"));

        // Unfocused - Width 47 (< 48 and >= 38): Medium title
        let backend = TestBackend::new(47, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("Task Details [Enter]"));
        assert!(!rendered.contains("Inspector [Enter to toggle]"));

        // Unfocused - Width < 38: Compact title
        let backend = TestBackend::new(25, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("Inspector"));

        // Focused - Widths 51 and 56 (terminal widths 90 and 100): select medium title
        state.plan_review_focus = PlanReviewFocus::TaskInspector;

        let backend_90 = TestBackend::new(51, 20);
        let mut terminal_90 = Terminal::new(backend_90).unwrap();
        terminal_90
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered_90 = buffer_to_string(&terminal_90);
        assert!(rendered_90.contains("[▶ FOCUSED] Task Details [Enter]"));
        assert!(!rendered_90.contains("Task Details & Inspector"));

        let backend_100 = TestBackend::new(56, 20);
        let mut terminal_100 = Terminal::new(backend_100).unwrap();
        terminal_100
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered_100 = buffer_to_string(&terminal_100);
        assert!(rendered_100.contains("[▶ FOCUSED] Task Details [Enter]"));
        assert!(!rendered_100.contains("Task Details & Inspector"));

        // Focused - Width >= 59: Full title
        let backend_full = TestBackend::new(60, 20);
        let mut terminal_full = Terminal::new(backend_full).unwrap();
        terminal_full
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered_full = buffer_to_string(&terminal_full);
        assert!(rendered_full.contains("[▶ FOCUSED] Task Details & Inspector [Enter to toggle]"));
    }

    #[test]
    fn test_focus_highlighting_and_badge() {
        let mut state = AppState::new();
        state
            .review_tasks
            .push(sample_review_task("T-1", "Test Task"));

        // When unfocused (TaskList)
        state.plan_review_focus = PlanReviewFocus::TaskList;
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(!rendered.contains("[▶ FOCUSED]"));
        assert!(rendered.contains("Task Details & Inspector"));

        // When focused (TaskInspector)
        state.plan_review_focus = PlanReviewFocus::TaskInspector;
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("[▶ FOCUSED]"));
        assert!(rendered.contains("Task Details & Inspector"));
    }

    #[test]
    fn test_content_adaptive_heights_empty_and_populated() {
        let mut state = AppState::new();
        let mut item = sample_review_task("T-1", "Test Task");
        item.task.description = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6".to_string();
        state.review_tasks.push(item);

        // Standard 120x30: description has abundant vertical space (> 6 lines visible)
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("Line 1"));
        assert!(rendered.contains("Line 4"));
        assert!(rendered.contains("None (Task can execute immediately once approved)"));

        // Populated dependencies and overlap warnings
        let dep_id = Uuid::new_v4();
        let target_task_id = state.review_tasks[0].task.id;
        let target_project_id = state.review_tasks[0].task.project_id;
        state.review_tasks[0].dependencies.push(TaskDependency {
            dependent_id: target_task_id,
            depends_on_id: dep_id,
            kind: DependencyKind::Blocks,
        });
        state.review_tasks[0].overlap_warnings.push(OverlapWarning {
            id: Uuid::new_v4(),
            project_id: target_project_id,
            task_ids: json!([target_task_id]),
            resource: "crates/coordinator/src/main.rs".to_string(),
            severity: OverlapSeverity::Critical,
            acknowledged: false,
            created_at: Utc::now(),
        });

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered = buffer_to_string(&terminal);
        assert!(rendered.contains("Depends on:"));
        assert!(rendered.contains("[Critical]"));
    }

    #[test]
    fn test_vertical_scrolling_inspector_offset() {
        let mut state = AppState::new();
        let mut item = sample_review_task("SCROLL-1", "Scrollable Task");
        item.task.description = (1..=20)
            .map(|i| format!("Desc paragraph item {i:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        state.review_tasks.push(item);

        // At offset 0, item 01 is visible
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered_0 = buffer_to_string(&terminal);
        assert!(rendered_0.contains("Desc paragraph item 01"));

        // At offset 5, item 01 is scrolled out and item 06 is visible
        state.inspector_scroll_offset = 5;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| DetailsPaneWidget::render(f, f.area(), &state))
            .unwrap();
        let rendered_5 = buffer_to_string(&terminal);
        assert!(!rendered_5.contains("Desc paragraph item 01"));
        assert!(rendered_5.contains("Desc paragraph item 06"));
    }
}
