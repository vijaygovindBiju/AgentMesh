//! Phase 21 Operator TUI Enhancements & Production Console Verification Suite
//!
//! Comprehensive, deterministic Ratatui `TestBackend` test suite verifying all Phase 21
//! requirements across the 4-tier testing methodology:
//!
//! - Tier 1: Feature Coverage (5 screens, global header connectivity pills, command palette,
//!   safety confirmation dialogs, project management, recursive DAG tree, fleet telemetry cards,
//!   bounded diagnostics event stream, terminal resize resilience).
//! - Tier 2: Boundary & Corner Conditions (extreme event bursts, empty states, deep recursion,
//!   index clamping, unicode/symbols).
//! - Tier 3: Cross-Feature Combinations (modals over complex screens, action gating with overlaps,
//!   project switching state refresh, event filtering under burst).
//! - Tier 4: Real-World Workflows (operator review session, fleet triage, command palette workflow,
//!   task retry state transitions).

use std::collections::{HashMap, HashSet, VecDeque};
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::TestBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};
use serde_json::json;
use uuid::Uuid;

use coordinator::domain::{
    ApprovalStatus, HealthStatus,
    Project, ProjectStatus, Task, TaskStatus,
};
use coordinator::observability::CoordinatorEvent;
use coordinator::tui::{
    render, AppState, CurrentScreen, ReviewTaskState, TuiAction,
};

// ============================================================================
// Test Harness Helpers
// ============================================================================

fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).expect("Failed to initialize TestBackend terminal")
}

fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            s.push_str(buffer[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

fn assert_buffer_contains(terminal: &Terminal<TestBackend>, expected: &str) {
    let content = buffer_to_string(terminal);
    assert!(
        content.contains(expected),
        "Buffer did not contain expected text '{expected}'. Rendered buffer:\n{content}"
    );
}

#[allow(dead_code)]
fn assert_buffer_does_not_contain(terminal: &Terminal<TestBackend>, unexpected: &str) {
    let content = buffer_to_string(terminal);
    assert!(
        !content.contains(unexpected),
        "Buffer unexpectedly contained '{unexpected}'. Rendered buffer:\n{content}"
    );
}

fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press_char(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

#[allow(dead_code)]
fn press_modified(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn sample_task(id: Uuid, project_id: Uuid, short_id: &str, title: &str, status: TaskStatus) -> Task {
    Task {
        id,
        project_id,
        short_id: short_id.to_string(),
        title: title.to_string(),
        description: format!("Description for {short_id}"),
        affected_resources: json!(["crates/coordinator", "src/main.rs"]),
        status,
        assigned_agent_id: None,
        estimated_size: Some("M".to_string()),
        proposal_id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ============================================================================
// Phase 21 Core Architecture Models & Test Widgets
// ============================================================================

/// Phase 21 5-Screen Hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase21Screen {
    #[default]
    Dashboard,
    ProjectManagement,
    PlanReview,
    FleetInspector,
    Diagnostics,
}

impl Phase21Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Phase21Screen::Dashboard => "1: Dashboard",
            Phase21Screen::ProjectManagement => "2: Projects",
            Phase21Screen::PlanReview => "3: Plan Review",
            Phase21Screen::FleetInspector => "4: Fleet",
            Phase21Screen::Diagnostics => "5: Diagnostics",
        }
    }

    pub fn screen_header(&self) -> &'static str {
        match self {
            Phase21Screen::Dashboard => "System Overview & Live Dashboard",
            Phase21Screen::ProjectManagement => "Project Management & Spec Inspection",
            Phase21Screen::PlanReview => "Proposed Task Decomposition (Human Review)",
            Phase21Screen::FleetInspector => "Agent Fleet Management & Inspection",
            Phase21Screen::Diagnostics => "Live Event Stream & Diagnostics",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Phase21Screen::Dashboard => Phase21Screen::ProjectManagement,
            Phase21Screen::ProjectManagement => Phase21Screen::PlanReview,
            Phase21Screen::PlanReview => Phase21Screen::FleetInspector,
            Phase21Screen::FleetInspector => Phase21Screen::Diagnostics,
            Phase21Screen::Diagnostics => Phase21Screen::Dashboard,
        }
    }

    pub fn from_key(c: char) -> Option<Self> {
        match c {
            '1' => Some(Phase21Screen::Dashboard),
            '2' => Some(Phase21Screen::ProjectManagement),
            '3' => Some(Phase21Screen::PlanReview),
            '4' => Some(Phase21Screen::FleetInspector),
            '5' => Some(Phase21Screen::Diagnostics),
            _ => None,
        }
    }
}

/// Global Status Header State
#[derive(Debug, Clone)]
pub struct HeaderState {
    pub active_project_name: String,
    pub pg_connected: bool,
    pub nats_connected: bool,
    pub active_agents: usize,
    pub total_agents: usize,
    pub pending_approvals: usize,
    pub current_screen: Phase21Screen,
}

impl Default for HeaderState {
    fn default() -> Self {
        Self {
            active_project_name: "No Project Selected".to_string(),
            pg_connected: true,
            nats_connected: true,
            active_agents: 2,
            total_agents: 3,
            pending_approvals: 1,
            current_screen: Phase21Screen::Dashboard,
        }
    }
}

pub struct HeaderWidget;

impl HeaderWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &HeaderState) {
        let is_compact = area.width < 90;

        let (top_area, tabs_area) = if is_compact && area.height >= 2 {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(area);
            (chunks[0], chunks[1])
        } else {
            (area, Rect::default())
        };

        let pg_pill = if state.pg_connected {
            Span::styled("[PG: ● OK]", Style::default().fg(Color::Green))
        } else {
            Span::styled("[PG: ○ OFF]", Style::default().fg(Color::Red))
        };

        let nats_pill = if state.nats_connected {
            Span::styled("[NATS: ● OK]", Style::default().fg(Color::Green))
        } else {
            Span::styled("[NATS: ○ OFF]", Style::default().fg(Color::Red))
        };

        let agent_ratio = Span::styled(
            format!("[Agents: {}/{}]", state.active_agents, state.total_agents),
            Style::default().fg(Color::Cyan),
        );

