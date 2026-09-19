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
    pub fn render(frame: &mut Frame, area: Rect, item: &ReviewTaskState, is_selected: bool) {
        let border_style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if is_selected {
                BorderType::Thick
            } else {
                BorderType::Plain
            })
            .border_style(border_style);

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        // Render card content lines
        let mut lines = Vec::new();

        // 1. Header: [PLAN-1] Title
        let prefix = if is_selected { "▶ " } else { "  " };
        let short_id_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let title_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Cyan)),
            Span::styled(format!("[{}] ", item.task.short_id), short_id_style),
            Span::styled(item.task.title.clone(), title_style),
        ]));

        // 2. Status Badge & Review Decision
        let (status_text, status_color) = match item.human_decision {
            Some(ApprovalStatus::Approved) => (" APPROVED ", Color::Green),
            Some(ApprovalStatus::EditedAndApproved) => (" EDITED & APPROVED ", Color::Green),
            Some(ApprovalStatus::Rejected) => (" REJECTED ", Color::Red),
            None => match item.task.status {
                TaskStatus::HumanReview => (" HUMAN REVIEW ", Color::Yellow),
                TaskStatus::Approved => (" APPROVED ", Color::Green),
                TaskStatus::Rejected => (" REJECTED ", Color::Red),
                TaskStatus::Executing => (" EXECUTING ", Color::Cyan),
                TaskStatus::Completed => (" COMPLETED ", Color::Blue),
                TaskStatus::Failed => (" FAILED ", Color::Red),
                _ => (" PROPOSED ", Color::DarkGray),
            },
        };

        let agent_name = item
            .suggested_agent_name
            .as_deref()
            .unwrap_or("Unassigned (advisory)");

        lines.push(Line::from(vec![
            Span::raw("    Status: "),
            Span::styled(
                status_text,
                Style::default()
                    .bg(status_color)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  Agent: "),
            Span::styled(agent_name, Style::default().fg(Color::Magenta)),
        ]));

        // 3. Overlap warning banner (if any)
        if !item.overlap_warnings.is_empty() {
            let unacked_count = item
                .overlap_warnings
                .iter()
                .filter(|w| !w.acknowledged)
                .count();
            if unacked_count > 0 {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!(" ⚠ {unacked_count} Resource Overlap Warning(s) [A to ack] "),
                        Style::default()
                            .fg(Color::Red)
                            .bg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!(
                            " ✔ {} Overlap Warning(s) Acknowledged ",
                            item.overlap_warnings.len()
                        ),
                        Style::default().fg(Color::Green).bg(Color::Black),
                    ),
                ]));
            }
        }

        // 4. Dependencies preview
        let deps_summary = if item.dependencies.is_empty() {
            "None (Root task)".to_string()
        } else {
            format!("{} blocking prerequisite(s)", item.dependencies.len())
        };
        lines.push(Line::from(vec![
            Span::raw("    Deps: "),
            Span::styled(deps_summary, Style::default().fg(Color::DarkGray)),
        ]));

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner_area);
    }
}
