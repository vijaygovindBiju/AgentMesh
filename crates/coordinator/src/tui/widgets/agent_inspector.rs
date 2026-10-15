use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::domain::AgentStatus;
use crate::tui::state::AppState;

pub struct AgentInspectorWidget;

impl AgentInspectorWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        let agent = match state.agents.get(state.selected_agent_index) {
            Some(a) => a,
            None => return,
        };

        // Center a modal dialog in the area
        let popup_width = (area.width * 75 / 100).clamp(60, 90).min(area.width);
        let popup_height = (area.height * 75 / 100).clamp(18, 28).min(area.height);

        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        // Clear beneath modal
        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Connected Agent Capability Profile & Hardware Telemetry [Esc/C: Close] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let (name, owner) = if let Some(idx) = agent.human_owner.find(" (") {
            let name = &agent.human_owner[..idx];
            let owner = agent.human_owner[idx + 2..].trim_end_matches(')');
            (name.to_string(), owner.to_string())
        } else {
            (
                format!("agent-{}", &agent.id.to_string()[..8]),
                agent.human_owner.clone(),
            )
        };

        let (status_str, status_color) = match agent.status {
            AgentStatus::Idle => ("IDLE", Color::Green),
            AgentStatus::Busy => ("BUSY", Color::Yellow),
            AgentStatus::Blocked => ("BLOCKED", Color::Magenta),
            AgentStatus::Error => ("ERROR", Color::Red),
            AgentStatus::Offline => ("OFFLINE", Color::DarkGray),
        };

        let profile = agent.capabilities_profile();
        let capabilities = agent.capabilities_list();

        let mut lines = Vec::new();

        // 1. Identity & Status Bar
        lines.push(Line::from(vec![
            Span::styled(" Agent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  Owner: ", Style::default().fg(Color::DarkGray)),
            Span::styled(owner, Style::default().fg(Color::Yellow)),
            Span::styled("  |  Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(" {status_str} "),
                Style::default()
                    .bg(status_color)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  Adapter: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:?}", agent.adapter_type),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled(" ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(agent.id.to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled("  |  NATS: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&agent.nats_subject, Style::default().fg(Color::Cyan)),
        ]));

        lines.push(Line::from(""));

        // 2. Hardware & Runtime Telemetry
        lines.push(Line::from(vec![Span::styled(
            "─── System Runtime & Hardware ──────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        if let Some(ref prof) = profile {
            let agy_ver = prof
                .runtime
                .agy_version
                .as_deref()
                .unwrap_or("None (Mock runtime)");
            let mem_str = prof
                .runtime
                .memory_mb
                .map(|m| format!("{m} MB"))
                .unwrap_or_else(|| "N/A".to_string());

            lines.push(Line::from(vec![
                Span::styled("  OS: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}/{}", prof.runtime.os, prof.runtime.arch),
                    Style::default().fg(Color::White),
                ),
                Span::styled("   CPU Cores: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    prof.runtime.cpu_count.to_string(),
                    Style::default().fg(Color::White),
                ),
                Span::styled("   RAM: ", Style::default().fg(Color::Cyan)),
                Span::styled(mem_str, Style::default().fg(Color::White)),
                Span::styled("   Antigravity CLI: ", Style::default().fg(Color::Cyan)),
                Span::styled(agy_ver, Style::default().fg(Color::Yellow)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  OS: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
                    Style::default().fg(Color::White),
                ),
                Span::styled("   CPU Cores: ", Style::default().fg(Color::Cyan)),
                Span::styled("Detected via Host", Style::default().fg(Color::White)),
                Span::styled("   Adapter Runtime: ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{:?}", agent.adapter_type),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // 3. Installed Languages & Toolchains
        lines.push(Line::from(vec![Span::styled(
            "─── Installed Languages & Toolchains ───────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        if let Some(ref prof) = profile {
            if prof.languages.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  None explicitly reported",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for lang in &prof.languages {
                    let ver = lang.version.as_deref().unwrap_or("latest");
                    let frameworks_str = if lang.frameworks.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", lang.frameworks.join(", "))
                    };

                    lines.push(Line::from(vec![
                        Span::styled("  • ", Style::default().fg(Color::Green)),
                        Span::styled(
                            lang.name.clone(),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!(" ({ver})"), Style::default().fg(Color::White)),
                        Span::styled(frameworks_str, Style::default().fg(Color::Cyan)),
                    ]));
                }
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Green)),
                Span::styled(
                    "Standard host toolchains active",
                    Style::default().fg(Color::Green),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // 4. Developer Tools & CLIs
        lines.push(Line::from(vec![Span::styled(
            "─── Developer Tools & CLI Utilities ────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        if let Some(ref prof) = profile {
            if prof.tools.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No developer tools detected",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                let tool_spans: Vec<Span> = prof
                    .tools
                    .iter()
                    .flat_map(|t| {
                        let ver = t.version.as_deref().unwrap_or("detected");
                        vec![
                            Span::styled(
                                format!(" [{}: {}] ", t.name, ver),
                                Style::default().fg(Color::Cyan),
                            ),
                            Span::raw(" "),
                        ]
                    })
                    .collect();
                lines.push(Line::from(tool_spans));
            }
        } else {
            lines.push(Line::from(vec![Span::styled(
                "  Standard mesh utilities (cargo, git, nats, sqlx)",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        lines.push(Line::from(""));

        // 5. Domain & Skill Tags
        lines.push(Line::from(vec![Span::styled(
            "─── Domain & Skill Tags ────────────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )]));

        if capabilities.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No domain tags assigned",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let tag_spans: Vec<Span> = capabilities
                .iter()
                .flat_map(|tag| {
                    vec![
                        Span::styled(
                            format!(" #{tag} "),
                            Style::default()
                                .bg(Color::DarkGray)
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                    ]
                })
                .collect();
            lines.push(Line::from(tag_spans));
        }

        lines.push(Line::from(""));

        // Footer instructions
        lines.push(Line::from(vec![Span::styled(
            "Press [Esc], [C], or [Enter] to dismiss this inspector dialog.",
            Style::default().fg(Color::DarkGray),
        )]));

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }
}