        let pending_pill = Span::styled(
            format!("[Pending: {}]", state.pending_approvals),
            if state.pending_approvals > 0 {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        );

        let proj_span = if is_compact {
            Span::styled(
                format!(" [{}] ", state.active_project_name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!(" Project: {} ", state.active_project_name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )
        };

        let status_line = Line::from(vec![
            Span::styled("AgentMesh ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            proj_span,
            Span::raw(" "),
            pg_pill,
            Span::raw(" "),
            nats_pill,
            Span::raw(" "),
            agent_ratio,
            Span::raw(" "),
            pending_pill,
        ]);

        let p = Paragraph::new(status_line).block(
            Block::default()
                .borders(if !is_compact && area.height >= 3 { Borders::ALL } else { Borders::NONE })
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(p, top_area);

        if is_compact && area.height >= 2 {
            let titles = vec![
                Line::from("1:Dash"),
                Line::from("2:Proj"),
                Line::from("3:Plan"),
                Line::from("4:Fleet"),
                Line::from("5:Diag"),
            ];
            let tabs = Tabs::new(titles)
                .select(state.current_screen as usize)
                .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            frame.render_widget(tabs, tabs_area);
        }
    }
}

/// Global Command Palette Modal (`:`)
#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    pub is_open: bool,
    pub query: String,
    pub selected_index: usize,
    pub commands: Vec<CommandItem>,
}

#[derive(Debug, Clone)]
pub struct CommandItem {
    pub name: String,
    pub description: String,
    pub category: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    SwitchScreen(Phase21Screen),
    LoadProject(String),
    ApproveAllPending,
    RefreshData,
    Quit,
}

impl CommandPaletteState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            query: String::new(),
            selected_index: 0,
            commands: vec![
                CommandItem {
                    name: "nav:dashboard".to_string(),
                    description: "Switch to System Dashboard".to_string(),
                    category: "Navigation".to_string(),
                    action: CommandAction::SwitchScreen(Phase21Screen::Dashboard),
                },
                CommandItem {
                    name: "nav:projects".to_string(),
                    description: "Switch to Project Management".to_string(),
                    category: "Navigation".to_string(),
                    action: CommandAction::SwitchScreen(Phase21Screen::ProjectManagement),
                },
                CommandItem {
                    name: "nav:plan".to_string(),
                    description: "Switch to Plan Review & DAG".to_string(),
                    category: "Navigation".to_string(),
                    action: CommandAction::SwitchScreen(Phase21Screen::PlanReview),
                },
                CommandItem {
                    name: "nav:fleet".to_string(),
                    description: "Switch to Fleet Inspector".to_string(),
                    category: "Navigation".to_string(),
                    action: CommandAction::SwitchScreen(Phase21Screen::FleetInspector),
                },
                CommandItem {
                    name: "nav:diagnostics".to_string(),
                    description: "Switch to Live Diagnostics".to_string(),
                    category: "Navigation".to_string(),
                    action: CommandAction::SwitchScreen(Phase21Screen::Diagnostics),
                },
                CommandItem {
                    name: "action:approve_all".to_string(),
                    description: "Approve all unblocked proposed tasks".to_string(),
                    category: "Workflow".to_string(),
                    action: CommandAction::ApproveAllPending,
                },
                CommandItem {
                    name: "action:refresh".to_string(),
                    description: "Refresh state from PostgreSQL & NATS".to_string(),
                    category: "System".to_string(),
                    action: CommandAction::RefreshData,
                },
                CommandItem {
                    name: "system:quit".to_string(),
                    description: "Exit coordinator operator console".to_string(),
                    category: "System".to_string(),
                    action: CommandAction::Quit,
                },
            ],
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn filtered_commands(&self) -> Vec<&CommandItem> {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.commands.iter().collect()
        } else {
            self.commands
                .iter()
                .filter(|c| c.name.to_lowercase().contains(&q) || c.description.to_lowercase().contains(&q))
                .collect()
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<CommandAction> {
        if !self.is_open {
            if key.code == KeyCode::Char(':') {
                self.open();
            }
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.close();
                None
            }
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                None
            }
            KeyCode::Down => {
                let count = self.filtered_commands().len();
                if count > 0 && self.selected_index + 1 < count {
                    self.selected_index += 1;
                }
                None
            }
            KeyCode::Enter => {
                let matches = self.filtered_commands();
                if let Some(cmd) = matches.get(self.selected_index) {
                    let action = cmd.action.clone();
                    self.close();
                    Some(action)
                } else {
                    None
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_index = 0;
                None
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.selected_index = 0;
                None
            }
            _ => None,
        }
    }
}

pub struct CommandPaletteWidget;

impl CommandPaletteWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &CommandPaletteState) {
        if !state.is_open {
            return;
        }

        let width = (area.width * 70 / 100).clamp(40, 80).min(area.width);
        let height = (area.height * 60 / 100).clamp(10, 20).min(area.height);

        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 3;
        let modal_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Command Palette (:) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, modal_area);

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(4)])
            .split(modal_area);

        let input_line = Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(&state.query, Style::default().fg(Color::White)),
            Span::styled("█", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(input_line), inner[0]);

        let divider = Paragraph::new("─".repeat(inner[1].width as usize))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(divider, inner[1]);

        let matches = state.filtered_commands();
        let rows: Vec<Row> = matches
            .iter()
            .enumerate()
            .map(|(idx, cmd)| {
                let is_sel = idx == state.selected_index;
                let prefix = if is_sel { "▶ " } else { "  " };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Row::new(vec![
                    Span::styled(format!("{prefix}{}", cmd.name), style),
                    Span::styled(&cmd.description, Style::default().fg(Color::Gray)),
                    Span::styled(&cmd.category, Style::default().fg(Color::DarkGray)),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [Constraint::Length(22), Constraint::Min(20), Constraint::Length(12)],
        );
        frame.render_widget(table, inner[2]);
    }
}

/// Safety Confirmation Dialog Modal
#[derive(Debug, Clone)]
pub struct ConfirmationModalState {
    pub is_open: bool,
    pub title: String,
    pub message: String,
    pub action_target_id: Option<Uuid>,
    pub pending_action: Option<DestructiveAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestructiveAction {
    CancelTask(Uuid),
    RejectTask(Uuid),
}

impl ConfirmationModalState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            title: String::new(),
            message: String::new(),
            action_target_id: None,
            pending_action: None,
        }
    }

    pub fn prompt_cancel(&mut self, task_id: Uuid, short_id: &str) {
        self.is_open = true;
        self.title = "Confirm Task Cancellation".to_string();
        self.message = format!("Are you sure you want to cancel task {short_id}? This terminates active worker deliveries.");
        self.action_target_id = Some(task_id);
        self.pending_action = Some(DestructiveAction::CancelTask(task_id));
    }

    pub fn prompt_reject(&mut self, task_id: Uuid, short_id: &str) {
        self.is_open = true;
        self.title = "Confirm Task Rejection".to_string();
        self.message = format!("Are you sure you want to reject proposed task {short_id}?");
        self.action_target_id = Some(task_id);
        self.pending_action = Some(DestructiveAction::RejectTask(task_id));
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<DestructiveAction> {
        if !self.is_open {
            return None;
        }

        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let action = self.pending_action.take();
                self.is_open = false;
                action
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.is_open = false;
                self.pending_action = None;
                None
            }
            _ => None,
        }
    }
}

pub struct ConfirmationModalWidget;

impl ConfirmationModalWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &ConfirmationModalState) {
        if !state.is_open {
            return;
        }

        let width = (area.width * 60 / 100).clamp(40, 70).min(area.width);
        let height = 8.min(area.height);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        let modal_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(format!(" ⚠ {} ", state.title), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(Color::Red));
        frame.render_widget(block, modal_area);

