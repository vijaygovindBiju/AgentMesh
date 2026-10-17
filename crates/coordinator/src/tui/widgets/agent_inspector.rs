use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use crate::domain::{AgentStatus, HealthStatus};
use crate::tui::state::AppState;

pub struct AgentInspectorWidget;

impl AgentInspectorWidget {
    /// Dispatches between modal dialog rendering and panel rendering.
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if state.show_agent_inspector && area == frame.area() {
            Self::render_modal(frame, area, state);
        } else {
            Self::render_panel(frame, area, state);
        }
    }

    /// Renders the inspector directly into a designated panel area (e.g. in FleetInspectorScreen).
    pub fn render_panel(frame: &mut Frame, area: Rect, state: &AppState) {
        Self::render_to_area(frame, area, state, false);
    }

    /// Renders the inspector as a centered popup dialog with a Clear backdrop.
    pub fn render_modal(frame: &mut Frame, area: Rect, state: &AppState) {
        let popup_width = (area.width * 85 / 100).clamp(60, 95).min(area.width);
        let popup_height = (area.height * 85 / 100).clamp(18, 30).min(area.height);

        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        frame.render_widget(Clear, popup_area);
        Self::render_to_area(frame, popup_area, state, true);
    }

    fn render_to_area(frame: &mut Frame, area: Rect, state: &AppState, is_modal: bool) {
        let border_type = if is_modal {
            BorderType::Double
        } else {
            BorderType::Plain
        };

        let title_text = if is_modal {
            " Connected Agent Capability Profile & Hardware Telemetry [Esc/C: Close] "
        } else {
            " Connected Agent Capability Profile & Telemetry "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let agent = match state.agents.get(state.selected_agent_index) {
            Some(a) => a,
            None => {
                let empty = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "No agent selected for capability and telemetry inspection.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]);
                frame.render_widget(empty, inner);
                return;
            }
        };

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

        let (health_badge, health_style) = match agent.health_status {
            HealthStatus::Healthy => ("[Healthy]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            HealthStatus::Degraded => ("[Degraded]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            HealthStatus::Unhealthy => ("[Unhealthy]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            HealthStatus::Offline => ("[Offline]", Style::default().fg(Color::DarkGray)),
        };

        let (latency_str, latency_style) = if let Some(lat) = agent.heartbeat_latency_ms {
            let col = if lat > 5000 {
                Color::Red
            } else if lat > 1000 {
                Color::Yellow
            } else {
                Color::Green
            };
            (format!("{lat}ms"), Style::default().fg(col).add_modifier(Modifier::BOLD))
        } else {
            ("-".to_string(), Style::default().fg(Color::DarkGray))
        };

        let profile = agent.capabilities_profile();
        let capabilities = agent.capabilities_list();

        let mut lines = Vec::new();

        // 1. Alert Banner (if error reported)
        if let Some(ref err) = agent.last_error {
            lines.push(Line::from(vec![
                Span::styled(
                    "⚠ ERROR BANNER: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    err,
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));
        }

        // 2. Identity & Health Status Bar
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
            Span::styled("  |  Health: ", Style::default().fg(Color::DarkGray)),
            Span::styled(health_badge, health_style),
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
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]));

        // 3. Liveness & Heartbeat Telemetry
        let last_seen_str = agent
            .last_seen
            .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "Never".to_string());

        lines.push(Line::from(vec![
            Span::styled(" Last Seen: ", Style::default().fg(Color::DarkGray)),
            Span::styled(last_seen_str, Style::default().fg(Color::White)),
            Span::styled("  |  Heartbeat Latency: ", Style::default().fg(Color::DarkGray)),
            Span::styled(latency_str, latency_style),
            Span::styled("  |  Failures: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                agent.consecutive_failures.to_string(),
                if agent.consecutive_failures > 0 {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                },
            ),
            Span::styled(" (Done: ", Style::default().fg(Color::DarkGray)),
            Span::styled(agent.tasks_completed_count.to_string(), Style::default().fg(Color::Green)),
            Span::styled(", Failed: ", Style::default().fg(Color::DarkGray)),
            Span::styled(agent.tasks_failed_count.to_string(), Style::default().fg(Color::Red)),
            Span::styled(")", Style::default().fg(Color::DarkGray)),
        ]));

        // 4. Active Delivery & Concurrency
        let active_task_str = if let Some(tid) = agent.current_task_id {
            if let Some(rt) = state.review_tasks.iter().find(|rt| rt.task.id == tid) {
                format!("[{}] {}", rt.task.short_id, rt.task.title)
            } else {
                format!("[{}]", &tid.to_string()[..8])
            }
        } else {
            "Idle (No Active Task)".to_string()
        };

        let draining_str = if agent.is_draining { "YES" } else { "NO" };
        let draining_color = if agent.is_draining { Color::Yellow } else { Color::Green };

        lines.push(Line::from(vec![
            Span::styled(" Active Task: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                active_task_str,
                if agent.current_task_id.is_some() {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
            Span::styled("  |  Load: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{}", agent.active_tasks_count, agent.max_concurrency),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled("  |  Draining: ", Style::default().fg(Color::DarkGray)),
            Span::styled(draining_str, Style::default().fg(draining_color)),
        ]));

        // 5. Role, NATS & Security
        let revoked_str = if agent.is_revoked {
            "REVOKED"
        } else {
            "VALID"
        };
        let revoked_color = if agent.is_revoked {
            Color::Red
        } else {
            Color::Green
        };
        let expires_str = agent
            .api_key_expires_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "Never".to_string());

        lines.push(Line::from(vec![
            Span::styled(" Role: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&agent.role, Style::default().fg(Color::Yellow)),
            Span::styled("  |  Key Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(revoked_str, Style::default().fg(revoked_color).add_modifier(Modifier::BOLD)),
            Span::styled("  |  Key Expires: ", Style::default().fg(Color::DarkGray)),
            Span::styled(expires_str, Style::default().fg(Color::White)),
            Span::styled("  |  NATS: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&agent.nats_subject, Style::default().fg(Color::Cyan)),
        ]));

        lines.push(Line::from(""));

        // 6. Hardware & Runtime Telemetry
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

        // 7. Installed Languages & Toolchains
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

        // 8. Developer Tools & CLIs
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

        // 9. Domain & Skill Tags
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

        if is_modal {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press [Esc], [C], or [Enter] to dismiss this inspector dialog.",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        let p = Paragraph::new(lines);
        frame.render_widget(p, inner);
    }
}
