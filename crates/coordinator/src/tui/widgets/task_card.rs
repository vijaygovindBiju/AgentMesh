use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::domain::{ApprovalStatus, TaskStatus};
use crate::tui::state::ReviewTaskState;

pub struct TaskCardWidget;

impl TaskCardWidget {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        item: &ReviewTaskState,
        is_focused: bool,
        is_selected: bool,
    ) {
        // 1. High-Contrast Combined Focus & Multi-Select Indicator
        let (border_type, border_style, block_title) = if is_focused && is_selected {
            (
                BorderType::Thick,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                Some(Line::from(vec![Span::styled(
                    " [SELECTED] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )])),
            )
        } else if is_focused {
            (
                BorderType::Thick,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                None,
            )
        } else if is_selected {
            (
                BorderType::Double,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                None,
            )
        } else {
            (
                BorderType::Plain,
                Style::default().fg(Color::DarkGray),
                None,
            )
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(border_style);

        if let Some(title) = block_title {
            block = block.title(title);
        }

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if inner_area.height == 0 || inner_area.width == 0 {
            return;
        }

        // Render card content lines
        let mut lines = Vec::new();

        // 1. Header: Prefix + [ID] + Title (with Ellipsis Truncation)
        let (prefix_str, prefix_spans) = if is_focused && is_selected {
            (
                "▶[✓] ",
                vec![
                    Span::styled(
                        "▶",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "[✓] ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
            )
        } else if is_focused {
            (
                "▶ ",
                vec![Span::styled(
                    "▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )],
            )
        } else if is_selected {
            (
                "  [✓] ",
                vec![Span::styled(
                    "  [✓] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )],
            )
        } else {
            ("  ", vec![Span::raw("  ")])
        };

        let short_id_str = format!("[{}] ", item.task.short_id);
        let prefix_len = prefix_str.chars().count();
        let short_id_len = short_id_str.chars().count();
        let reserved = prefix_len + short_id_len;
        let available_for_title = (inner_area.width as usize).saturating_sub(reserved);

        let truncated_title = if item.task.title.chars().count() > available_for_title {
            if available_for_title <= 3 {
                "...".chars().take(available_for_title).collect::<String>()
            } else {
                let keep = available_for_title - 3;
                let mut s: String = item.task.title.chars().take(keep).collect();
                s.push_str("...");
                s
            }
        } else {
            item.task.title.clone()
        };

        let short_id_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let title_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);

        let mut header_spans = prefix_spans;
        header_spans.push(Span::styled(short_id_str, short_id_style));
        header_spans.push(Span::styled(truncated_title, title_style));
        lines.push(Line::from(header_spans));

        // 2. Status Badge & Review Decision
        let (status_text, status_color) = match item.human_decision {
            Some(ApprovalStatus::Approved) => (" ✔ APPROVED ", Color::Green),
            Some(ApprovalStatus::EditedAndApproved) => (" ✔ EDITED & APPROVED ", Color::Green),
            Some(ApprovalStatus::Rejected) => (" ✖ REJECTED ", Color::Red),
            None => match item.task.status {
                TaskStatus::HumanReview => (" ? HUMAN REVIEW ", Color::Yellow),
                TaskStatus::Approved => (" ⏳ APPROVED ", Color::Green),
                TaskStatus::Rejected => (" ✖ REJECTED ", Color::Red),
                TaskStatus::Executing => (" ▶ EXECUTING ", Color::Cyan),
                TaskStatus::Completed => (" ✔ COMPLETED ", Color::Green),
                TaskStatus::Failed => (" ✖ FAILED ", Color::Red),
                _ => (" ⊘ PROPOSED ", Color::DarkGray),
            },
        };

        let status_badge_span = Span::styled(
            status_text,
            Style::default()
                .bg(status_color)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        );

        let agent_name = item
            .suggested_agent_name
            .as_deref()
            .unwrap_or("Unassigned (advisory)");

        let progress_bar_span = if let Some(pct) = item.progress_percent {
            let bar = if inner_area.width >= 85 {
                let filled = (pct as usize / 10).min(10);
                let empty = 10 - filled;
                format!(" [{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty))
            } else if inner_area.width >= 45 {
                let filled = (pct as usize / 20).min(5);
                let empty = 5 - filled;
                format!(" [{}{}] {pct}%", "█".repeat(filled), "░".repeat(empty))
            } else {
                format!(" {pct}%")
            };
            Some(Span::styled(
                bar,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
        } else if item.task.status == TaskStatus::Completed {
            let bar = if inner_area.width >= 85 {
                " [██████████] 100%".to_string()
            } else if inner_area.width >= 45 {
                " [█████] 100%".to_string()
            } else {
                " 100%".to_string()
            };
            Some(Span::styled(
                bar,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            None
        };

        // 3. Overlap Warning & Dependencies Summary
        let overlap_warning_span = if !item.overlap_warnings.is_empty() {
            let unacked_count = item
                .overlap_warnings
                .iter()
                .filter(|w| !w.acknowledged)
                .count();
            if unacked_count > 0 {
                Some(Span::styled(
                    format!(" ⚠ {unacked_count} Resource Overlap Warning(s) [A to ack] "),
                    Style::default()
                        .fg(Color::Red)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Some(Span::styled(
                    format!(
                        " ✔ {} Overlap Warning(s) Acknowledged ",
                        item.overlap_warnings.len()
                    ),
                    Style::default().fg(Color::Green).bg(Color::Black),
                ))
            }
        } else {
            None
        };

        let deps_summary = if item.dependencies.is_empty() {
            "None (Root task)".to_string()
        } else {
            format!("{} blocking prerequisite(s)", item.dependencies.len())
        };

        // 4. Metadata Line Formatting: Adapt between compact (< 60 cols) and standard (>= 60 cols)
        let is_compact_width = inner_area.width < 60;
        let indent = if inner_area.width < 85 { "  " } else { "    " };

        if is_compact_width {
            // Line 2: Status: <badge>  Progress: <bar>
            let mut line2_spans = vec![Span::raw(format!("{indent}Status: ")), status_badge_span];
            if let Some(ref progress) = progress_bar_span {
                line2_spans.push(Span::raw("  Progress: "));
                line2_spans.push(progress.clone());
            }
            lines.push(Line::from(line2_spans));

            // Line 3: Agent: <name> (with dependencies preserved if height is constrained)
            if inner_area.height <= 3 {
                let mut line3_spans = vec![
                    Span::raw(format!("{indent}Agent: ")),
                    Span::styled(agent_name, Style::default().fg(Color::Magenta)),
                ];
                if let Some(ref warning) = overlap_warning_span {
                    line3_spans.push(Span::raw("  "));
                    line3_spans.push(warning.clone());
                }
                line3_spans.push(Span::raw("  Deps: "));
                line3_spans.push(Span::styled(
                    deps_summary.clone(),
                    Style::default().fg(Color::DarkGray),
                ));
                lines.push(Line::from(line3_spans));
            } else {
                // inner_area.height >= 4: Agent on Line 3, Overlap / Deps on Line 4+
                lines.push(Line::from(vec![
                    Span::raw(format!("{indent}Agent: ")),
                    Span::styled(agent_name, Style::default().fg(Color::Magenta)),
                ]));

                if inner_area.height == 4 {
                    let mut line4_spans = Vec::new();
                    if let Some(ref warning) = overlap_warning_span {
                        line4_spans.push(Span::raw(indent));
                        line4_spans.push(warning.clone());
                        line4_spans.push(Span::raw("  Deps: "));
                        line4_spans.push(Span::styled(
                            deps_summary.clone(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    } else {
                        line4_spans.push(Span::raw(format!("{indent}Deps: ")));
                        line4_spans.push(Span::styled(
                            deps_summary.clone(),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    lines.push(Line::from(line4_spans));
                } else {
                    // inner_area.height >= 5
                    if let Some(ref warning) = overlap_warning_span {
                        lines.push(Line::from(vec![Span::raw(indent), warning.clone()]));
                    }
                    lines.push(Line::from(vec![
                        Span::raw(format!("{indent}Deps: ")),
                        Span::styled(deps_summary.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        } else {
            // Standard / Wide screens (>= 60 cols): Line 2 has cleanly aligned Status, Agent, Progress
            let mut line2_spans = vec![
                Span::raw(format!("{indent}Status: ")),
                status_badge_span,
                Span::raw("  Agent: "),
                Span::styled(agent_name, Style::default().fg(Color::Magenta)),
            ];
            if let Some(ref progress) = progress_bar_span {
                line2_spans.push(Span::raw("  Progress: "));
                line2_spans.push(progress.clone());
            }
            lines.push(Line::from(line2_spans));

            // Line 3: Overlap warning and Dependencies preview
            if inner_area.height <= 3 {
                if let Some(ref warning) = overlap_warning_span {
                    lines.push(Line::from(vec![
                        Span::raw(indent),
                        warning.clone(),
                        Span::raw("  Deps: "),
                        Span::styled(deps_summary.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw(format!("{indent}Deps: ")),
                        Span::styled(deps_summary.clone(), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            } else {
                // inner_area.height >= 4: Overlap on Line 3, Deps on Line 4
                if let Some(ref warning) = overlap_warning_span {
                    lines.push(Line::from(vec![Span::raw(indent), warning.clone()]));
                }
                lines.push(Line::from(vec![
                    Span::raw(format!("{indent}Deps: ")),
                    Span::styled(deps_summary.clone(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner_area);
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

    use crate::domain::{
        DependencyKind, OverlapSeverity, OverlapWarning, Task, TaskDependency, TaskStatus,
    };
    use crate::tui::state::ReviewTaskState;

    fn sample_task(short_id: &str, title: &str, status: TaskStatus) -> ReviewTaskState {
        ReviewTaskState {
            task: Task {
                id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                short_id: short_id.to_string(),
                title: title.to_string(),
                description: format!("Description for {short_id}"),
                affected_resources: json!(["crates/coordinator/src/main.rs"]),
                status,
                assigned_agent_id: None,
                estimated_size: Some("M".to_string()),
                proposal_id: Uuid::new_v4(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            suggested_agent_name: Some("WorkerAgent-Backend".to_string()),
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        }
    }

    fn render_to_string(
        width: u16,
        height: u16,
        item: &ReviewTaskState,
        is_focused: bool,
        is_selected: bool,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                TaskCardWidget::render(frame, frame.area(), item, is_focused, is_selected);
            })
            .unwrap();
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
    fn test_task_card_focus_and_selection_indicators() {
        let item = sample_task("PLAN-1", "Test Task", TaskStatus::HumanReview);

        // 1. Both focused and selected
        let out_both = render_to_string(80, 5, &item, true, true);
        assert!(
            out_both.contains("▶[✓] "),
            "Must contain combined indicator '▶[✓] '. Buffer:\n{out_both}"
        );
        assert!(
            out_both.contains("[SELECTED]"),
            "Must contain [SELECTED] title badge. Buffer:\n{out_both}"
        );
        assert!(
            out_both.contains("┏"),
            "Thick border character expected for focused+selected. Buffer:\n{out_both}"
        );

        // 2. Only focused
        let out_focused = render_to_string(80, 5, &item, true, false);
        assert!(
            out_focused.contains("▶ "),
            "Must contain focus indicator '▶ '. Buffer:\n{out_focused}"
        );
        assert!(
            !out_focused.contains("▶[✓] "),
            "Must NOT contain selection mark when only focused"
        );
        assert!(
            !out_focused.contains("[SELECTED]"),
            "Must NOT contain [SELECTED] badge when only focused"
        );
        assert!(
            out_focused.contains("┏"),
            "Thick border expected for focused. Buffer:\n{out_focused}"
        );

        // 3. Only selected
        let out_selected = render_to_string(80, 5, &item, false, true);
        assert!(
            out_selected.contains("  [✓] "),
            "Must contain select indicator '  [✓] '. Buffer:\n{out_selected}"
        );
        assert!(
            !out_selected.contains("▶"),
            "Must NOT contain focus cursor '▶' when only selected"
        );
        assert!(
            out_selected.contains("╔"),
            "Double border expected for selected. Buffer:\n{out_selected}"
        );

        // 4. Neither
        let out_neither = render_to_string(80, 5, &item, false, false);
        assert!(
            !out_neither.contains("▶"),
            "Must not contain '▶' when neither"
        );
        assert!(
            !out_neither.contains("[✓]"),
            "Must not contain '[✓]' when neither"
        );
        assert!(
            out_neither.contains("┌"),
            "Plain border expected when neither. Buffer:\n{out_neither}"
        );
    }

    #[test]
    fn test_task_card_title_ellipsis_truncation() {
        let long_title =
            "This is a very long title that will certainly exceed the width of any compact card pane";
        let item = sample_task("PLAN-LONG", long_title, TaskStatus::HumanReview);

        // Compact width (35 cols total -> inner width 33 cols)
        let out_compact = render_to_string(35, 5, &item, true, false);
        assert!(
            out_compact.contains("[PLAN-LONG]"),
            "Short ID must be preserved"
        );
        assert!(
            out_compact.contains("..."),
            "Long title must end with ellipsis '...'"
        );
        assert!(
            !out_compact.contains("compact card pane"),
            "End of original title must be truncated"
        );

        // Wide width (120 cols total)
        let out_wide = render_to_string(120, 5, &item, true, false);
        assert!(
            out_wide.contains(long_title),
            "Full title must fit without truncation on wide card"
        );
    }

    #[test]
    fn test_task_card_metadata_compact_width_under_60() {
        let mut item = sample_task("PLAN-2", "Compact Metadata Test", TaskStatus::Executing);
        item.progress_percent = Some(65);

        // Width 50 (< 60 cols)
        let out = render_to_string(50, 5, &item, false, false);
        // Line 2 has Status and Progress
        assert!(out.contains("Status:"), "Line 2 must include Status: label");
        assert!(out.contains("EXECUTING"), "Status badge must be visible");
        assert!(
            out.contains("Progress:"),
            "Line 2 must include Progress: label"
        );
        assert!(out.contains("65%"), "Progress percentage must be visible");
        // Line 3 has Agent
        assert!(out.contains("Agent:"), "Line 3 must include Agent: label");
        assert!(
            out.contains("WorkerAgent-Backend"),
            "Agent name must be visible"
        );
    }

    #[test]
    fn test_task_card_metadata_standard_width_over_60() {
        let mut item = sample_task("PLAN-3", "Standard Metadata Test", TaskStatus::Completed);
        item.progress_percent = Some(100);

        // Width 80 (>= 60 cols)
        let out = render_to_string(80, 5, &item, false, false);
        assert!(out.contains("Status:"), "Must include Status: label");
        assert!(out.contains("COMPLETED"), "Must include status badge");
        assert!(out.contains("Agent:"), "Must include Agent: label");
        assert!(
            out.contains("WorkerAgent-Backend"),
            "Must include agent name"
        );
        assert!(out.contains("Progress:"), "Must include Progress: label");
        assert!(out.contains("100%"), "Must include progress bar");
    }

    #[test]
    fn test_task_card_overlap_warning_and_dependencies_preservation() {
        let mut item = sample_task("PLAN-4", "Overlap & Deps Test", TaskStatus::HumanReview);
        item.dependencies = vec![
            TaskDependency {
                dependent_id: item.task.id,
                depends_on_id: Uuid::new_v4(),
                kind: DependencyKind::Blocks,
            },
            TaskDependency {
                dependent_id: item.task.id,
                depends_on_id: Uuid::new_v4(),
                kind: DependencyKind::Blocks,
            },
        ];
        item.overlap_warnings.push(OverlapWarning {
            id: Uuid::new_v4(),
            project_id: item.task.project_id,
            task_ids: json!([item.task.id]),
            resource: "crates/coordinator/src/main.rs".to_string(),
            severity: OverlapSeverity::Warning,
            acknowledged: false,
            created_at: Utc::now(),
        });

        // 1. Compact card (height 5 -> inner_area.height 3)
        // Must preserve BOTH the overlap warning and dependencies preview
        let out_compact = render_to_string(100, 5, &item, true, false);
        assert!(
            out_compact.contains("⚠ 1 Resource Overlap Warning(s) [A to ack]"),
            "Overlap warning must be visible in compact height"
        );
        assert!(
            out_compact.contains("2 blocking prerequisite(s)"),
            "Dependencies preview must be preserved and visible in compact height"
        );

        // 2. Expanded card (height 7 -> inner_area.height 5)
        let out_expanded = render_to_string(100, 7, &item, true, false);
        assert!(
            out_expanded.contains("⚠ 1 Resource Overlap Warning(s) [A to ack]"),
            "Overlap warning must be visible in expanded height"
        );
        assert!(
            out_expanded.contains("2 blocking prerequisite(s)"),
            "Dependencies preview must be visible in expanded height"
        );

        // 3. Acknowledged warning
        item.overlap_warnings[0].acknowledged = true;
        let out_acked = render_to_string(100, 5, &item, true, false);
        assert!(
            out_acked.contains("✔ 1 Overlap Warning(s) Acknowledged"),
            "Acknowledged warning must be visible"
        );
        assert!(
            out_acked.contains("2 blocking prerequisite(s)"),
            "Dependencies preview must remain visible after ack"
        );
    }

    #[test]
    fn test_task_card_zero_or_minimal_dimensions_safety() {
        let item = sample_task("PLAN-0", "Safe Area", TaskStatus::Proposed);
        // Area with 0 height or 0 width should not panic
        let _ = render_to_string(0, 0, &item, false, false);
        let _ = render_to_string(1, 1, &item, false, false);
        let _ = render_to_string(2, 2, &item, false, false);
        let _ = render_to_string(10, 3, &item, false, false);
    }
}