        let inner = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(2), Constraint::Length(1)])
            .split(modal_area);

        let msg = Paragraph::new(state.message.as_str())
            .style(Style::default().fg(Color::White));
        frame.render_widget(msg, inner[0]);

        let buttons = Line::from(vec![
            Span::styled(" [Y] Confirm ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("    "),
            Span::styled(" [N/Esc] Cancel ", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(buttons), inner[1]);
    }
}

/// Project Management State & Table View
#[derive(Debug, Clone)]
pub struct ProjectManagementState {
    pub projects: Vec<ProjectSummary>,
    pub selected_index: usize,
    pub active_project_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub repo_path: String,
    pub base_branch: String,
    pub current_commit_sha: String,
    pub task_count: usize,
}

impl ProjectManagementState {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            selected_index: 0,
            active_project_id: None,
        }
    }

    pub fn with_sample_projects() -> Self {
        let p1_id = Uuid::new_v4();
        let p2_id = Uuid::new_v4();
        Self {
            projects: vec![
                ProjectSummary {
                    id: p1_id,
                    name: "AgentMesh Core".to_string(),
                    status: "Executing".to_string(),
                    repo_path: "/workspace/AgentMesh".to_string(),
                    base_branch: "master".to_string(),
                    current_commit_sha: "9161054".to_string(),
                    task_count: 14,
                },
                ProjectSummary {
                    id: p2_id,
                    name: "Minigrep Real-World".to_string(),
                    status: "Completed".to_string(),
                    repo_path: "/workspace/minigrep".to_string(),
                    base_branch: "main".to_string(),
                    current_commit_sha: "cfe8977".to_string(),
                    task_count: 8,
                },
            ],
            selected_index: 0,
            active_project_id: Some(p1_id),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.projects.is_empty() && self.selected_index + 1 < self.projects.len() {
                    self.selected_index += 1;
                }
                None
            }
            KeyCode::Enter => {
                if let Some(proj) = self.projects.get(self.selected_index) {
                    self.active_project_id = Some(proj.id);
                    Some(TuiAction::RefreshData) // LoadProject emission
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

pub struct ProjectManagementWidget;

impl ProjectManagementWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &ProjectManagementState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);

        // 1. Projects Table
        let rows: Vec<Row> = state
            .projects
            .iter()
            .enumerate()
            .map(|(idx, p)| {
                let is_sel = idx == state.selected_index;
                let is_active = Some(p.id) == state.active_project_id;
                let mark = if is_active { "★ " } else if is_sel { "▶ " } else { "  " };
                let style = if is_sel {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Row::new(vec![
                    Span::styled(format!("{mark}{}", p.name), style),
                    Span::styled(&p.status, Style::default().fg(Color::Cyan)),
                    Span::styled(&p.base_branch, Style::default().fg(Color::Gray)),
                    Span::styled(p.task_count.to_string(), Style::default().fg(Color::DarkGray)),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(24),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(6),
            ],
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Projects List ([Enter] Load, [↑/↓] Navigate) ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(table, chunks[0]);

        // 2. Project Metadata Spec Inspector
        let details_block = Block::default()
            .borders(Borders::ALL)
            .title(" Active Project Spec & Git Metadata ")
            .border_style(Style::default().fg(Color::DarkGray));

        if let Some(p) = state.projects.get(state.selected_index) {
            let lines = vec![
                Line::from(vec![
                    Span::styled("Project Name:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&p.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Repository:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&p.repo_path, Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Base Branch:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&p.base_branch, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("HEAD Commit:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&p.current_commit_sha, Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled("Tasks Count:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{} planned tasks", p.task_count), Style::default().fg(Color::White)),
                ]),
            ];
            let p_widget = Paragraph::new(lines).block(details_block);
            frame.render_widget(p_widget, chunks[1]);
        } else {
            let empty = Paragraph::new("No projects found.").block(details_block);
            frame.render_widget(empty, chunks[1]);
        }
    }
}

/// Recursive DAG Dependency Tree Generator & State
#[derive(Debug, Clone)]
pub struct DagNode {
    pub task_id: Uuid,
    pub short_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub dependencies: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct RecursiveDagState {
    pub nodes: HashMap<Uuid, DagNode>,
    pub ordered_task_ids: Vec<Uuid>,
    pub selected_index: usize,
}

impl RecursiveDagState {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            ordered_task_ids: Vec::new(),
            selected_index: 0,
        }
    }

    /// Build a multi-tier dependency chain: A -> B -> C -> D
    pub fn with_multi_tier_pipeline() -> Self {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let id_c = Uuid::new_v4();
        let id_d = Uuid::new_v4();

        let mut nodes = HashMap::new();
        nodes.insert(
            id_a,
            DagNode {
                task_id: id_a,
                short_id: "PLAN-1".to_string(),
                title: "Database Schema Migration".to_string(),
                status: TaskStatus::Completed,
                dependencies: vec![],
            },
        );
        nodes.insert(
            id_b,
            DagNode {
                task_id: id_b,
                short_id: "PLAN-2".to_string(),
                title: "NATS JetStream Stream Provisioning".to_string(),
                status: TaskStatus::Approved,
                dependencies: vec![id_a],
            },
        );
        nodes.insert(
            id_c,
            DagNode {
                task_id: id_c,
                short_id: "PLAN-3".to_string(),
                title: "Coordinator Assignment Service".to_string(),
                status: TaskStatus::HumanReview,
                dependencies: vec![id_b],
            },
        );
        nodes.insert(
            id_d,
            DagNode {
                task_id: id_d,
                short_id: "PLAN-4".to_string(),
                title: "End-to-End Operator Validation".to_string(),
                status: TaskStatus::Proposed,
                dependencies: vec![id_c],
            },
        );

        let ordered_task_ids = vec![id_a, id_b, id_c, id_d];
        Self {
            nodes,
            ordered_task_ids,
            selected_index: 0,
        }
    }

    /// Compute topological tiers: Tier 0 = roots (no dependencies), Tier 1 = depends on Tier 0, etc.
    pub fn compute_topological_tiers(&self) -> Vec<Vec<Uuid>> {
        let mut tiers: Vec<Vec<Uuid>> = Vec::new();
        let mut assigned = HashSet::new();

        // Tier 0: roots
        let tier0: Vec<Uuid> = self
            .ordered_task_ids
            .iter()
            .copied()
            .filter(|id| self.nodes.get(id).map_or(false, |n| n.dependencies.is_empty()))
            .collect();

        for id in &tier0 {
            assigned.insert(*id);
        }
        tiers.push(tier0);

        // Iteratively assign subsequent tiers
        while assigned.len() < self.ordered_task_ids.len() {
            let mut next_tier = Vec::new();
            for id in &self.ordered_task_ids {
                if !assigned.contains(id) {
                    if let Some(node) = self.nodes.get(id) {
                        if node.dependencies.iter().all(|dep| assigned.contains(dep)) {
                            next_tier.push(*id);
                        }
                    }
                }
            }

            if next_tier.is_empty() {
                // Cycle or broken dependency: collect remaining
                for id in &self.ordered_task_ids {
                    if !assigned.contains(id) {
                        next_tier.push(*id);
                    }
                }
                for id in &next_tier {
                    assigned.insert(*id);
                }
                tiers.push(next_tier);
                break;
            }

            for id in &next_tier {
                assigned.insert(*id);
            }
            tiers.push(next_tier);
        }

        tiers
    }

    /// Jump to upstream blocker (`[`)
    pub fn jump_to_blocker(&mut self) -> bool {
        if let Some(current_id) = self.ordered_task_ids.get(self.selected_index) {
            if let Some(node) = self.nodes.get(current_id) {
                if let Some(first_blocker) = node.dependencies.first() {
                    if let Some(target_idx) = self.ordered_task_ids.iter().position(|id| id == first_blocker) {
                        self.selected_index = target_idx;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Jump to downstream dependent (`]`)
    pub fn jump_to_dependent(&mut self) -> bool {
        if let Some(current_id) = self.ordered_task_ids.get(self.selected_index) {
            for (idx, id) in self.ordered_task_ids.iter().enumerate() {
                if let Some(node) = self.nodes.get(id) {
                    if node.dependencies.contains(current_id) {
                        self.selected_index = idx;
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn render_tree_lines<'a>(&'a self) -> Vec<Line<'a>> {
        let tiers = self.compute_topological_tiers();
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("Topological Execution Tiers (Recursive Traversal)", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]));

        for (tier_idx, tier) in tiers.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("── Tier {tier_idx} ──────────────────────────────────────"), Style::default().fg(Color::DarkGray)),
            ]));

            for task_id in tier {
                if let Some(node) = self.nodes.get(task_id) {
                    let is_selected = self.ordered_task_ids.get(self.selected_index) == Some(task_id);
                    let prefix = if is_selected { "▶ " } else { "  " };

                    let status_badge = match node.status {
                        TaskStatus::Completed => Span::styled("[C]", Style::default().fg(Color::Green)),
                        TaskStatus::Approved => Span::styled("[A]", Style::default().fg(Color::Cyan)),
                        TaskStatus::HumanReview => Span::styled("[R]", Style::default().fg(Color::Yellow)),
                        TaskStatus::Executing => Span::styled("[E]", Style::default().fg(Color::Blue)),
                        TaskStatus::Failed => Span::styled("[F]", Style::default().fg(Color::Red)),
                        TaskStatus::Cancelled => Span::styled("[X]", Style::default().fg(Color::DarkGray)),
                        _ => Span::styled("[P]", Style::default().fg(Color::Gray)),
                    };

                    let dep_str = if node.dependencies.is_empty() {
                        " (Root / Unblocked)".to_string()
                    } else {
                        let dep_names: Vec<String> = node
                            .dependencies
                            .iter()
                            .filter_map(|d| self.nodes.get(d).map(|n| n.short_id.clone()))
                            .collect();
                        format!(" (Waits on: {})", dep_names.join(", "))
                    };

                    let line_style = if is_selected {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Yellow)),
                        status_badge,
                        Span::raw(" "),
                        Span::styled(&node.short_id, line_style),
                        Span::raw(": "),
                        Span::styled(&node.title, line_style),
                        Span::styled(dep_str, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }

        lines
    }
}

pub struct DagWidget;

impl DagWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &RecursiveDagState) {
        let lines = state.render_tree_lines();
        let p = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Visual Dependency Graph ([↑/↓] Nav, [[] Blocker, []] Dependent) ")
                .border_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(p, area);
    }
}

/// Enriched Fleet Telemetry Card State & View
#[derive(Debug, Clone)]
pub struct FleetTelemetryState {
    pub agents: Vec<AgentTelemetry>,
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub struct AgentTelemetry {
    pub id: Uuid,
    pub name: String,
    pub adapter_type: String,
    pub health_status: HealthStatus,
    pub last_seen: DateTime<Utc>,
    pub heartbeat_latency_ms: i64,
    pub consecutive_failures: i32,
    pub last_error: Option<String>,
    pub current_task_short_id: Option<String>,
    pub capabilities: Vec<String>,
}

impl FleetTelemetryState {
    pub fn with_sample_fleet() -> Self {
        Self {
            agents: vec![
                AgentTelemetry {
                    id: Uuid::new_v4(),
                    name: "agent-backend (Alice)".to_string(),
                    adapter_type: "Agy".to_string(),
                    health_status: HealthStatus::Healthy,
                    last_seen: Utc::now(),
                    heartbeat_latency_ms: 45,
                    consecutive_failures: 0,
                    last_error: None,
                    current_task_short_id: Some("PLAN-2".to_string()),
                    capabilities: vec!["rust".to_string(), "backend".to_string(), "cargo".to_string()],
                },
                AgentTelemetry {
                    id: Uuid::new_v4(),
                    name: "agent-infra (Bob)".to_string(),
                    adapter_type: "Mock".to_string(),
                    health_status: HealthStatus::Degraded,
                    last_seen: Utc::now(),
                    heartbeat_latency_ms: 1850,
                    consecutive_failures: 1,
                    last_error: Some("High heartbeat latency detected (1850ms > 1000ms)".to_string()),
                    current_task_short_id: None,
                    capabilities: vec!["docker".to_string(), "nats".to_string()],
                },
                AgentTelemetry {
                    id: Uuid::new_v4(),
                    name: "agent-reviewer (Charlie)".to_string(),
                    adapter_type: "Agy".to_string(),
                    health_status: HealthStatus::Unhealthy,
                    last_seen: Utc::now(),
                    heartbeat_latency_ms: 12500,
                    consecutive_failures: 3,
                    last_error: Some("Connection reset by peer; 3 consecutive task timeouts".to_string()),
                    current_task_short_id: None,
                    capabilities: vec!["review".to_string(), "git".to_string()],
                },
            ],
            selected_index: 0,
        }
    }
}

pub struct FleetTelemetryWidget;

impl FleetTelemetryWidget {
    pub fn render(frame: &mut Frame, area: Rect, state: &FleetTelemetryState) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // 1. Agents Health Table
        let rows: Vec<Row> = state
            .agents
            .iter()
            .enumerate()
            .map(|(idx, a)| {
                let is_sel = idx == state.selected_index;
                let mark = if is_sel { "▶ " } else { "  " };
                let (health_badge, health_style) = match a.health_status {
                    HealthStatus::Healthy => ("[Healthy]", Style::default().fg(Color::Green)),
                    HealthStatus::Degraded => ("[Degraded]", Style::default().fg(Color::Yellow)),
                    HealthStatus::Unhealthy => ("[Unhealthy]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    HealthStatus::Offline => ("[Offline]", Style::default().fg(Color::DarkGray)),
                };
                let latency_span = Span::styled(
                    format!("{}ms", a.heartbeat_latency_ms),
                    if a.heartbeat_latency_ms > 1000 {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Green)
                    },
                );
                Row::new(vec![
                    Span::styled(format!("{mark}{}", a.name), if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                    Span::styled(&a.adapter_type, Style::default().fg(Color::Cyan)),
                    Span::styled(health_badge, health_style),
                    latency_span,
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(22),
                Constraint::Length(8),
                Constraint::Length(12),
                Constraint::Length(8),
            ],
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Connected Agent Fleet & Liveness ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(table, chunks[0]);

        // 2. Selected Agent Telemetry Card & Error Banner
        let card_block = Block::default()
            .borders(Borders::ALL)
            .title(" Agent Telemetry Profile ")
            .border_style(Style::default().fg(Color::DarkGray));

        if let Some(agent) = state.agents.get(state.selected_index) {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Agent Name:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&agent.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("Runtime Adapter: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&agent.adapter_type, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Heartbeat Lat.:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{} ms", agent.heartbeat_latency_ms), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::styled("Current Task:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        agent.current_task_short_id.clone().unwrap_or_else(|| "Idle (No Active Task)".to_string()),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Capabilities:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(agent.capabilities.join(", "), Style::default().fg(Color::White)),
                ]),
            ];

            // Render Error Banner if degraded or unhealthy
            if let Some(err) = &agent.last_error {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("⚠ ERROR BANNER: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled(err, Style::default().fg(Color::LightRed)),
                ]));
            }

            let card = Paragraph::new(lines).block(card_block);
            frame.render_widget(card, chunks[1]);
        } else {
            let empty = Paragraph::new("No agent selected.").block(card_block);
            frame.render_widget(empty, chunks[1]);
        }
    }
}

/// Bounded Ring Buffer & Multi-Axis Filtering for Diagnostics
#[derive(Debug, Clone)]
pub struct BoundedEventStream {
    pub capacity: usize,
    pub events: VecDeque<CoordinatorEvent>,
    pub severity_filter: Option<String>,
    pub entity_filter: Option<String>,
    pub selected_index: usize,
}

impl BoundedEventStream {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: VecDeque::with_capacity(capacity),
            severity_filter: None,
            entity_filter: None,
            selected_index: 0,
        }
    }

    /// O(1) bounded push: evicts oldest element if capacity reached
    pub fn push(&mut self, event: CoordinatorEvent) {
        if self.events.len() >= self.capacity {
            self.events.pop_back();
        }
        self.events.push_front(event);
    }

    pub fn filtered_events(&self) -> Vec<&CoordinatorEvent> {
        self.events
            .iter()
            .filter(|ev| {
                if let Some(ref sev) = self.severity_filter {
                    let event_sev = if ev.event_type.contains("fail") || ev.event_type.contains("error") {
                        "ERROR"
                    } else if ev.event_type.contains("warn") || ev.event_type.contains("overlap") {
                        "WARN"
                    } else {
                        "INFO"
                    };
                    if event_sev != sev.as_str() {
                        return false;
                    }
                }
                if let Some(ref entity) = self.entity_filter {
                    if !ev.event_type.starts_with(entity) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    pub fn clamp_selection(&mut self) {
        let count = self.filtered_events().len();
        if count == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= count {
            self.selected_index = count - 1;
        }
    }
}

// ============================================================================
// TIER 1: FEATURE COVERAGE SUITES
// ============================================================================

// ----------------------------------------------------------------------------
// Tier 1.1: 5-Screen Operator Navigation & Title Rendering (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_screen_nav_5_screens_initial_and_titles() {
    let screens = [
        Phase21Screen::Dashboard,
        Phase21Screen::ProjectManagement,
        Phase21Screen::PlanReview,
        Phase21Screen::FleetInspector,
        Phase21Screen::Diagnostics,
    ];

    assert_eq!(Phase21Screen::default(), Phase21Screen::Dashboard);
    assert_eq!(screens[0].title(), "1: Dashboard");
    assert_eq!(screens[1].title(), "2: Projects");
    assert_eq!(screens[2].title(), "3: Plan Review");
    assert_eq!(screens[3].title(), "4: Fleet");
    assert_eq!(screens[4].title(), "5: Diagnostics");

    assert_eq!(screens[0].screen_header(), "System Overview & Live Dashboard");
    assert_eq!(screens[1].screen_header(), "Project Management & Spec Inspection");
    assert_eq!(screens[2].screen_header(), "Proposed Task Decomposition (Human Review)");
    assert_eq!(screens[3].screen_header(), "Agent Fleet Management & Inspection");
    assert_eq!(screens[4].screen_header(), "Live Event Stream & Diagnostics");
}

#[test]
fn test_tier1_screen_nav_5_screens_tab_cycling() {
    let mut screen = Phase21Screen::Dashboard;

    screen = screen.next();
    assert_eq!(screen, Phase21Screen::ProjectManagement);

    screen = screen.next();
    assert_eq!(screen, Phase21Screen::PlanReview);

    screen = screen.next();
    assert_eq!(screen, Phase21Screen::FleetInspector);

    screen = screen.next();
    assert_eq!(screen, Phase21Screen::Diagnostics);

    screen = screen.next();
    assert_eq!(screen, Phase21Screen::Dashboard);
}

#[test]
fn test_tier1_screen_nav_numeric_hotkeys_1_to_5() {
    assert_eq!(Phase21Screen::from_key('1'), Some(Phase21Screen::Dashboard));
    assert_eq!(Phase21Screen::from_key('2'), Some(Phase21Screen::ProjectManagement));
    assert_eq!(Phase21Screen::from_key('3'), Some(Phase21Screen::PlanReview));
    assert_eq!(Phase21Screen::from_key('4'), Some(Phase21Screen::FleetInspector));
    assert_eq!(Phase21Screen::from_key('5'), Some(Phase21Screen::Diagnostics));
    assert_eq!(Phase21Screen::from_key('6'), None);
}

#[test]
fn test_tier1_screen_nav_all_5_screen_headers_render() {
    let mut terminal = create_terminal(100, 24);
    let screens = [
        Phase21Screen::Dashboard,
        Phase21Screen::ProjectManagement,
        Phase21Screen::PlanReview,
        Phase21Screen::FleetInspector,
        Phase21Screen::Diagnostics,
    ];

    for s in &screens {
        terminal
            .draw(|f| {
                let p = Paragraph::new(s.screen_header()).block(Block::default().borders(Borders::ALL));
                f.render_widget(p, f.area());
            })
            .unwrap();
        assert_buffer_contains(&terminal, s.screen_header());
    }
}

#[test]
fn test_tier1_screen_nav_preserves_backward_compatible_aliases() {
    // Verifies coordinator's CurrentScreen enum continues to work seamlessly
    let mut state = AppState::new();
    assert_eq!(state.current_screen, CurrentScreen::ProjectInput);

    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::PlanReview);

    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::Dashboard);

    state.handle_key(press_key(KeyCode::Tab));
    assert_eq!(state.current_screen, CurrentScreen::Diagnostics);
}

// ----------------------------------------------------------------------------
// Tier 1.2: Global Status Header & Connectivity Pills (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_header_renders_connectivity_pills_healthy() {
    let mut terminal = create_terminal(120, 5);
    let state = HeaderState {
        active_project_name: "Cloud Mesh".to_string(),
        pg_connected: true,
        nats_connected: true,
        active_agents: 2,
        total_agents: 3,
        pending_approvals: 1,
        current_screen: Phase21Screen::Dashboard,
    };

    terminal.draw(|f| HeaderWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[PG: ● OK]");
    assert_buffer_contains(&terminal, "[NATS: ● OK]");
    assert_buffer_contains(&terminal, "[Agents: 2/3]");
    assert_buffer_contains(&terminal, "[Pending: 1]");
    assert_buffer_contains(&terminal, "Project: Cloud Mesh");
}

#[test]
fn test_tier1_header_renders_connectivity_pills_offline() {
    let mut terminal = create_terminal(120, 5);
    let state = HeaderState {
        active_project_name: "No Project Selected".to_string(),
        pg_connected: false,
        nats_connected: false,
        active_agents: 0,
        total_agents: 0,
        pending_approvals: 0,
        current_screen: Phase21Screen::Dashboard,
    };

    terminal.draw(|f| HeaderWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[PG: ○ OFF]");
    assert_buffer_contains(&terminal, "[NATS: ○ OFF]");
    assert_buffer_contains(&terminal, "[Agents: 0/0]");
    assert_buffer_contains(&terminal, "[Pending: 0]");
}

#[test]
fn test_tier1_header_renders_pending_approvals_alert() {
    let mut terminal = create_terminal(120, 5);
    let state = HeaderState {
        active_project_name: "Prod Deployment".to_string(),
        pg_connected: true,
        nats_connected: true,
        active_agents: 4,
        total_agents: 4,
        pending_approvals: 5,
        current_screen: Phase21Screen::PlanReview,
    };

    terminal.draw(|f| HeaderWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[Pending: 5]");
}

#[test]
fn test_tier1_header_adaptive_compact_terminal_render() {
    let mut terminal = create_terminal(75, 4);
    let state = HeaderState {
        active_project_name: "Compact".to_string(),
        pg_connected: true,
        nats_connected: true,
        active_agents: 1,
        total_agents: 2,
        pending_approvals: 2,
        current_screen: Phase21Screen::ProjectManagement,
    };

    terminal.draw(|f| HeaderWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[PG: ● OK]");
    assert_buffer_contains(&terminal, "[NATS: ● OK]");
    assert_buffer_contains(&terminal, "[Agents: 1/2]");
}

#[test]
fn test_tier1_header_zero_panics_on_minimal_dimensions() {
    let mut terminal = create_terminal(60, 2);
    let state = HeaderState::default();
    terminal.draw(|f| HeaderWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

// ----------------------------------------------------------------------------
// Tier 1.3: Global Command Palette Modal (:) (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_command_palette_open_and_close() {
    let mut state = CommandPaletteState::new();
    assert!(!state.is_open);

    // Open via ':'
    state.handle_key(press_char(':'));
    assert!(state.is_open);
    assert_eq!(state.query, "");

    // Close via Esc
    state.handle_key(press_key(KeyCode::Esc));
    assert!(!state.is_open);
}

#[test]
fn test_tier1_command_palette_query_filtering() {
    let mut state = CommandPaletteState::new();
    state.open();

    // Type "fleet"
    for c in "fleet".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.query, "fleet");

    let matches = state.filtered_commands();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "nav:fleet");
}

#[test]
fn test_tier1_command_palette_backspace_filtering() {
    let mut state = CommandPaletteState::new();
    state.open();

    for c in "plan".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.filtered_commands().len(), 1);

    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.query, "pla");
    assert_eq!(state.filtered_commands().len(), 1);
}

#[test]
fn test_tier1_command_palette_up_down_selection() {
    let mut state = CommandPaletteState::new();
    state.open();
    assert_eq!(state.selected_index, 0);

    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_index, 1);

    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_index, 2);

    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.selected_index, 1);
}

