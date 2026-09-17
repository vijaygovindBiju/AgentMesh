use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::domain::{ApprovalStatus, OverlapSeverity};
use crate::tui::state::{AppState, InputMode};

pub struct DetailsPaneWidget;

impl DetailsPaneWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Task Details & Inspector [Enter to toggle] ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let Some(selected) = state.selected_task() else {
            let empty_p = Paragraph::new("No tasks available to inspect.").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty_p, inner_area);
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Header / IDs / Agent
                Constraint::Min(6),    // Description / Edit area
                Constraint::Length(5), // Dependencies
                Constraint::Length(5), // Affected Resources
                Constraint::Length(6), // Overlap Warnings
            ])
            .split(inner_area);

        // 1. Task Metadata
        let mut meta_lines = Vec::new();
        meta_lines.push(Line::from(vec![
            Span::styled("Task ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("[{}] ", selected.task.short_id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(selected.task.title.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));

        let agent = selected.suggested_agent_name.as_deref().unwrap_or("Unassigned");
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

        let meta_p = Paragraph::new(meta_lines);
        frame.render_widget(meta_p, chunks[0]);

        // 2. Description / Inline Editor
        let desc_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if state.input_mode == InputMode::EditingDescription {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .title(if state.input_mode == InputMode::EditingDescription {
                " Description (EDITING - [Enter] confirm & approve, [Esc] cancel) "
            } else {
                " Description [Press 'E' to edit inline] "
            });

        let desc_text = if state.input_mode == InputMode::EditingDescription {
            format!("{}_", state.edit_buffer)
        } else {
            selected.task.description.clone()
        };

        let desc_p = Paragraph::new(desc_text)
            .block(desc_block)
            .wrap(Wrap { trim: false })
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
            vec![Line::from(Span::styled("  None (Task can execute immediately once approved)", Style::default().fg(Color::Green)))]
        } else {
            selected
                .dependencies
                .iter()
                .map(|d| {
                    Line::from(vec![
                        Span::styled("  ↳ Depends on Task ID: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(d.depends_on_id.to_string(), Style::default().fg(Color::Cyan)),
                        Span::styled(format!(" [{:?}]", d.kind), Style::default().fg(Color::Yellow)),
                    ])
                })
                .collect()
        };

        let deps_p = Paragraph::new(dep_lines).block(deps_block);
        frame.render_widget(deps_p, chunks[2]);

        // 4. Affected Resources
        let res_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Affected Resources ");

        let resources = selected.task.resources();
        let res_lines: Vec<Line> = if resources.is_empty() {
            vec![Line::from(Span::styled("  No shared file/module resources specified", Style::default().fg(Color::DarkGray)))]
        } else {
            resources
                .iter()
                .map(|r| Line::from(vec![
                    Span::styled("  • ", Style::default().fg(Color::Cyan)),
                    Span::styled(r.clone(), Style::default().fg(Color::White)),
                ]))
                .collect()
        };

        let res_p = Paragraph::new(res_lines).block(res_block);
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
            vec![Line::from(Span::styled("  ✔ No concurrent resource collisions detected", Style::default().fg(Color::Green)))]
        } else {
            selected
                .overlap_warnings
                .iter()
                .map(|w| {
                    if w.acknowledged {
                        Line::from(vec![
                            Span::styled("  [ACKNOWLEDGED] ", Style::default().fg(Color::Green)),
                            Span::styled(format!("Resource '{}' is shared (conflict accepted)", w.resource), Style::default().fg(Color::DarkGray)),
                        ])
                    } else {
                        let sev_style = match w.severity {
                            OverlapSeverity::Critical => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            OverlapSeverity::Warning => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            OverlapSeverity::Info => Style::default().fg(Color::Blue),
                        };
                        Line::from(vec![
                            Span::styled(format!("  [{:?}] ", w.severity), sev_style),
                            Span::styled(format!("Resource '{}' is shared with other task(s)", w.resource), Style::default().fg(Color::White)),
                        ])
                    }
                })
                .collect()
        };


        let warn_p = Paragraph::new(warn_lines).block(warn_block);
        frame.render_widget(warn_p, chunks[4]);
    }
}
