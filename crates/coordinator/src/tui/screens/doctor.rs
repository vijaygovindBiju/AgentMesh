use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::doctor::{DoctorStatus, Readiness};
use crate::tui::state::AppState;

pub struct DoctorScreen;

impl DoctorScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" AgentMesh Doctor ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let Some(report) = &state.doctor_report else {
            let message = if state.doctor_running {
                "Running safe checks... (press R to re-run)"
            } else {
                "Press R to run safe checks."
            };
            frame.render_widget(Paragraph::new(message), inner);
            return;
        };

        let readiness_style = match report.readiness {
            Readiness::Ready => Style::default().fg(Color::Green),
            Readiness::Degraded => Style::default().fg(Color::Yellow),
            Readiness::Blocked => Style::default().fg(Color::Red),
        };
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" Readiness: {:?} ", report.readiness),
                readiness_style.add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " required failures: {} | warnings: {} | generated: {}",
                report.required_failures, report.warnings, report.generated_at
            )),
        ]))
        .wrap(Wrap { trim: true });

        let header_area = Rect {
            height: 2.min(inner.height),
            ..inner
        };
        frame.render_widget(header, header_area);

        let rows = report.checks.iter().enumerate().map(|(index, check)| {
            let (label, color) = match check.status {
                DoctorStatus::Pass => ("PASS", Color::Green),
                DoctorStatus::Warn => ("WARN", Color::Yellow),
                DoctorStatus::Fail => ("FAIL", Color::Red),
                DoctorStatus::Skip => ("SKIP", Color::DarkGray),
            };
            let selected = if index == state.doctor_selected_index {
                ">"
            } else {
                " "
            };
            Row::new(vec![
                selected.to_string(),
                label.to_string(),
                check.id.clone(),
                check.summary.clone(),
            ])
            .style(Style::default().fg(color))
        });
        let table_area = Rect {
            y: inner.y.saturating_add(header_area.height),
            height: inner.height.saturating_sub(header_area.height),
            ..inner
        };
        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(6),
                Constraint::Length(32),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec!["", "Status", "Check", "Summary"])
                .style(Style::default().fg(Color::Cyan)),
        )
        .block(Block::default().borders(Borders::BOTTOM))
        .column_spacing(1);
        frame.render_widget(table, table_area);

        if let Some(check) = report.checks.get(state.doctor_selected_index) {
            let detail = format!(
                "Evidence: {}\n{}",
                check.evidence,
                check
                    .remediation
                    .as_deref()
                    .map(|r| format!("Remediation: {r}"))
                    .unwrap_or_else(|| "No remediation required.".to_string())
            );
            let detail_area = Rect {
                x: inner.x.saturating_add(inner.width / 2),
                y: inner.y.saturating_add(inner.height.saturating_sub(5)),
                width: inner.width / 2,
                height: 5.min(inner.height),
            };
            frame.render_widget(
                Paragraph::new(detail)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Selected check "),
                    )
                    .wrap(Wrap { trim: true }),
                detail_area,
            );
        }
    }
}