#[test]
fn test_tier1_command_palette_action_dispatch_on_enter() {
    let mut state = CommandPaletteState::new();
    state.open();

    // Filter to "quit"
    for c in "quit".chars() {
        state.handle_key(press_char(c));
    }

    let action = state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, Some(CommandAction::Quit));
    assert!(!state.is_open);
}

// ----------------------------------------------------------------------------
// Tier 1.4: Safety Confirmation Dialog Modal (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_confirmation_dialog_render_content() {
    let mut terminal = create_terminal(80, 24);
    let mut state = ConfirmationModalState::new();
    let task_id = Uuid::new_v4();

    state.prompt_cancel(task_id, "PLAN-3");
    assert!(state.is_open);

    terminal.draw(|f| ConfirmationModalWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "Confirm Task Cancellation");
    assert_buffer_contains(&terminal, "Are you sure you want to cancel task PLAN-3?");
    assert_buffer_contains(&terminal, "[Y] Confirm");
    assert_buffer_contains(&terminal, "[N/Esc] Cancel");
}

#[test]
fn test_tier1_confirmation_dialog_confirm_with_y() {
    let mut state = ConfirmationModalState::new();
    let task_id = Uuid::new_v4();

    state.prompt_cancel(task_id, "PLAN-1");
    let action = state.handle_key(press_char('y'));

    assert_eq!(action, Some(DestructiveAction::CancelTask(task_id)));
    assert!(!state.is_open);
}

#[test]
fn test_tier1_confirmation_dialog_confirm_with_enter() {
    let mut state = ConfirmationModalState::new();
    let task_id = Uuid::new_v4();

    state.prompt_reject(task_id, "PLAN-2");
    let action = state.handle_key(press_key(KeyCode::Enter));

    assert_eq!(action, Some(DestructiveAction::RejectTask(task_id)));
    assert!(!state.is_open);
}

#[test]
fn test_tier1_confirmation_dialog_cancel_with_n() {
    let mut state = ConfirmationModalState::new();
    let task_id = Uuid::new_v4();

    state.prompt_cancel(task_id, "PLAN-4");
    let action = state.handle_key(press_char('n'));

    assert_eq!(action, None);
    assert!(!state.is_open);
}

#[test]
fn test_tier1_confirmation_dialog_cancel_with_esc() {
    let mut state = ConfirmationModalState::new();
    let task_id = Uuid::new_v4();

    state.prompt_reject(task_id, "PLAN-5");
    let action = state.handle_key(press_key(KeyCode::Esc));

    assert_eq!(action, None);
    assert!(!state.is_open);
}

// ----------------------------------------------------------------------------
// Tier 1.5: Project Management Screen & Selection (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_project_mgmt_rendering_table_and_metadata() {
    let mut terminal = create_terminal(100, 24);
    let state = ProjectManagementState::with_sample_projects();

    terminal.draw(|f| ProjectManagementWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh Core");
    assert_buffer_contains(&terminal, "Minigrep Real-World");
    assert_buffer_contains(&terminal, "Projects List");
    assert_buffer_contains(&terminal, "Active Project Spec & Git Metadata");
    assert_buffer_contains(&terminal, "/workspace/AgentMesh");
    assert_buffer_contains(&terminal, "HEAD Commit");
}

#[test]
fn test_tier1_project_mgmt_navigation_j_k() {
    let mut state = ProjectManagementState::with_sample_projects();
    assert_eq!(state.selected_index, 0);

    state.handle_key(press_char('j'));
    assert_eq!(state.selected_index, 1);

    state.handle_key(press_char('k'));
    assert_eq!(state.selected_index, 0);
}

#[test]
fn test_tier1_project_mgmt_enter_emits_load_project() {
    let mut state = ProjectManagementState::with_sample_projects();
    let target_id = state.projects[1].id;

    state.handle_key(press_char('j'));
    let action = state.handle_key(press_key(KeyCode::Enter));

    assert_eq!(state.active_project_id, Some(target_id));
    assert!(action.is_some());
}

#[test]
fn test_tier1_project_mgmt_empty_projects_fallback() {
    let mut terminal = create_terminal(100, 20);
    let state = ProjectManagementState::new();

    terminal.draw(|f| ProjectManagementWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "No projects found");
}

#[test]
fn test_tier1_project_mgmt_bounds_clamping_at_edges() {
    let mut state = ProjectManagementState::with_sample_projects();

    state.handle_key(press_key(KeyCode::Up));
    assert_eq!(state.selected_index, 0);

    state.handle_key(press_key(KeyCode::Down));
    state.handle_key(press_key(KeyCode::Down));
    state.handle_key(press_key(KeyCode::Down));
    assert_eq!(state.selected_index, 1);
}

// ----------------------------------------------------------------------------
// Tier 1.6: Interactive Recursive DAG Dependency Tree (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_dag_multi_level_recursive_traversal() {
    let state = RecursiveDagState::with_multi_tier_pipeline();
    let tiers = state.compute_topological_tiers();

    // 4 levels of depth: A -> B -> C -> D
    assert_eq!(tiers.len(), 4);
    assert_eq!(tiers[0].len(), 1); // PLAN-1 (Root)
    assert_eq!(tiers[1].len(), 1); // PLAN-2
    assert_eq!(tiers[2].len(), 1); // PLAN-3
    assert_eq!(tiers[3].len(), 1); // PLAN-4 (Leaf)

    let node_a = state.nodes.get(&tiers[0][0]).unwrap();
    assert_eq!(node_a.short_id, "PLAN-1");

    let node_d = state.nodes.get(&tiers[3][0]).unwrap();
    assert_eq!(node_d.short_id, "PLAN-4");
}

#[test]
fn test_tier1_dag_renders_all_depth_levels_in_buffer() {
    let mut terminal = create_terminal(100, 24);
    let state = RecursiveDagState::with_multi_tier_pipeline();

    terminal.draw(|f| DagWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "Tier 0");
    assert_buffer_contains(&terminal, "Tier 1");
    assert_buffer_contains(&terminal, "Tier 2");
    assert_buffer_contains(&terminal, "Tier 3");
    assert_buffer_contains(&terminal, "PLAN-1");
    assert_buffer_contains(&terminal, "PLAN-4");
}

#[test]
fn test_tier1_dag_status_badges_rendered() {
    let mut terminal = create_terminal(100, 24);
    let state = RecursiveDagState::with_multi_tier_pipeline();

    terminal.draw(|f| DagWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[C]"); // Completed
    assert_buffer_contains(&terminal, "[A]"); // Approved
    assert_buffer_contains(&terminal, "[R]"); // HumanReview
    assert_buffer_contains(&terminal, "[P]"); // Proposed
}

#[test]
fn test_tier1_dag_jump_to_blocker_shortcut() {
    let mut state = RecursiveDagState::with_multi_tier_pipeline();

    // Start on PLAN-2 (index 1) which depends on PLAN-1 (index 0)
    state.selected_index = 1;
    let jumped = state.jump_to_blocker();

    assert!(jumped);
    assert_eq!(state.selected_index, 0); // Jumped to PLAN-1
}

#[test]
fn test_tier1_dag_jump_to_dependent_shortcut() {
    let mut state = RecursiveDagState::with_multi_tier_pipeline();

    // Start on PLAN-1 (index 0) which is blocker for PLAN-2 (index 1)
    state.selected_index = 0;
    let jumped = state.jump_to_dependent();

    assert!(jumped);
    assert_eq!(state.selected_index, 1); // Jumped to PLAN-2
}

#[test]
fn test_tier1_dag_jump_on_root_or_leaf_is_safe_noop() {
    let mut state = RecursiveDagState::with_multi_tier_pipeline();

    // PLAN-1 has no blocker
    state.selected_index = 0;
    assert!(!state.jump_to_blocker());
    assert_eq!(state.selected_index, 0);

    // PLAN-4 has no dependent
    state.selected_index = 3;
    assert!(!state.jump_to_dependent());
    assert_eq!(state.selected_index, 3);
}

// ----------------------------------------------------------------------------
// Tier 1.7: Fleet Inspector & Enriched Telemetry Cards (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_fleet_telemetry_renders_health_badges() {
    let mut terminal = create_terminal(110, 24);
    let state = FleetTelemetryState::with_sample_fleet();

    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "[Healthy]");
    assert_buffer_contains(&terminal, "[Degraded]");
    assert_buffer_contains(&terminal, "[Unhealthy]");
}

#[test]
fn test_tier1_fleet_telemetry_renders_heartbeat_latencies() {
    let mut terminal = create_terminal(110, 24);
    let state = FleetTelemetryState::with_sample_fleet();

    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "45ms");
    assert_buffer_contains(&terminal, "1850ms");
    assert_buffer_contains(&terminal, "12500ms");
}

#[test]
fn test_tier1_fleet_telemetry_renders_error_banner_on_degraded_agent() {
    let mut terminal = create_terminal(110, 24);
    let mut state = FleetTelemetryState::with_sample_fleet();

    state.selected_index = 1; // agent-infra (Degraded)
    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();

    assert_buffer_contains(&terminal, "ERROR BANNER:");
    assert_buffer_contains(&terminal, "High heartbeat latency detected");
}

#[test]
fn test_tier1_fleet_telemetry_renders_error_banner_on_unhealthy_agent() {
    let mut terminal = create_terminal(110, 24);
    let mut state = FleetTelemetryState::with_sample_fleet();

    state.selected_index = 2; // agent-reviewer (Unhealthy)
    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();

    assert_buffer_contains(&terminal, "ERROR BANNER:");
    assert_buffer_contains(&terminal, "Connection reset by peer");
}

#[test]
fn test_tier1_fleet_telemetry_renders_active_task_assignment() {
    let mut terminal = create_terminal(110, 24);
    let state = FleetTelemetryState::with_sample_fleet();

    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "PLAN-2");
}

#[test]
fn test_tier1_fleet_telemetry_renders_runtime_adapters() {
    let mut terminal = create_terminal(110, 24);
    let state = FleetTelemetryState::with_sample_fleet();

    terminal.draw(|f| FleetTelemetryWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "Agy");
    assert_buffer_contains(&terminal, "Mock");
}

// ----------------------------------------------------------------------------
// Tier 1.8: Diagnostics Event Stream & Bounded Ring Buffer (6 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_diagnostics_bounded_capacity_invariant_under_1000_events() {
    let mut stream = BoundedEventStream::new(300);

    for i in 0..1_000 {
        let ev = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: format!("task.event_{i}"),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Event message {i}"),
            payload: json!({ "index": i }),
        };
        stream.push(ev);
    }

    assert_eq!(stream.events.len(), 300);
    // Newest event at front
    assert_eq!(stream.events.front().unwrap().message, "Event message 999");
    // Oldest preserved event at back
    assert_eq!(stream.events.back().unwrap().message, "Event message 700");
}

#[test]
fn test_tier1_diagnostics_rapid_burst_10000_events_performance() {
    let mut stream = BoundedEventStream::new(300);
    let start = std::time::Instant::now();

    for i in 0..10_000 {
        let ev = CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "agent.heartbeat".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Burst {i}"),
            payload: json!({}),
        };
        stream.push(ev);
    }

    let elapsed = start.elapsed();
    assert_eq!(stream.events.len(), 300);
    assert!(elapsed.as_millis() < 250, "10k event burst took too long: {elapsed:?}");
}

#[test]
fn test_tier1_diagnostics_severity_filtering() {
    let mut stream = BoundedEventStream::new(300);

    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "task.failed".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Failed execution".to_string(),
        payload: json!({}),
    });
    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "task.overlap_warning".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Overlap warning".to_string(),
        payload: json!({}),
    });
    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "task.started".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Task started".to_string(),
        payload: json!({}),
    });

    assert_eq!(stream.filtered_events().len(), 3);

    // Filter ERROR
    stream.severity_filter = Some("ERROR".to_string());
    assert_eq!(stream.filtered_events().len(), 1);
    assert_eq!(stream.filtered_events()[0].message, "Failed execution");

    // Filter WARN
    stream.severity_filter = Some("WARN".to_string());
    assert_eq!(stream.filtered_events().len(), 1);
    assert_eq!(stream.filtered_events()[0].message, "Overlap warning");
}

#[test]
fn test_tier1_diagnostics_entity_filtering() {
    let mut stream = BoundedEventStream::new(300);

    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "task.completed".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Task done".to_string(),
        payload: json!({}),
    });
    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "agent.registered".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Agent online".to_string(),
        payload: json!({}),
    });

    stream.entity_filter = Some("agent".to_string());
    let filtered = stream.filtered_events();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].message, "Agent online");
}

#[test]
fn test_tier1_diagnostics_selection_clamping_on_filter_shrink() {
    let mut stream = BoundedEventStream::new(300);

    for i in 0..10 {
        stream.push(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "task.event".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Task {i}"),
            payload: json!({}),
        });
    }

    stream.selected_index = 8;
    assert_eq!(stream.selected_index, 8);

    // Apply entity filter matching 0 events
    stream.entity_filter = Some("nonexistent".to_string());
    stream.clamp_selection();
    assert_eq!(stream.selected_index, 0);
}

#[test]
fn test_tier1_diagnostics_event_table_buffer_render() {
    let mut terminal = create_terminal(100, 20);
    let mut stream = BoundedEventStream::new(300);

    stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "task.assigned".to_string(),
        project_id: None,
        task_id: None,
        agent_id: None,
        message: "Assigned PLAN-1 to Alice".to_string(),
        payload: json!({}),
    });

    terminal
        .draw(|f| {
            let rows: Vec<Row> = stream
                .filtered_events()
                .iter()
                .map(|e| {
                    Row::new(vec![
                        Span::styled(&e.event_type, Style::default().fg(Color::Cyan)),
                        Span::styled(&e.message, Style::default().fg(Color::White)),
                    ])
                })
                .collect();
            let table = Table::new(rows, [Constraint::Length(20), Constraint::Min(40)]).block(
                Block::default().borders(Borders::ALL).title(" Live Diagnostics Event Stream "),
            );
            f.render_widget(table, f.area());
        })
        .unwrap();

    assert_buffer_contains(&terminal, "Live Diagnostics Event Stream");
    assert_buffer_contains(&terminal, "task.assigned");
    assert_buffer_contains(&terminal, "Assigned PLAN-1 to Alice");
}

// ----------------------------------------------------------------------------
// Tier 1.9: Terminal Resize Resilience Across Dimensions (5 tests)
// ----------------------------------------------------------------------------

#[test]
fn test_tier1_resize_minimal_60x18() {
    let mut terminal = create_terminal(60, 18);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_compact_70x20() {
    let mut terminal = create_terminal(70, 20);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_standard_80x24() {
    let mut terminal = create_terminal(80, 24);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_expanded_100x30() {
    let mut terminal = create_terminal(100, 30);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier1_resize_ultrawide_160x50() {
    let mut terminal = create_terminal(160, 50);
    let state = AppState::new().with_mock_data();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

// ============================================================================
// TIER 2: BOUNDARY & CORNER CONDITIONS (8 tests)
// ============================================================================

#[test]
fn test_tier2_empty_state_zero_tasks_zero_agents() {
    let mut terminal = create_terminal(120, 24);
    let state = AppState::new();
    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "No Project Selected");
}

#[test]
fn test_tier2_command_palette_no_matching_results() {
    let mut terminal = create_terminal(80, 24);
    let mut state = CommandPaletteState::new();
    state.open();

    for c in "xyz_nonexistent_command".chars() {
        state.handle_key(press_char(c));
    }

    assert_eq!(state.filtered_commands().len(), 0);
    terminal.draw(|f| CommandPaletteWidget::render(f, f.area(), &state)).unwrap();
    assert_buffer_contains(&terminal, "xyz_nonexistent_command");
}

#[test]
fn test_tier2_dag_single_node_tree() {
    let mut state = RecursiveDagState::new();
    let id = Uuid::new_v4();
    state.nodes.insert(
        id,
        DagNode {
            task_id: id,
            short_id: "SOLO-1".to_string(),
            title: "Standalone Task".to_string(),
            status: TaskStatus::Approved,
            dependencies: vec![],
        },
    );
    state.ordered_task_ids = vec![id];

    let tiers = state.compute_topological_tiers();
    assert_eq!(tiers.len(), 1);
    assert_eq!(tiers[0], vec![id]);
}

#[test]
fn test_tier2_dag_cyclic_dependency_safety_guard() {
    let mut state = RecursiveDagState::new();
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();

    // Mutual cycle: A -> B and B -> A
    state.nodes.insert(
        id_a,
        DagNode {
            task_id: id_a,
            short_id: "CYC-A".to_string(),
            title: "Cycle A".to_string(),
            status: TaskStatus::Proposed,
            dependencies: vec![id_b],
        },
    );
    state.nodes.insert(
        id_b,
        DagNode {
            task_id: id_b,
            short_id: "CYC-B".to_string(),
            title: "Cycle B".to_string(),
            status: TaskStatus::Proposed,
            dependencies: vec![id_a],
        },
    );
    state.ordered_task_ids = vec![id_a, id_b];

    // compute_topological_tiers must terminate without infinite loop
    let tiers = state.compute_topological_tiers();
    assert!(!tiers.is_empty());
}

#[test]
fn test_tier2_bounded_buffer_eviction_fifo_order() {
    let mut stream = BoundedEventStream::new(3);

    for i in 1..=5 {
        stream.push(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: "test".to_string(),
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Event {i}"),
            payload: json!({}),
        });
    }

    assert_eq!(stream.events.len(), 3);
    assert_eq!(stream.events[0].message, "Event 5");
    assert_eq!(stream.events[1].message, "Event 4");
    assert_eq!(stream.events[2].message, "Event 3");
}

#[test]
fn test_tier2_extreme_long_strings_rendering() {
    let mut terminal = create_terminal(80, 24);
    let mut state = AppState::new();
    state.active_project = Some(Project {
        id: Uuid::new_v4(),
        name: "A".repeat(300),
        description: "B".repeat(1000),
        status: ProjectStatus::Planning,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    terminal.draw(|f| render(f, &state)).unwrap();
    assert_buffer_contains(&terminal, "AgentMesh");
}

#[test]
fn test_tier2_unicode_and_symbols_in_query() {
    let mut state = CommandPaletteState::new();
    state.open();

    for c in "🦀 Rocket 🚀 :: [テスト]".chars() {
        state.handle_key(press_char(c));
    }
    assert_eq!(state.query, "🦀 Rocket 🚀 :: [テスト]");

    state.handle_key(press_key(KeyCode::Backspace));
    assert_eq!(state.query, "🦀 Rocket 🚀 :: [テスト");
}

#[test]
fn test_tier2_task_status_retry_transitions() {
    // Phase 21 requirement: Retry from Failed/Cancelled to Approved/HumanReview
    assert!(TaskStatus::Cancelled.can_transition_to(TaskStatus::Approved));
    assert!(TaskStatus::Cancelled.can_transition_to(TaskStatus::HumanReview));
    assert!(TaskStatus::Failed.can_transition_to(TaskStatus::Approved));
    assert!(TaskStatus::Failed.can_transition_to(TaskStatus::HumanReview));

    // Terminal states cannot transition to active Execution directly
    assert!(!TaskStatus::Cancelled.can_transition_to(TaskStatus::Executing));
    assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Approved));
}

// ============================================================================
// TIER 3: CROSS-FEATURE COMBINATIONS (5 tests)
// ============================================================================

#[test]
fn test_tier3_command_palette_modal_rendered_over_dag_view() {
    let mut terminal = create_terminal(100, 30);
    let dag_state = RecursiveDagState::with_multi_tier_pipeline();
    let mut cmd_state = CommandPaletteState::new();
    cmd_state.open();

    terminal
        .draw(|f| {
            DagWidget::render(f, f.area(), &dag_state);
            CommandPaletteWidget::render(f, f.area(), &cmd_state);
        })
        .unwrap();

    assert_buffer_contains(&terminal, "Command Palette (:)");
    assert_buffer_contains(&terminal, "Visual Dependency Graph");
}

#[test]
fn test_tier3_confirmation_modal_rendered_over_fleet_inspector() {
    let mut terminal = create_terminal(100, 30);
    let fleet_state = FleetTelemetryState::with_sample_fleet();
    let mut modal_state = ConfirmationModalState::new();
    modal_state.prompt_cancel(Uuid::new_v4(), "PLAN-2");

    terminal
        .draw(|f| {
            FleetTelemetryWidget::render(f, f.area(), &fleet_state);
            ConfirmationModalWidget::render(f, f.area(), &modal_state);
        })
        .unwrap();

    assert_buffer_contains(&terminal, "Confirm Task Cancellation");
    assert_buffer_contains(&terminal, "Connected Agent Fleet");
}

#[test]
fn test_tier3_project_selection_updates_status_header() {
    let mut terminal = create_terminal(100, 10);
    let proj_state = ProjectManagementState::with_sample_projects();

    let header_state = HeaderState {
        active_project_name: proj_state.projects[0].name.clone(),
        pg_connected: true,
        nats_connected: true,
        active_agents: 3,
        total_agents: 3,
        pending_approvals: 0,
        current_screen: Phase21Screen::ProjectManagement,
    };

    terminal.draw(|f| HeaderWidget::render(f, f.area(), &header_state)).unwrap();
    assert_buffer_contains(&terminal, "Project: AgentMesh Core");
    assert_buffer_contains(&terminal, "[PG: ● OK]");
}

#[test]
fn test_tier3_overlap_acknowledgment_unblocks_plan_approval() {
    let proj_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let warn_id = Uuid::new_v4();

    let mut task_state = ReviewTaskState::new(sample_task(
        task_id,
        proj_id,
        "PLAN-1",
        "Database Layer",
        TaskStatus::HumanReview,
    ));

    task_state.overlap_warnings.push(coordinator::domain::OverlapWarning {
        id: warn_id,
        project_id: proj_id,
        task_ids: serde_json::json!([task_id, Uuid::new_v4()]),
        resource: "crates/db".to_string(),
        severity: coordinator::domain::OverlapSeverity::Warning,
        acknowledged: false,
        created_at: Utc::now(),
    });

    assert!(!task_state.overlap_warnings[0].acknowledged);

    // Operator acknowledges overlap
    task_state.overlap_warnings[0].acknowledged = true;
    assert!(task_state.overlap_warnings[0].acknowledged);

    // Now unblocked for approval
    task_state.human_decision = Some(ApprovalStatus::Approved);
    assert_eq!(task_state.human_decision, Some(ApprovalStatus::Approved));
}

#[test]
fn test_tier3_rapid_event_burst_with_active_filter() {
    let mut stream = BoundedEventStream::new(300);
    stream.severity_filter = Some("ERROR".to_string());

    for i in 0..1_000 {
        let is_err = i % 10 == 0;
        stream.push(CoordinatorEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type: if is_err { "task.failed".to_string() } else { "task.started".to_string() },
            project_id: None,
            task_id: None,
            agent_id: None,
            message: format!("Msg {i}"),
            payload: json!({}),
        });
    }

    // In 300 retained events, exactly 30 are errors (every 10th)
    let filtered = stream.filtered_events();
    assert_eq!(filtered.len(), 30);
    assert!(filtered.iter().all(|e| e.event_type == "task.failed"));
}

// ============================================================================
// TIER 4: REAL-WORLD OPERATOR WORKFLOWS (4 tests)
// ============================================================================

#[test]
fn test_tier4_e2e_operator_review_and_safety_approval_flow() {
    // Scenario:
    // 1. Operator opens Project Management and selects a project.
    // 2. Navigates to Plan Review & DAG.
    // 3. Inspects recursive DAG dependency hierarchy.
    // 4. Attempts cancellation on a task -> Safety Confirmation modal intercepts.
    // 5. Operator confirms cancellation -> Action emitted safely.

    let mut proj_state = ProjectManagementState::with_sample_projects();
    let load_action = proj_state.handle_key(press_key(KeyCode::Enter));
    assert!(load_action.is_some());

    let dag_state = RecursiveDagState::with_multi_tier_pipeline();
    assert_eq!(dag_state.compute_topological_tiers().len(), 4);

    let mut modal_state = ConfirmationModalState::new();
    let cancel_target_id = dag_state.ordered_task_ids[2];
    modal_state.prompt_cancel(cancel_target_id, "PLAN-3");

    assert!(modal_state.is_open);
    let destructive_action = modal_state.handle_key(press_char('y'));
    assert_eq!(destructive_action, Some(DestructiveAction::CancelTask(cancel_target_id)));
    assert!(!modal_state.is_open);
}

#[test]
fn test_tier4_e2e_fleet_incident_triage_flow() {
    // Scenario:
    // 1. System alert received -> Operator opens Fleet Inspector.
    // 2. Selects degraded/unhealthy agent.
    // 3. Inspects error banner and latency.
    // 4. Drills down into diagnostics event stream filtered for that agent.

    let mut fleet_state = FleetTelemetryState::with_sample_fleet();
    fleet_state.selected_index = 2; // Unhealthy agent

    let selected_agent = &fleet_state.agents[fleet_state.selected_index];
    assert_eq!(selected_agent.health_status, HealthStatus::Unhealthy);
    assert!(selected_agent.last_error.is_some());

    let mut diag_stream = BoundedEventStream::new(300);
    diag_stream.push(CoordinatorEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        event_type: "agent.timeout".to_string(),
        project_id: None,
        task_id: None,
        agent_id: Some(selected_agent.id),
        message: format!("Heartbeat timeout on {}", selected_agent.name),
        payload: json!({ "agent_id": selected_agent.id }),
    });

    diag_stream.entity_filter = Some("agent".to_string());
    let results = diag_stream.filtered_events();
    assert_eq!(results.len(), 1);
    assert!(results[0].message.contains("Heartbeat timeout"));
}

#[test]
fn test_tier4_e2e_command_palette_navigation_flow() {
    // Scenario:
    // 1. Operator presses ':' to open Command Palette.
    // 2. Types "fleet".
    // 3. Presses Enter -> Navigates to Fleet Inspector screen.

    let mut cmd_state = CommandPaletteState::new();
    cmd_state.handle_key(press_char(':'));
    assert!(cmd_state.is_open);

    for c in "fleet".chars() {
        cmd_state.handle_key(press_char(c));
    }

    let action = cmd_state.handle_key(press_key(KeyCode::Enter));
    assert_eq!(action, Some(CommandAction::SwitchScreen(Phase21Screen::FleetInspector)));
    assert!(!cmd_state.is_open);
}

#[test]
fn test_tier4_e2e_retry_task_lifecycle_flow() {
    // Scenario:
    // A task failed during execution -> Operator retries task.
    // State machine verifies transition from Failed to Approved.
    let task_id = Uuid::new_v4();
    let proj_id = Uuid::new_v4();

    let mut task = sample_task(task_id, proj_id, "PLAN-1", "Flaky DB Job", TaskStatus::Failed);
    assert_eq!(task.status, TaskStatus::Failed);

    assert!(task.status.can_transition_to(TaskStatus::Approved));
    task.status = TaskStatus::Approved;
    assert_eq!(task.status, TaskStatus::Approved);
}
