use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use uuid::Uuid;

use crate::domain::{
    Agent, ApprovalStatus, OverlapWarning, Project, Task, TaskDependency, TaskStatus,
};

/// Maximum capacity of the bounded in-memory recent events ring buffer.
pub const MAX_RECENT_EVENTS: usize = 300;
pub const DEMO_PROJECT_NAME: &str = "AgentMesh Demo";
pub const DEMO_PROJECT_DESCRIPTION: &str =
    "Demonstrate AgentMesh project planning, human review, and dependency-aware task execution.";

/// Actions emitted by the TUI to be handled by the outer application or coordinator.
/// The TUI NEVER performs side effects (DB writes, NATS publishes, LLM requests) directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    /// Human approved a task
    ApproveTask { task_id: Uuid },
    /// Human approved all pending tasks in the current plan
    ApproveAllTasks,
    /// Human rejected a task
    RejectTask { task_id: Uuid },
    /// Human edited task description inline
    EditTaskDescription {
        task_id: Uuid,
        new_description: String,
    },
    /// Human submitted a new project for AI planning
    SubmitProject { name: String, description: String },
    /// Human acknowledged a detected resource overlap warning
    AcknowledgeOverlap { warning_id: Uuid },
    /// Human cancelled an active or assigned task
    CancelTask { task_id: Uuid },
    /// Request refresh of project / agent data from storage
    RefreshData,
    /// Human requested exit
    Quit,
    /// Human selected and requested loading an existing project
    LoadProject { project_id: Uuid },
    /// Human requested retrying a failed or cancelled task
    RetryTask { task_id: Uuid },
    /// Human requested deep timeline drilldown inspection for a task
    InspectTaskTimeline { task_id: Uuid },
}

/// Incoming real-time events sent to the TUI from NATS subscriber or coordinator.
#[derive(Debug, Clone)]
pub enum TuiUpdateEvent {
    AgentMessage(agent_protocol::AgentMessage),
    CoordinatorEvent(crate::coordinator::CoordinatorEvent),
    Tasks(Vec<ReviewTaskState>),
    Agents(Vec<Agent>),
    Overlaps(Vec<OverlapWarning>),
    StatusMessage(String),
    Metrics(crate::observability::SystemMetrics),
    Timeline(crate::observability::TaskTimeline),
    ObservabilityEvent(crate::observability::CoordinatorEvent),
    RecentEvents(Vec<crate::observability::CoordinatorEvent>),
    Projects(Vec<Project>),
    ActiveProject(Project),
    Connectivity { pg: bool, nats: bool },
}

/// Active top-level screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurrentScreen {
    #[default]
    Dashboard,
    ProjectInput,
    ProjectManagement,
    PlanReview,
    FleetInspector,
    Diagnostics,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Classic,
    Ocean,
    Forest,
    Sunset,
}

impl Theme {
    pub const ALL: [Theme; 4] = [Theme::Classic, Theme::Ocean, Theme::Forest, Theme::Sunset];
    pub fn name(self) -> &'static str {
        match self {
            Theme::Classic => "Classic",
            Theme::Ocean => "Ocean",
            Theme::Forest => "Forest",
            Theme::Sunset => "Sunset",
        }
    }
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|v| *v == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }
    pub fn previous(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|v| *v == self).unwrap_or(0) + Self::ALL.len() - 1)
            % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    pub const ALL: [FontStyle; 4] = [
        FontStyle::Regular,
        FontStyle::Bold,
        FontStyle::Italic,
        FontStyle::BoldItalic,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Regular => "Regular",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::BoldItalic => "Bold Italic",
        }
    }
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|v| *v == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }
    pub fn previous(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|v| *v == self).unwrap_or(0) + Self::ALL.len() - 1)
            % Self::ALL.len()]
    }
}

pub type Screen = CurrentScreen;

impl CurrentScreen {
    pub fn title(&self) -> &'static str {
        match self {
            CurrentScreen::Dashboard => "1. Dashboard",
            CurrentScreen::ProjectInput => "2. Project Input",
            CurrentScreen::ProjectManagement => "2. Project Management",
            CurrentScreen::PlanReview => "3. Plan Review",
            CurrentScreen::FleetInspector => "4. Fleet Inspector",
            CurrentScreen::Diagnostics => "5. Diagnostics",
            CurrentScreen::Settings => "6. Settings",
        }
    }
}

/// Destructive action requiring operator safety confirmation before execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestructiveAction {
    CancelTask { task_id: Uuid, short_id: String },
    RejectTask { task_id: Uuid, short_id: String },
    ApproveAllTasks,
}

/// State model for safety confirmation modal dialog
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationModalState {
    pub title: String,
    pub message: String,
    pub action: DestructiveAction,
}

impl Default for ConfirmationModalState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmationModalState {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            action: DestructiveAction::CancelTask {
                task_id: Uuid::nil(),
                short_id: String::new(),
            },
        }
    }

    pub fn cancel(task_id: Uuid, short_id: impl Into<String>) -> Self {
        let short = short_id.into();
        Self {
            title: "Confirm Task Cancellation".to_string(),
            message: format!("Are you sure you want to cancel task {short}?"),
            action: DestructiveAction::CancelTask {
                task_id,
                short_id: short,
            },
        }
    }

    pub fn reject(task_id: Uuid, short_id: impl Into<String>) -> Self {
        let short = short_id.into();
        Self {
            title: "Confirm Task Rejection".to_string(),
            message: format!("Are you sure you want to reject task {short}?"),
            action: DestructiveAction::RejectTask {
                task_id,
                short_id: short,
            },
        }
    }

    pub fn approve_all(pending_count: usize) -> Self {
        Self {
            title: "Confirm Bulk Task Approval".to_string(),
            message: format!(
                "Approve all {pending_count} pending tasks and dispatch to available agents?"
            ),
            action: DestructiveAction::ApproveAllTasks,
        }
    }

    pub fn approve_selected(count: usize) -> Self {
        Self {
            title: "Confirm Bulk Task Approval".to_string(),
            message: format!("Approve selected {count} task(s)?"),
            action: DestructiveAction::ApproveAllTasks,
        }
    }
}

/// Active focus pane on the Dashboard screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardFocus {
    #[default]
    AgentsTable,
    TasksTable,
    OverlapsTable,
}

/// Active focus pane on the PlanReview screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanReviewFocus {
    #[default]
    TaskList,
    TaskInspector,
}

impl PlanReviewFocus {
    #[allow(non_upper_case_globals)]
    pub const TaskCards: Self = Self::TaskList;
}

/// Active focus pane on the Diagnostics screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticsFocus {
    #[default]
    EventsStream,
    Timeline,
}

/// Current keyboard input mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    EditingDescription,
    EnteringProject,
}

/// In-memory view model for a task presented during plan review
#[derive(Debug, Clone)]
pub struct ReviewTaskState {
    pub task: Task,
    pub dependencies: Vec<TaskDependency>,
    pub suggested_agent_name: Option<String>,
    pub overlap_warnings: Vec<OverlapWarning>,
    pub human_decision: Option<ApprovalStatus>,
    pub progress_percent: Option<u8>,
}

impl ReviewTaskState {
    pub fn new(task: Task) -> Self {
        Self {
            task,
            dependencies: Vec::new(),
            suggested_agent_name: None,
            overlap_warnings: Vec::new(),
            human_decision: None,
            progress_percent: None,
        }
    }
}

/// Central in-memory state of the TUI application
pub struct AppState {
    pub current_screen: CurrentScreen,
    pub theme: Theme,
    pub font_style: FontStyle,
    pub settings_selected_index: usize,
    pub input_mode: InputMode,

    // Connectivity state
    pub pg_connected: bool,
    pub nats_connected: bool,

    // Project input fields
    pub project_name_input: String,
    pub project_desc_input: String,
    pub project_input_cursor: usize,
    /// 0 = Name, 1 = Description
    pub project_focus_field: usize,

    // Project management & context
    pub active_project: Option<Project>,
    pub available_projects: Vec<Project>,
    pub selected_project_index: usize,

    // Plan review state
    pub review_tasks: Vec<ReviewTaskState>,
    pub selected_task_index: usize,
    pub selected_task_indices: HashSet<usize>,
    pub show_details_pane: bool,
    pub show_dag_view: bool,
    pub dag_scroll_offset: usize,
    pub inspector_scroll_offset: usize,
    pub plan_review_focus: PlanReviewFocus,
    pub edit_buffer: String,
    pub edit_cursor: usize,

    // Dashboard / System context
    pub agents: Vec<Agent>,
    pub dashboard_focus: DashboardFocus,
    pub selected_agent_index: usize,
    pub selected_dashboard_task_index: usize,
    pub show_agent_inspector: bool,
    pub active_overlaps: Vec<OverlapWarning>,
    pub selected_overlap_index: usize,

    // Diagnostics / Observability
    pub metrics: Option<crate::observability::SystemMetrics>,
    pub recent_events: Vec<crate::observability::CoordinatorEvent>,
    pub event_ring_buffer: VecDeque<crate::observability::CoordinatorEvent>,
    pub selected_timeline: Option<crate::observability::TaskTimeline>,
    pub selected_event_index: usize,
    pub diagnostics_focus: DiagnosticsFocus,
    pub show_event_payload_modal: bool,
    pub show_timeline_modal: bool,
    pub event_filter_severity: Option<String>,
    pub event_filter_entity: Option<String>,

    // Command palette state
    pub show_command_palette: bool,
    pub command_palette_query: String,
    pub command_palette_selected_index: usize,

    // Safety confirmation dialog
    pub pending_confirmation: Option<ConfirmationModalState>,
    pub confirm_destructive_actions: bool,

    // Global modal dialogs
    pub show_help_modal: bool,

    // Status bar & messaging
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub viewport_size: AtomicU32,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_screen: CurrentScreen::Dashboard,
            theme: Theme::default(),
            font_style: FontStyle::default(),
            settings_selected_index: 0,
            input_mode: InputMode::Normal,

            pg_connected: false,
            nats_connected: false,

            project_name_input: String::new(),
            project_desc_input: String::new(),
            project_input_cursor: 0,
            project_focus_field: 0,

            active_project: None,
            available_projects: Vec::new(),
            selected_project_index: 0,

            review_tasks: Vec::new(),
            selected_task_index: 0,
            selected_task_indices: HashSet::new(),
            show_details_pane: true,
            show_dag_view: false,
            dag_scroll_offset: 0,
            inspector_scroll_offset: 0,
            plan_review_focus: PlanReviewFocus::TaskList,
            edit_buffer: String::new(),
            edit_cursor: 0,

            agents: Vec::new(),
            dashboard_focus: DashboardFocus::AgentsTable,
            selected_agent_index: 0,
            selected_dashboard_task_index: 0,
            show_agent_inspector: false,
            active_overlaps: Vec::new(),
            selected_overlap_index: 0,

            metrics: None,
            recent_events: Vec::new(),
            event_ring_buffer: VecDeque::new(),
            selected_timeline: None,
            selected_event_index: 0,
            diagnostics_focus: DiagnosticsFocus::EventsStream,
            show_event_payload_modal: false,
            show_timeline_modal: false,
            event_filter_severity: None,
            event_filter_entity: None,

            show_command_palette: false,
            command_palette_query: String::new(),
            command_palette_selected_index: 0,

            pending_confirmation: None,
            confirm_destructive_actions: true,

            show_help_modal: false,

            status_message: Some("Ready. Press [Tab] to switch screens.".to_string()),
            should_quit: false,
            viewport_size: AtomicU32::new(0),
        }
    }

    pub fn set_viewport_size(&self, width: u16, height: u16) {
        let packed = ((width as u32) << 16) | (height as u32);
        self.viewport_size.store(packed, Ordering::Relaxed);
    }

    pub fn get_viewport_size(&self) -> (u16, u16) {
        let packed = self.viewport_size.load(Ordering::Relaxed);
        if packed == 0 {
            crossterm::terminal::size().unwrap_or((80, 24))
        } else {
            ((packed >> 16) as u16, (packed & 0xFFFF) as u16)
        }
    }

    /// Calculates the sliding-window task list scrolling offset so the active task card
    /// is kept visible and centered without prematurely pinning to the bottom.
    pub fn plan_task_scroll_offset(&self, visible_cards: usize) -> usize {
        let half_visible = visible_cards / 2;
        let max_offset = self.review_tasks.len().saturating_sub(visible_cards);
        self.selected_task_index
            .saturating_sub(half_visible)
            .min(max_offset)
    }

    /// Test-only state builder for UI unit and integration testing.
    #[doc(hidden)]
    pub fn with_test_data(mut self) -> Self {
        let proj_id = Uuid::new_v4();
        let agent1_id = Uuid::new_v4();
        let agent2_id = Uuid::new_v4();

        self.active_project = Some(Project {
            id: proj_id,
            name: "Cloud Migration Mesh".to_string(),
            description: "Decompose and migrate monolith services to cloud native workers"
                .to_string(),
            status: crate::domain::ProjectStatus::Planning,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });

        let mut agent1 = Agent::new_for_test(
            agent1_id,
            "agent-backend (Alice)",
            crate::domain::AdapterType::Agy,
            vec!["backend".to_string(), "rust".to_string()],
            crate::domain::AgentStatus::Idle,
        );
        agent1.capability_profile = Some(
            serde_json::to_value(agent_protocol::AgentCapabilities {
                runtime: agent_protocol::RuntimeCapability {
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    adapter_type: "Agy".to_string(),
                    agy_version: Some("0.4.2".to_string()),
                    cpu_count: 8,
                    memory_mb: Some(16384),
                },
                languages: vec![agent_protocol::LanguageCapability::new(
                    "rust",
                    Some("1.80.0".to_string()),
                    vec!["tokio".to_string(), "axum".to_string(), "sqlx".to_string()],
                )],
                tools: vec![
                    agent_protocol::ToolCapability::new(
                        "cargo",
                        Some("1.80.0".to_string()),
                        Some("/usr/bin/cargo".to_string()),
                    ),
                    agent_protocol::ToolCapability::new(
                        "git",
                        Some("2.43.0".to_string()),
                        Some("/usr/bin/git".to_string()),
                    ),
                ],
                tags: vec![
                    "backend".to_string(),
                    "systems".to_string(),
                    "cloud".to_string(),
                ],
            })
            .unwrap(),
        );

        let mut agent2 = Agent::new_for_test(
            agent2_id,
            "agent-infra (Bob)",
            crate::domain::AdapterType::Agy,
            vec![
                "infra".to_string(),
                "nats".to_string(),
                "docker".to_string(),
            ],
            crate::domain::AgentStatus::Busy,
        );
        agent2.capability_profile = Some(
            serde_json::to_value(agent_protocol::AgentCapabilities {
                runtime: agent_protocol::RuntimeCapability {
                    os: "linux".to_string(),
                    arch: "x86_64".to_string(),
                    adapter_type: "Agy".to_string(),
                    agy_version: None,
                    cpu_count: 4,
                    memory_mb: Some(8192),
                },
                languages: vec![agent_protocol::LanguageCapability::new(
                    "python",
                    Some("3.11.4".to_string()),
                    vec!["nats-py".to_string(), "pytest".to_string()],
                )],
                tools: vec![
                    agent_protocol::ToolCapability::new(
                        "docker",
                        Some("24.0.5".to_string()),
                        Some("/usr/bin/docker".to_string()),
                    ),
                    agent_protocol::ToolCapability::new(
                        "nats",
                        Some("0.1.0".to_string()),
                        Some("/usr/local/bin/nats".to_string()),
                    ),
                ],
                tags: vec![
                    "infra".to_string(),
                    "nats".to_string(),
                    "docker".to_string(),
                ],
            })
            .unwrap(),
        );

        self.agents = vec![agent1, agent2];

        let prop_id = Uuid::new_v4();
        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();
        let task3_id = Uuid::new_v4();

        let task1 = Task {
            id: task1_id,
            project_id: proj_id,
            short_id: "PLAN-1".to_string(),
            title: "Database schema migration".to_string(),
            description: "Apply initial PostgreSQL schemas and verify RLS policies".to_string(),
            affected_resources: serde_json::json!(["migrations/001_init.sql", "crates/db"]),
            status: TaskStatus::HumanReview,
            assigned_agent_id: None,
            estimated_size: Some("M".to_string()),
            proposal_id: prop_id,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let task2 = Task {
            id: task2_id,
            project_id: proj_id,
            short_id: "PLAN-2".to_string(),
            title: "Implement REST and NATS endpoints".to_string(),
            description: "Add handlers for agent events and task streaming".to_string(),
            affected_resources: serde_json::json!(["crates/coordinator/src/messaging"]),
            status: TaskStatus::HumanReview,
            assigned_agent_id: None,
            estimated_size: Some("L".to_string()),
            proposal_id: prop_id,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let task3 = Task {
            id: task3_id,
            project_id: proj_id,
            short_id: "PLAN-3".to_string(),
            title: "Shared configuration refactor".to_string(),
            description: "Update database connection pooling and messaging settings".to_string(),
            affected_resources: serde_json::json!(["crates/db", "config/default.toml"]),
            status: TaskStatus::HumanReview,
            assigned_agent_id: None,
            estimated_size: Some("S".to_string()),
            proposal_id: prop_id,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let warning = OverlapWarning {
            id: Uuid::new_v4(),
            project_id: proj_id,
            resource: "crates/db".to_string(),
            task_ids: serde_json::json!([task1_id.to_string(), task3_id.to_string()]),
            severity: crate::domain::OverlapSeverity::Critical,
            acknowledged: false,
            created_at: chrono::Utc::now(),
        };

        self.active_overlaps = vec![warning.clone()];

        let mut review1 = ReviewTaskState::new(task1);
        review1.suggested_agent_name = Some("agent-backend".to_string());
        review1.overlap_warnings = vec![warning.clone()];
        review1.progress_percent = Some(60);

        let mut review2 = ReviewTaskState::new(task2);
        review2.suggested_agent_name = Some("agent-infra".to_string());
        review2.dependencies = vec![TaskDependency {
            dependent_id: task2_id,
            depends_on_id: task1_id,
            kind: crate::domain::DependencyKind::Blocks,
        }];

        let mut review3 = ReviewTaskState::new(task3);
        review3.suggested_agent_name = Some("agent-backend".to_string());
        review3.overlap_warnings = vec![warning];

        self.review_tasks = vec![review1, review2, review3];
        self.metrics = Some(crate::observability::SystemMetrics {
            total_tasks: 3,
            tasks_proposed: 2,
            tasks_approved: 1,
            tasks_executing: 1,
            tasks_completed: 0,
            tasks_failed: 0,
            tasks_blocked: 0,
            total_agents: 2,
            agents_idle: 1,
            agents_busy: 1,
            agents_offline: 0,
            agents_healthy: 2,
            agents_degraded: 0,
            agents_unhealthy: 0,
            total_deliveries: 1,
            successful_deliveries: 1,
            delivery_success_rate_percent: 100.0,
            total_conflicts: 1,
            total_audit_alerts: 0,
        });
        self.recent_events = vec![
            crate::observability::CoordinatorEvent {
                id: Uuid::new_v4(),
                timestamp: chrono::Utc::now(),
                event_type: "coordinator.started".to_string(),
                project_id: Some(proj_id),
                task_id: None,
                agent_id: None,
                message: "Coordinator engine started successfully.".to_string(),
                payload: serde_json::json!({ "version": "0.1.0" }),
            },
            crate::observability::CoordinatorEvent {
                id: Uuid::new_v4(),
                timestamp: chrono::Utc::now(),
                event_type: "task.overlap_detected".to_string(),
                project_id: Some(proj_id),
                task_id: Some(task1_id),
                agent_id: None,
                message: "Conflict warning detected on crates/db".to_string(),
                payload: serde_json::json!({ "resource": "crates/db" }),
            },
        ];
        self.pg_connected = true;
        self.nats_connected = true;
        if let Some(ref proj) = self.active_project {
            self.available_projects = vec![proj.clone()];
            self.selected_project_index = 0;
        }
        self.event_ring_buffer = VecDeque::from(self.recent_events.clone());
        self.confirm_destructive_actions = false;
        self.current_screen = CurrentScreen::PlanReview;
        self
    }

    /// Handles a mouse event (wheel scrolling, tab clicking) and returns an optional `TuiAction`.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<TuiAction> {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                match self.current_screen {
                    CurrentScreen::PlanReview => {
                        let (width, _height) = self.get_viewport_size();
                        let list_width = if self.show_details_pane {
                            if self.show_dag_view {
                                width * 50 / 100
                            } else if width < 90 {
                                width * 46 / 100
                            } else if width < 140 {
                                width * 44 / 100
                            } else {
                                64
                            }
                        } else {
                            width
                        };

                        if self.show_details_pane && mouse.column >= list_width {
                            self.inspector_scroll_down();
                        } else if self.show_dag_view {
                            self.dag_scroll_offset = self.dag_scroll_offset.saturating_add(1);
                        } else if !self.review_tasks.is_empty()
                            && self.selected_task_index + 1 < self.review_tasks.len()
                        {
                            self.selected_task_index += 1;
                            self.inspector_scroll_offset = 0;
                        }
                    }
                    CurrentScreen::Dashboard => match self.dashboard_focus {
                        DashboardFocus::AgentsTable => {
                            if !self.agents.is_empty()
                                && self.selected_agent_index + 1 < self.agents.len()
                            {
                                self.selected_agent_index += 1;
                            }
                        }
                        DashboardFocus::TasksTable => {
                            if !self.review_tasks.is_empty()
                                && self.selected_dashboard_task_index + 1 < self.review_tasks.len()
                            {
                                self.selected_dashboard_task_index += 1;
                            }
                        }
                        DashboardFocus::OverlapsTable => {
                            if !self.active_overlaps.is_empty()
                                && self.selected_overlap_index + 1 < self.active_overlaps.len()
                            {
                                self.selected_overlap_index += 1;
                            }
                        }
                    },
                    CurrentScreen::FleetInspector => {
                        if !self.agents.is_empty()
                            && self.selected_agent_index + 1 < self.agents.len()
                        {
                            self.selected_agent_index += 1;
                        }
                    }
                    CurrentScreen::Diagnostics => match self.diagnostics_focus {
                        DiagnosticsFocus::EventsStream => {
                            let count = self.filtered_events_count();
                            if count > 0 && self.selected_event_index + 1 < count {
                                self.selected_event_index += 1;
                            }
                        }
                        DiagnosticsFocus::Timeline => {
                            if !self.review_tasks.is_empty()
                                && self.selected_task_index + 1 < self.review_tasks.len()
                            {
                                self.selected_task_index += 1;
                                self.inspector_scroll_offset = 0;
                            }
                        }
                    },
                    CurrentScreen::ProjectManagement | CurrentScreen::ProjectInput => {
                        if !self.available_projects.is_empty()
                            && self.selected_project_index + 1 < self.available_projects.len()
                        {
                            self.selected_project_index += 1;
                        }
                    }
                    CurrentScreen::Settings => {}
                }
                None
            }
            MouseEventKind::ScrollUp => {
                match self.current_screen {
                    CurrentScreen::PlanReview => {
                        let (width, _height) = self.get_viewport_size();
                        let list_width = if self.show_details_pane {
                            if self.show_dag_view {
                                width * 50 / 100
                            } else if width < 90 {
                                width * 46 / 100
                            } else if width < 140 {
                                width * 44 / 100
                            } else {
                                64
                            }
                        } else {
                            width
                        };

                        if self.show_details_pane && mouse.column >= list_width {
                            self.inspector_scroll_up();
                        } else if self.show_dag_view {
                            self.dag_scroll_offset = self.dag_scroll_offset.saturating_sub(1);
                        } else if self.selected_task_index > 0 {
                            self.selected_task_index -= 1;
                            self.inspector_scroll_offset = 0;
                        }
                    }
                    CurrentScreen::Dashboard => match self.dashboard_focus {
                        DashboardFocus::AgentsTable => {
                            if self.selected_agent_index > 0 {
                                self.selected_agent_index -= 1;
                            }
                        }
                        DashboardFocus::TasksTable => {
                            if self.selected_dashboard_task_index > 0 {
                                self.selected_dashboard_task_index -= 1;
                            }
                        }
                        DashboardFocus::OverlapsTable => {
                            if self.selected_overlap_index > 0 {
                                self.selected_overlap_index -= 1;
                            }
                        }
                    },
                    CurrentScreen::FleetInspector => {
                        if self.selected_agent_index > 0 {
                            self.selected_agent_index -= 1;
                        }
                    }
                    CurrentScreen::Diagnostics => match self.diagnostics_focus {
                        DiagnosticsFocus::EventsStream => {
                            if self.selected_event_index > 0 {
                                self.selected_event_index -= 1;
                            }
                        }
                        DiagnosticsFocus::Timeline => {
                            if self.selected_task_index > 0 {
                                self.selected_task_index -= 1;
                            }
                        }
                    },
                    CurrentScreen::ProjectManagement | CurrentScreen::ProjectInput => {
                        if self.selected_project_index > 0 {
                            self.selected_project_index -= 1;
                        }
                    }
                    CurrentScreen::Settings => {}
                }
                None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // If top header area (rows 0..=2)
                if mouse.row <= 2 {
                    let col = if mouse.column >= 68 {
                        mouse.column.saturating_sub(68)
                    } else {
                        mouse.column
                    };

                    if col < 11 {
                        self.current_screen = CurrentScreen::Dashboard;
                    } else if col < 22 {
                        self.current_screen = CurrentScreen::ProjectManagement;
                    } else if col < 32 {
                        self.current_screen = CurrentScreen::PlanReview;
                    } else if col < 42 {
                        self.current_screen = CurrentScreen::FleetInspector;
                    } else if col < 55 {
                        self.current_screen = CurrentScreen::Diagnostics;
                    } else if col < 66 {
                        self.current_screen = CurrentScreen::Settings;
                    }
                } else if self.current_screen == CurrentScreen::PlanReview && !self.show_dag_view {
                    let (width, height) = self.get_viewport_size();
                    let header_height = if width < 90 { 2 } else { 3 };
                    let body_height = height.saturating_sub(header_height + 3);
                    let list_width = if self.show_details_pane {
                        if self.show_dag_view {
                            width * 50 / 100
                        } else if width < 90 {
                            width * 46 / 100
                        } else if width < 140 {
                            width * 44 / 100
                        } else {
                            64
                        }
                    } else {
                        width
                    };
                    let list_top = header_height + 1;
                    let list_inner_height = body_height.saturating_sub(2);

                    if mouse.column < list_width
                        && mouse.row >= list_top
                        && mouse.row < header_height + body_height - 1
                        && !self.review_tasks.is_empty()
                    {
                        let visible_cards = (list_inner_height / 5).max(1) as usize;
                        let scroll_offset = self.plan_task_scroll_offset(visible_cards);
                        let clicked_index = scroll_offset + ((mouse.row - list_top) / 5) as usize;
                        if clicked_index < self.review_tasks.len() {
                            if self.selected_task_index != clicked_index {
                                self.inspector_scroll_offset = 0;
                            }
                            self.selected_task_index = clicked_index;
                            if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                                if !self.selected_task_indices.insert(clicked_index) {
                                    self.selected_task_indices.remove(&clicked_index);
                                }
                            } else {
                                self.selected_task_indices.clear();
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Handles a keyboard event and returns an optional `TuiAction` for outer handling.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        // Discard key releases to avoid duplicate handling
        if key.kind != KeyEventKind::Press {
            return None;
        }

        // Global shortcuts: Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Some(TuiAction::Quit);
        }

        // Modal dialogs capture keys when open
        if self.show_help_modal {
            match key.code {
                KeyCode::Esc
                | KeyCode::Char('?')
                | KeyCode::Enter
                | KeyCode::Char('q')
                | KeyCode::Char('Q') => {
                    self.show_help_modal = false;
                }
                _ => {}
            }
            return None;
        }

        if self.show_agent_inspector {
            match key.code {
                KeyCode::Esc
                | KeyCode::Char('c')
                | KeyCode::Char('C')
                | KeyCode::Enter
                | KeyCode::Char('q')
                | KeyCode::Char('Q') => {
                    self.show_agent_inspector = false;
                }
                _ => {}
            }
            return None;
        }

        if self.show_event_payload_modal {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.show_event_payload_modal = false;
                }
                _ => {}
            }
            return None;
        }

        if self.show_timeline_modal {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
                    self.show_timeline_modal = false;
                }
                _ => {}
            }
            return None;
        }

        // Safety confirmation modal dialog captures keys when open
        if let Some(confirmation) = self.pending_confirmation.take() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    match confirmation.action {
                        DestructiveAction::CancelTask { task_id, short_id } => {
                            if let Some(item) =
                                self.review_tasks.iter_mut().find(|t| t.task.id == task_id)
                            {
                                item.task.status = TaskStatus::Cancelled;
                            }
                            self.status_message = Some(format!("Cancelling task {short_id}..."));
                            return Some(TuiAction::CancelTask { task_id });
                        }
                        DestructiveAction::RejectTask { task_id, short_id } => {
                            if let Some(item) =
                                self.review_tasks.iter_mut().find(|t| t.task.id == task_id)
                            {
                                item.human_decision = Some(ApprovalStatus::Rejected);
                                item.task.status = TaskStatus::Rejected;
                            }
                            self.status_message = Some(format!("Task {short_id} marked Rejected."));
                            return Some(TuiAction::RejectTask { task_id });
                        }
                        DestructiveAction::ApproveAllTasks => {
                            let mut count = 0;
                            if !self.selected_task_indices.is_empty() {
                                for &idx in &self.selected_task_indices {
                                    if let Some(item) = self.review_tasks.get_mut(idx) {
                                        if item.task.status == TaskStatus::HumanReview
                                            || item.task.status == TaskStatus::Proposed
                                        {
                                            item.human_decision = Some(ApprovalStatus::Approved);
                                            item.task.status = TaskStatus::Approved;
                                            count += 1;
                                        }
                                    }
                                }
                                self.selected_task_indices.clear();
                                self.status_message =
                                    Some(format!("Approved selected {count} task(s)."));
                            } else {
                                for item in &mut self.review_tasks {
                                    if item.task.status == TaskStatus::HumanReview
                                        || item.task.status == TaskStatus::Proposed
                                    {
                                        item.human_decision = Some(ApprovalStatus::Approved);
                                        item.task.status = TaskStatus::Approved;
                                        count += 1;
                                    }
                                }
                                self.status_message = Some(format!("Approved all {count} tasks."));
                            }
                            return Some(TuiAction::ApproveAllTasks);
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.status_message = Some("Action cancelled.".to_string());
                    return None;
                }
                _ => {
                    self.pending_confirmation = Some(confirmation);
                    return None;
                }
            }
        }

        // Command palette modal dialog captures keys when open
        if self.show_command_palette {
            return self.handle_command_palette_key(key);
        }

        if self.current_screen == CurrentScreen::Settings {
            match key.code {
                KeyCode::Esc | KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.current_screen = CurrentScreen::Dashboard
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.settings_selected_index = self.settings_selected_index.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.settings_selected_index = (self.settings_selected_index + 1).min(1)
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Enter => {
                    let forward = !matches!(key.code, KeyCode::Left);
                    if self.settings_selected_index == 0 {
                        self.theme = if forward {
                            self.theme.next()
                        } else {
                            self.theme.previous()
                        };
                    } else {
                        self.font_style = if forward {
                            self.font_style.next()
                        } else {
                            self.font_style.previous()
                        };
                    }
                }
                _ => return self.handle_normal_mode_key(key),
            }
            return None;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_mode_key(key),
            InputMode::EditingDescription => self.handle_editing_key(key),
            InputMode::EnteringProject => self.handle_project_input_key(key),
        }
    }

    fn handle_command_palette_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Esc => {
                self.show_command_palette = false;
                self.command_palette_query.clear();
                self.command_palette_selected_index = 0;
                None
            }
            KeyCode::Up => {
                if self.command_palette_selected_index > 0 {
                    self.command_palette_selected_index -= 1;
                }
                None
            }
            KeyCode::Down => {
                let count = crate::tui::widgets::command_palette::filtered_commands(
                    &self.command_palette_query,
                )
                .len();
                if count > 0 && self.command_palette_selected_index + 1 < count {
                    self.command_palette_selected_index += 1;
                }
                None
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.command_palette_selected_index > 0 {
                    self.command_palette_selected_index -= 1;
                }
                None
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let count = crate::tui::widgets::command_palette::filtered_commands(
                    &self.command_palette_query,
                )
                .len();
                if count > 0 && self.command_palette_selected_index + 1 < count {
                    self.command_palette_selected_index += 1;
                }
                None
            }
            KeyCode::Backspace => {
                self.command_palette_query.pop();
                self.command_palette_selected_index = 0;
                None
            }
            KeyCode::Enter => {
                let matches = crate::tui::widgets::command_palette::filtered_commands(
                    &self.command_palette_query,
                );
                self.show_command_palette = false;
                if let Some(cmd) = matches.get(self.command_palette_selected_index) {
                    match cmd.name.as_str() {
                        "nav:dashboard" => {
                            self.current_screen = CurrentScreen::Dashboard;
                            None
                        }
                        "nav:projects" => {
                            self.current_screen = CurrentScreen::ProjectManagement;
                            None
                        }
                        "nav:plan" => {
                            self.current_screen = CurrentScreen::PlanReview;
                            None
                        }
                        "nav:fleet" => {
                            self.current_screen = CurrentScreen::FleetInspector;
                            None
                        }
                        "nav:diagnostics" => {
                            self.current_screen = CurrentScreen::Diagnostics;
                            None
                        }
                        "nav:settings" => {
                            self.current_screen = CurrentScreen::Settings;
                            None
                        }
                        "demo:run" => Some(TuiAction::SubmitProject {
                            name: DEMO_PROJECT_NAME.to_string(),
                            description: DEMO_PROJECT_DESCRIPTION.to_string(),
                        }),
                        "task:approve" => {
                            if let Some(task) = self.selected_task() {
                                let task_id = task.task.id;
                                Some(TuiAction::ApproveTask { task_id })
                            } else {
                                None
                            }
                        }
                        "task:approve_all" => {
                            self.prompt_approve_all_tasks();
                            None
                        }
                        "task:reject" => {
                            if let Some(task) = self.selected_task() {
                                let task_id = task.task.id;
                                if self.confirm_destructive_actions {
                                    self.prompt_reject_task(task_id);
                                    None
                                } else {
                                    Some(TuiAction::RejectTask { task_id })
                                }
                            } else {
                                None
                            }
                        }
                        "task:cancel" => {
                            if let Some(task) = self.selected_task() {
                                let task_id = task.task.id;
                                if self.confirm_destructive_actions {
                                    self.prompt_cancel_task(task_id);
                                    None
                                } else {
                                    Some(TuiAction::CancelTask { task_id })
                                }
                            } else {
                                None
                            }
                        }
                        "task:retry" => {
                            if let Some(task) = self.selected_task() {
                                let task_id = task.task.id;
                                Some(TuiAction::RetryTask { task_id })
                            } else {
                                None
                            }
                        }
                        "task:edit" => {
                            if let Some(desc) =
                                self.selected_task().map(|t| t.task.description.clone())
                            {
                                self.input_mode = InputMode::EditingDescription;
                                self.edit_cursor = desc.len();
                                self.edit_buffer = desc;
                            }
                            None
                        }
                        "system:refresh" => Some(TuiAction::RefreshData),
                        "system:help" => {
                            self.show_help_modal = true;
                            None
                        }
                        "system:settings" => {
                            self.current_screen = CurrentScreen::Settings;
                            None
                        }
                        "system:quit" => {
                            self.should_quit = true;
                            Some(TuiAction::Quit)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            KeyCode::Char(c) => {
                self.command_palette_query.push(c);
                self.command_palette_selected_index = 0;
                None
            }
            _ => None,
        }
    }

    fn handle_normal_mode_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Char(':') => {
                self.show_command_palette = true;
                self.command_palette_query.clear();
                self.command_palette_selected_index = 0;
                None
            }
            KeyCode::Char('?') => {
                self.show_help_modal = true;
                None
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.current_screen = CurrentScreen::Settings;
                None
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.should_quit = true;
                Some(TuiAction::Quit)
            }
            KeyCode::Tab => {
                self.cycle_screen();
                None
            }
            KeyCode::BackTab => {
                self.cycle_screen_back();
                None
            }
            KeyCode::Char('[') => {
                if self.current_screen == CurrentScreen::PlanReview && self.show_dag_view {
                    self.jump_to_blocker();
                } else {
                    self.cycle_screen_back();
                }
                None
            }
            KeyCode::Char(']') => {
                if self.current_screen == CurrentScreen::PlanReview && self.show_dag_view {
                    self.jump_to_dependent();
                } else {
                    self.cycle_screen();
                }
                None
            }
            KeyCode::Char('1') => {
                self.current_screen = CurrentScreen::Dashboard;
                None
            }
            KeyCode::Char('2') => {
                self.current_screen = CurrentScreen::ProjectManagement;
                None
            }
            KeyCode::Char('3') => {
                self.current_screen = CurrentScreen::PlanReview;
                None
            }
            KeyCode::Char('4') => {
                self.current_screen = CurrentScreen::FleetInspector;
                None
            }
            KeyCode::Char('5') => {
                self.current_screen = CurrentScreen::Diagnostics;
                None
            }
            KeyCode::Char('6') => {
                self.current_screen = CurrentScreen::Settings;
                None
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if self.current_screen == CurrentScreen::PlanReview {
                    if let Some(item) = self.review_tasks.get(self.selected_task_index) {
                        if item.task.status == TaskStatus::Failed
                            || item.task.status == TaskStatus::Cancelled
                        {
                            let task_id = item.task.id;
                            let short_id = item.task.short_id.clone();
                            self.status_message = Some(format!("Retrying task {short_id}..."));
                            return Some(TuiAction::RetryTask { task_id });
                        }
                    }
                }
                self.status_message = Some("Refreshed data.".to_string());
                Some(TuiAction::RefreshData)
            }
            _ => match self.current_screen {
                CurrentScreen::PlanReview => self.handle_plan_review_key(key),
                CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_project_index > 0 {
                            self.selected_project_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.available_projects.is_empty()
                            && self.selected_project_index + 1 < self.available_projects.len()
                        {
                            self.selected_project_index += 1;
                        }
                        None
                    }
                    KeyCode::Enter => {
                        if !self.available_projects.is_empty() {
                            let proj = &self.available_projects[self.selected_project_index];
                            let project_id = proj.id;
                            self.active_project = Some(proj.clone());
                            self.status_message =
                                Some(format!("Loading project '{}'...", proj.name));
                            Some(TuiAction::LoadProject { project_id })
                        } else {
                            self.input_mode = InputMode::EnteringProject;
                            self.status_message = Some(
                                    "Entering project details. [Tab] switch field, [Enter] submit, [Esc] cancel."
                                        .to_string(),
                                );
                            None
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        if !self.available_projects.is_empty() {
                            let proj = &self.available_projects[self.selected_project_index];
                            let project_id = proj.id;
                            self.active_project = Some(proj.clone());
                            self.status_message =
                                Some(format!("Loading project '{}'...", proj.name));
                            Some(TuiAction::LoadProject { project_id })
                        } else {
                            None
                        }
                    }
                    KeyCode::Char('n')
                    | KeyCode::Char('N')
                    | KeyCode::Char('i')
                    | KeyCode::Char('I') => {
                        self.input_mode = InputMode::EnteringProject;
                        self.status_message = Some(
                                "Entering project details. [Tab] switch field, [Enter] submit, [Esc] cancel."
                                    .to_string(),
                            );
                        None
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        let project = self
                            .available_projects
                            .get(self.selected_project_index)
                            .or(self.active_project.as_ref())
                            .cloned();
                        if let Some(project) = project {
                            self.project_name_input = format!("{} (Copy)", project.name);
                            self.project_desc_input = project.description;
                            self.project_focus_field = 0;
                            self.project_input_cursor = self.project_name_input.chars().count();
                            self.input_mode = InputMode::EnteringProject;
                            self.status_message = Some("Project copied into the form. Edit the name or details, then submit to create a separate project.".to_string());
                        } else {
                            self.status_message = Some("Select a project to copy.".to_string());
                        }
                        None
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') => Some(TuiAction::SubmitProject {
                        name: DEMO_PROJECT_NAME.to_string(),
                        description: DEMO_PROJECT_DESCRIPTION.to_string(),
                    }),
                    _ => None,
                },
                CurrentScreen::Dashboard => self.handle_dashboard_key(key),
                CurrentScreen::FleetInspector => self.handle_fleet_inspector_key(key),
                CurrentScreen::Diagnostics => self.handle_diagnostics_key(key),
                CurrentScreen::Settings => None,
            },
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if !self.active_overlaps.is_empty()
                    && self.selected_overlap_index < self.active_overlaps.len()
                {
                    let warning = self.active_overlaps.remove(self.selected_overlap_index);
                    let warning_id = warning.id;
                    for rt in &mut self.review_tasks {
                        for w in &mut rt.overlap_warnings {
                            if w.id == warning_id {
                                w.acknowledged = true;
                            }
                        }
                    }
                    if self.selected_overlap_index >= self.active_overlaps.len()
                        && self.selected_overlap_index > 0
                    {
                        self.selected_overlap_index -= 1;
                    }
                    self.status_message =
                        Some(format!("Acknowledged overlap on '{}'.", warning.resource));
                    Some(TuiAction::AcknowledgeOverlap { warning_id })
                } else {
                    None
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                self.dashboard_focus = match self.dashboard_focus {
                    DashboardFocus::AgentsTable => DashboardFocus::OverlapsTable,
                    DashboardFocus::TasksTable => DashboardFocus::AgentsTable,
                    DashboardFocus::OverlapsTable => DashboardFocus::TasksTable,
                };
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.dashboard_focus = match self.dashboard_focus {
                    DashboardFocus::AgentsTable => DashboardFocus::TasksTable,
                    DashboardFocus::TasksTable => DashboardFocus::OverlapsTable,
                    DashboardFocus::OverlapsTable => DashboardFocus::AgentsTable,
                };
                None
            }
            _ => match self.dashboard_focus {
                DashboardFocus::AgentsTable => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_agent_index > 0 {
                            self.selected_agent_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.agents.is_empty()
                            && self.selected_agent_index + 1 < self.agents.len()
                        {
                            self.selected_agent_index += 1;
                        }
                        None
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Enter => {
                        if !self.agents.is_empty() {
                            self.show_agent_inspector = true;
                        }
                        None
                    }
                    _ => None,
                },
                DashboardFocus::TasksTable => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_dashboard_task_index > 0 {
                            self.selected_dashboard_task_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.review_tasks.is_empty()
                            && self.selected_dashboard_task_index + 1 < self.review_tasks.len()
                        {
                            self.selected_dashboard_task_index += 1;
                        }
                        None
                    }
                    KeyCode::Enter => {
                        if let Some(rt) = self.review_tasks.get(self.selected_dashboard_task_index)
                        {
                            self.status_message = Some(format!(
                                "Task [{}] {}: {:?} (Progress: {})",
                                rt.task.short_id,
                                rt.task.title,
                                rt.task.status,
                                rt.progress_percent
                                    .map(|p| format!("{p}%"))
                                    .unwrap_or_else(|| "N/A".to_string())
                            ));
                        }
                        None
                    }
                    _ => None,
                },
                DashboardFocus::OverlapsTable => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_overlap_index > 0 {
                            self.selected_overlap_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.active_overlaps.is_empty()
                            && self.selected_overlap_index + 1 < self.active_overlaps.len()
                        {
                            self.selected_overlap_index += 1;
                        }
                        None
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Enter => {
                        if !self.active_overlaps.is_empty()
                            && self.selected_overlap_index < self.active_overlaps.len()
                        {
                            let warning = self.active_overlaps.remove(self.selected_overlap_index);
                            let warning_id = warning.id;
                            for rt in &mut self.review_tasks {
                                for w in &mut rt.overlap_warnings {
                                    if w.id == warning_id {
                                        w.acknowledged = true;
                                    }
                                }
                            }
                            if self.selected_overlap_index >= self.active_overlaps.len()
                                && self.selected_overlap_index > 0
                            {
                                self.selected_overlap_index -= 1;
                            }
                            self.status_message =
                                Some(format!("Acknowledged overlap on '{}'.", warning.resource));
                            Some(TuiAction::AcknowledgeOverlap { warning_id })
                        } else {
                            None
                        }
                    }
                    _ => None,
                },
            },
        }
    }

    fn handle_fleet_inspector_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_agent_index > 0 {
                    self.selected_agent_index -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.agents.is_empty() && self.selected_agent_index + 1 < self.agents.len() {
                    self.selected_agent_index += 1;
                }
                None
            }
            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Enter => {
                if !self.agents.is_empty() {
                    self.show_agent_inspector = true;
                }
                None
            }
            _ => None,
        }
    }

    fn handle_diagnostics_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::BackTab => {
                self.diagnostics_focus = match self.diagnostics_focus {
                    DiagnosticsFocus::EventsStream => DiagnosticsFocus::Timeline,
                    DiagnosticsFocus::Timeline => DiagnosticsFocus::EventsStream,
                };
                None
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                let next = match self.event_filter_severity.as_deref() {
                    Some("ALL") | None => Some("ERROR".to_string()),
                    Some("ERROR") => Some("WARN".to_string()),
                    Some("WARN") => Some("INFO".to_string()),
                    Some("INFO") => Some("ALL".to_string()),
                    _ => Some("ALL".to_string()),
                };
                self.event_filter_severity = next;
                self.clamp_event_selection();
                None
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                let next = match self.event_filter_entity.as_deref() {
                    Some("ALL") | None => Some("TASK".to_string()),
                    Some("TASK") => Some("AGENT".to_string()),
                    Some("AGENT") => Some("COORDINATOR".to_string()),
                    Some("COORDINATOR") => Some("ALL".to_string()),
                    _ => Some("ALL".to_string()),
                };
                self.event_filter_entity = next;
                self.clamp_event_selection();
                None
            }
            _ => match self.diagnostics_focus {
                DiagnosticsFocus::EventsStream => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_event_index > 0 {
                            self.selected_event_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let count = self.filtered_events_count();
                        if count > 0 && self.selected_event_index + 1 < count {
                            self.selected_event_index += 1;
                        }
                        None
                    }
                    KeyCode::Enter => {
                        if self.filtered_events_count() > 0 {
                            self.show_event_payload_modal = true;
                        }
                        None
                    }
                    _ => None,
                },
                DiagnosticsFocus::Timeline => match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected_task_index > 0 {
                            self.selected_task_index -= 1;
                        }
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.review_tasks.is_empty()
                            && self.selected_task_index + 1 < self.review_tasks.len()
                        {
                            self.selected_task_index += 1;
                        }
                        None
                    }
                    KeyCode::Enter => {
                        if let Some(rt) = self.review_tasks.get(self.selected_task_index) {
                            let task_id = rt.task.id;
                            self.show_timeline_modal = true;
                            Some(TuiAction::InspectTaskTimeline { task_id })
                        } else {
                            None
                        }
                    }
                    _ => None,
                },
            },
        }
    }

    fn handle_plan_review_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        if key.code == KeyCode::Char('g') || key.code == KeyCode::Char('G') {
            self.show_dag_view = !self.show_dag_view;
            self.status_message = Some(if self.show_dag_view {
                "Switched to ASCII DAG Dependency Graph view. Press [G] to return to Task Cards."
                    .to_string()
            } else {
                "Switched to Task Cards view. Press [G] for DAG Graph view.".to_string()
            });
            return None;
        }

        // Shift+A approves all / selected; Ctrl+A selects all without performing an action.
        if key.code == KeyCode::Char('A') && !key.modifiers.contains(KeyModifiers::CONTROL) {
            self.prompt_approve_all_tasks();
            return None;
        }

        if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.selected_task_indices = (0..self.review_tasks.len()).collect();
            self.status_message = Some(format!("Selected all {} tasks.", self.review_tasks.len()));
            return None;
        }

        // Shift+Up/Down/J/K scrolls inspector regardless of pane focus if details pane is visible
        if key.modifiers.contains(KeyModifiers::SHIFT) && self.show_details_pane {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                    self.inspector_scroll_up();
                    return None;
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                    self.inspector_scroll_down();
                    return None;
                }
                _ => {}
            }
        }

        if self.review_tasks.is_empty() {
            return None;
        }

        match key.code {
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('h')
            | KeyCode::Char('l') => {
                if self.show_details_pane {
                    self.plan_review_focus = match self.plan_review_focus {
                        PlanReviewFocus::TaskList => PlanReviewFocus::TaskInspector,
                        PlanReviewFocus::TaskInspector => PlanReviewFocus::TaskList,
                    };
                }
                None
            }
            KeyCode::Char(' ') => {
                if !self.selected_task_indices.insert(self.selected_task_index) {
                    self.selected_task_indices.remove(&self.selected_task_index);
                }
                self.status_message = Some(format!(
                    "Selected {} task(s).",
                    self.selected_task_indices.len()
                ));
                None
            }
            KeyCode::Esc if !self.selected_task_indices.is_empty() => {
                self.selected_task_indices.clear();
                self.status_message = Some("Task selection cleared.".to_string());
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.plan_review_focus == PlanReviewFocus::TaskInspector {
                    self.inspector_scroll_up();
                } else if self.selected_task_index > 0 {
                    self.selected_task_index -= 1;
                    self.inspector_scroll_offset = 0;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.plan_review_focus == PlanReviewFocus::TaskInspector {
                    self.inspector_scroll_down();
                } else if self.selected_task_index + 1 < self.review_tasks.len() {
                    self.selected_task_index += 1;
                    self.inspector_scroll_offset = 0;
                }
                None
            }
            KeyCode::Enter => {
                self.show_details_pane = !self.show_details_pane;
                if !self.show_details_pane {
                    self.plan_review_focus = PlanReviewFocus::TaskList;
                }
                None
            }
            KeyCode::Char('a') => {
                if let Some(item) = self.review_tasks.get_mut(self.selected_task_index) {
                    if let Some(pos) = item.overlap_warnings.iter().position(|w| !w.acknowledged) {
                        let warning_id = item.overlap_warnings[pos].id;
                        let res = item.overlap_warnings[pos].resource.clone();
                        item.overlap_warnings[pos].acknowledged = true;
                        self.active_overlaps.retain(|w| w.id != warning_id);
                        self.status_message = Some(format!("Acknowledged overlap on '{res}'."));
                        return Some(TuiAction::AcknowledgeOverlap { warning_id });
                    }
                }
                None
            }

            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(item) = self.review_tasks.get_mut(self.selected_task_index) {
                    let task_id = item.task.id;
                    let short_id = item.task.short_id.clone();
                    item.human_decision = Some(ApprovalStatus::Approved);
                    item.task.status = TaskStatus::Approved;
                    self.status_message = Some(format!("Task {short_id} marked Approved."));

                    // Advance selection if not at end
                    if self.selected_task_index + 1 < self.review_tasks.len() {
                        self.selected_task_index += 1;
                        self.inspector_scroll_offset = 0;
                    }
                    Some(TuiAction::ApproveTask { task_id })
                } else {
                    None
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                if let Some(item) = self.review_tasks.get_mut(self.selected_task_index) {
                    let task_id = item.task.id;
                    if self.confirm_destructive_actions {
                        self.prompt_reject_task(task_id);
                        None
                    } else {
                        let short_id = item.task.short_id.clone();
                        item.human_decision = Some(ApprovalStatus::Rejected);
                        item.task.status = TaskStatus::Rejected;
                        self.status_message = Some(format!("Task {short_id} marked Rejected."));

                        if self.selected_task_index + 1 < self.review_tasks.len() {
                            self.selected_task_index += 1;
                            self.inspector_scroll_offset = 0;
                        }
                        Some(TuiAction::RejectTask { task_id })
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                if let Some(item) = self.review_tasks.get(self.selected_task_index) {
                    let current_desc = item.task.description.clone();
                    self.edit_buffer = current_desc;
                    self.edit_cursor = self.edit_buffer.len();
                    self.input_mode = InputMode::EditingDescription;
                    self.status_message = Some(
                        "Editing description inline. [Enter] save & approve, [Esc] cancel."
                            .to_string(),
                    );
                }
                None
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if let Some(item) = self.review_tasks.get(self.selected_task_index) {
                    let task_id = item.task.id;
                    if self.confirm_destructive_actions {
                        self.prompt_cancel_task(task_id);
                        None
                    } else {
                        let short_id = item.task.short_id.clone();
                        self.status_message = Some(format!("Cancelling task {short_id}..."));
                        Some(TuiAction::CancelTask { task_id })
                    }
                } else {
                    None
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(item) = self.review_tasks.get(self.selected_task_index) {
                    if item.task.status == TaskStatus::Failed
                        || item.task.status == TaskStatus::Cancelled
                    {
                        let task_id = item.task.id;
                        let short_id = item.task.short_id.clone();
                        self.status_message = Some(format!("Retrying task {short_id}..."));
                        return Some(TuiAction::RetryTask { task_id });
                    }
                }
                None
            }
            KeyCode::Char('[') | KeyCode::Char('p') | KeyCode::Char('P') => {
                self.jump_to_blocker();
                None
            }
            KeyCode::Char(']') | KeyCode::Char('d') | KeyCode::Char('D') => {
                self.jump_to_dependent();
                None
            }
            KeyCode::PageUp => {
                if self.plan_review_focus == PlanReviewFocus::TaskInspector {
                    self.inspector_scroll_offset = self.inspector_scroll_offset.saturating_sub(5);
                } else {
                    if self.selected_task_index >= 5 {
                        self.selected_task_index -= 5;
                    } else {
                        self.selected_task_index = 0;
                    }
                    self.inspector_scroll_offset = 0;
                    self.dag_scroll_offset = self.dag_scroll_offset.saturating_sub(5);
                }
                None
            }
            KeyCode::PageDown => {
                if self.plan_review_focus == PlanReviewFocus::TaskInspector {
                    self.inspector_scroll_offset = self.inspector_scroll_offset.saturating_add(5);
                } else {
                    if !self.review_tasks.is_empty() {
                        self.selected_task_index =
                            (self.selected_task_index + 5).min(self.review_tasks.len() - 1);
                    }
                    self.inspector_scroll_offset = 0;
                    self.dag_scroll_offset = self.dag_scroll_offset.saturating_add(5);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status_message = Some("Edit cancelled.".to_string());
                None
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                let new_desc = self.edit_buffer.trim().to_string();
                if let Some(item) = self.review_tasks.get_mut(self.selected_task_index) {
                    let task_id = item.task.id;
                    let short_id = item.task.short_id.clone();
                    item.task.description = new_desc.clone();
                    // Acceptance criteria: [E] edit description, Enter confirms edit + approves
                    item.human_decision = Some(ApprovalStatus::EditedAndApproved);
                    item.task.status = TaskStatus::Approved;
                    self.status_message =
                        Some(format!("Updated {short_id} description & marked Approved."));

                    Some(TuiAction::EditTaskDescription {
                        task_id,
                        new_description: new_desc,
                    })
                } else {
                    None
                }
            }
            KeyCode::Backspace => {
                if self.edit_cursor > 0 && !self.edit_buffer.is_empty() {
                    if let Some((prev_idx, _)) =
                        self.edit_buffer[..self.edit_cursor].char_indices().last()
                    {
                        self.edit_buffer.remove(prev_idx);
                        self.edit_cursor = prev_idx;
                    }
                }
                None
            }
            KeyCode::Left => {
                if self.edit_cursor > 0 {
                    if let Some((prev_idx, _)) =
                        self.edit_buffer[..self.edit_cursor].char_indices().last()
                    {
                        self.edit_cursor = prev_idx;
                    }
                }
                None
            }
            KeyCode::Right => {
                if self.edit_cursor < self.edit_buffer.len() {
                    if let Some(ch) = self.edit_buffer[self.edit_cursor..].chars().next() {
                        self.edit_cursor += ch.len_utf8();
                    }
                }
                None
            }
            KeyCode::Char(c) => {
                self.edit_buffer.insert(self.edit_cursor, c);
                self.edit_cursor += c.len_utf8();
                None
            }
            _ => None,
        }
    }

    fn handle_project_input_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status_message = Some("Project input cancelled.".to_string());
                None
            }
            KeyCode::Tab => {
                self.project_focus_field = (self.project_focus_field + 1) % 2;
                None
            }
            KeyCode::BackTab => {
                self.project_focus_field = if self.project_focus_field == 0 { 1 } else { 0 };
                None
            }
            KeyCode::Enter => {
                if self.project_name_input.trim().is_empty() {
                    self.status_message = Some("Project name cannot be empty.".to_string());
                    return None;
                }
                self.input_mode = InputMode::Normal;
                let name = self.project_name_input.trim().to_string();
                let desc = self.project_desc_input.trim().to_string();
                self.project_name_input.clear();
                self.project_desc_input.clear();
                self.project_focus_field = 0;
                self.project_input_cursor = 0;
                self.status_message =
                    Some(format!("Submitted project '{name}' for decomposition."));

                Some(TuiAction::SubmitProject {
                    name,
                    description: desc,
                })
            }
            KeyCode::Backspace => {
                if self.project_focus_field == 0 {
                    self.project_name_input.pop();
                } else {
                    self.project_desc_input.pop();
                }
                None
            }
            KeyCode::Char(c) => {
                if self.project_focus_field == 0 {
                    self.project_name_input.push(c);
                } else {
                    self.project_desc_input.push(c);
                }
                None
            }
            _ => None,
        }
    }

    pub fn cycle_screen(&mut self) {
        self.current_screen = match self.current_screen {
            CurrentScreen::Dashboard => CurrentScreen::ProjectManagement,
            CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => {
                CurrentScreen::PlanReview
            }
            CurrentScreen::PlanReview => CurrentScreen::FleetInspector,
            CurrentScreen::FleetInspector => CurrentScreen::Diagnostics,
            CurrentScreen::Diagnostics => CurrentScreen::Settings,
            CurrentScreen::Settings => CurrentScreen::Dashboard,
        };
    }

    pub fn cycle_screen_back(&mut self) {
        self.current_screen = match self.current_screen {
            CurrentScreen::Dashboard => CurrentScreen::Settings,
            CurrentScreen::Settings => CurrentScreen::Diagnostics,
            CurrentScreen::ProjectInput | CurrentScreen::ProjectManagement => {
                CurrentScreen::Dashboard
            }
            CurrentScreen::PlanReview => CurrentScreen::ProjectManagement,
            CurrentScreen::FleetInspector => CurrentScreen::PlanReview,
            CurrentScreen::Diagnostics => CurrentScreen::FleetInspector,
        };
    }

    /// Prompts confirmation modal for bulk approving all pending tasks or selected tasks
    pub fn prompt_approve_all_tasks(&mut self) {
        if !self.selected_task_indices.is_empty() {
            let count = self
                .selected_task_indices
                .iter()
                .filter_map(|&idx| self.review_tasks.get(idx))
                .filter(|t| {
                    t.task.status == TaskStatus::HumanReview
                        || t.task.status == TaskStatus::Proposed
                })
                .count();
            if count == 0 {
                self.status_message = Some("No pending tasks in selection to approve.".to_string());
                return;
            }
            self.pending_confirmation = Some(ConfirmationModalState::approve_selected(count));
        } else {
            let count = self
                .review_tasks
                .iter()
                .filter(|t| {
                    t.task.status == TaskStatus::HumanReview
                        || t.task.status == TaskStatus::Proposed
                })
                .count();
            if count == 0 {
                self.status_message = Some("No pending tasks to approve.".to_string());
                return;
            }
            self.pending_confirmation = Some(ConfirmationModalState::approve_all(count));
        }
    }

    pub fn selected_task(&self) -> Option<&ReviewTaskState> {
        self.review_tasks.get(self.selected_task_index)
    }

    /// Jump selection cursor to the first prerequisite/blocker of the currently selected task.
    pub fn jump_to_blocker(&mut self) -> bool {
        if let Some(current) = self.review_tasks.get(self.selected_task_index) {
            if let Some(first_blocker) = current.dependencies.first() {
                let blocker_id = first_blocker.depends_on_id;
                if let Some(pos) = self
                    .review_tasks
                    .iter()
                    .position(|t| t.task.id == blocker_id)
                {
                    self.selected_task_index = pos;
                    self.inspector_scroll_offset = 0;
                    if let Some(target) = self.review_tasks.get(pos) {
                        self.status_message = Some(format!(
                            "Jumped to prerequisite task {}.",
                            target.task.short_id
                        ));
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Jump selection cursor to the first downstream dependent task that waits on the currently selected task.
    pub fn jump_to_dependent(&mut self) -> bool {
        if let Some(current) = self.review_tasks.get(self.selected_task_index) {
            let current_id = current.task.id;
            for (idx, task) in self.review_tasks.iter().enumerate() {
                if task
                    .dependencies
                    .iter()
                    .any(|d| d.depends_on_id == current_id)
                {
                    self.selected_task_index = idx;
                    self.inspector_scroll_offset = 0;
                    self.status_message =
                        Some(format!("Jumped to dependent task {}.", task.task.short_id));
                    return true;
                }
            }
        }
        false
    }

    /// Scroll the inspector pane downwards by 1 unit
    pub fn inspector_scroll_down(&mut self) {
        self.inspector_scroll_offset = self.inspector_scroll_offset.saturating_add(1);
    }

    /// Scroll the inspector pane upwards by 1 unit
    pub fn inspector_scroll_up(&mut self) {
        self.inspector_scroll_offset = self.inspector_scroll_offset.saturating_sub(1);
    }

    /// Select a task by index and reset inspector scroll offset
    pub fn select_task(&mut self, index: usize) {
        if index < self.review_tasks.len() && self.selected_task_index != index {
            self.selected_task_index = index;
            self.inspector_scroll_offset = 0;
        }
    }

    /// Prompts confirmation modal for task cancellation
    pub fn prompt_cancel_task(&mut self, task_id: Uuid) {
        let short_id = self
            .review_tasks
            .iter()
            .find(|t| t.task.id == task_id)
            .map(|t| t.task.short_id.clone())
            .unwrap_or_else(|| {
                let s = task_id.to_string();
                s[..8.min(s.len())].to_string()
            });
        self.pending_confirmation = Some(ConfirmationModalState {
            title: "Confirm Task Cancellation".to_string(),
            message: format!("Are you sure you want to cancel task {short_id}?"),
            action: DestructiveAction::CancelTask { task_id, short_id },
        });
    }

    /// Prompts confirmation modal for task rejection
    pub fn prompt_reject_task(&mut self, task_id: Uuid) {
        let short_id = self
            .review_tasks
            .iter()
            .find(|t| t.task.id == task_id)
            .map(|t| t.task.short_id.clone())
            .unwrap_or_else(|| {
                let s = task_id.to_string();
                s[..8.min(s.len())].to_string()
            });
        self.pending_confirmation = Some(ConfirmationModalState {
            title: "Confirm Task Rejection".to_string(),
            message: format!("Are you sure you want to reject task {short_id}?"),
            action: DestructiveAction::RejectTask { task_id, short_id },
        });
    }

    /// Returns the number of currently active agents (Idle or Busy) alongside total agents.
    pub fn active_agent_count(&self) -> (usize, usize) {
        let total = self.agents.len();
        let active = self
            .agents
            .iter()
            .filter(|a| {
                matches!(
                    a.status,
                    crate::domain::AgentStatus::Idle | crate::domain::AgentStatus::Busy
                )
            })
            .count();
        (active, total)
    }

    /// Returns the number of tasks pending human approval.
    pub fn pending_approvals_count(&self) -> usize {
        self.review_tasks
            .iter()
            .filter(|rt| {
                matches!(
                    rt.task.status,
                    TaskStatus::HumanReview | TaskStatus::Proposed
                ) && rt.human_decision.is_none()
            })
            .count()
    }

    /// Clamps all selection and scroll indices safely to their valid bounds.
    pub fn clamp_all_indices(&mut self) {
        if self.review_tasks.is_empty() {
            self.selected_task_index = 0;
            self.selected_dashboard_task_index = 0;
        } else {
            if self.selected_task_index >= self.review_tasks.len() {
                self.selected_task_index = self.review_tasks.len().saturating_sub(1);
            }
            if self.selected_dashboard_task_index >= self.review_tasks.len() {
                self.selected_dashboard_task_index = self.review_tasks.len().saturating_sub(1);
            }
        }

        if self.agents.is_empty() {
            self.selected_agent_index = 0;
        } else if self.selected_agent_index >= self.agents.len() {
            self.selected_agent_index = self.agents.len().saturating_sub(1);
        }

        if self.active_overlaps.is_empty() {
            self.selected_overlap_index = 0;
        } else if self.selected_overlap_index >= self.active_overlaps.len() {
            self.selected_overlap_index = self.active_overlaps.len().saturating_sub(1);
        }

        if self.recent_events.is_empty() {
            self.selected_event_index = 0;
        } else if self.selected_event_index >= self.recent_events.len() {
            self.selected_event_index = self.recent_events.len().saturating_sub(1);
        }
        self.clamp_event_selection();

        if self.available_projects.is_empty() {
            self.selected_project_index = 0;
        } else if self.selected_project_index >= self.available_projects.len() {
            self.selected_project_index = self.available_projects.len().saturating_sub(1);
        }
    }

    /// Returns filtered references to recent events according to current multi-axis filters.
    pub fn filtered_recent_events(&self) -> Vec<&crate::observability::CoordinatorEvent> {
        let sev_filter = self.event_filter_severity.as_deref().unwrap_or("ALL");
        let ent_filter = self.event_filter_entity.as_deref().unwrap_or("ALL");

        self.recent_events
            .iter()
            .filter(|ev| {
                if !sev_filter.eq_ignore_ascii_case("ALL") {
                    let event_sev = if ev.event_type.contains("fail")
                        || ev.event_type.contains("error")
                        || ev.event_type.contains("alert")
                    {
                        "ERROR"
                    } else if ev.event_type.contains("warn")
                        || ev.event_type.contains("overlap")
                        || ev.event_type.contains("blocked")
                        || ev.event_type.contains("degraded")
                    {
                        "WARN"
                    } else {
                        "INFO"
                    };
                    if !event_sev.eq_ignore_ascii_case(sev_filter) {
                        return false;
                    }
                }

                if !ent_filter.eq_ignore_ascii_case("ALL") {
                    let matches_entity = match ent_filter.to_uppercase().as_str() {
                        "TASK" => {
                            ev.event_type.to_lowercase().starts_with("task") || ev.task_id.is_some()
                        }
                        "AGENT" => {
                            ev.event_type.to_lowercase().starts_with("agent")
                                || ev.agent_id.is_some()
                        }
                        "COORDINATOR" => {
                            (!ev.event_type.to_lowercase().starts_with("task")
                                && !ev.event_type.to_lowercase().starts_with("agent")
                                && ev.task_id.is_none()
                                && ev.agent_id.is_none())
                                || ev.event_type.to_lowercase().starts_with("coord")
                        }
                        other => ev
                            .event_type
                            .to_lowercase()
                            .starts_with(&other.to_lowercase()),
                    };
                    if !matches_entity {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// Count of events currently visible under active filters.
    pub fn filtered_events_count(&self) -> usize {
        self.filtered_recent_events().len()
    }

    /// Clamps selected_event_index safely to the filtered event count.
    pub fn clamp_event_selection(&mut self) {
        let count = self.filtered_events_count();
        if count == 0 {
            self.selected_event_index = 0;
        } else if self.selected_event_index >= count {
            self.selected_event_index = count - 1;
        }
    }

    /// Helper to insert a recent event at the front and keep up to bounded capacity.
    pub fn push_recent_event(&mut self, ev: crate::observability::CoordinatorEvent) {
        self.event_ring_buffer.push_front(ev.clone());
        if self.event_ring_buffer.len() > MAX_RECENT_EVENTS {
            self.event_ring_buffer.truncate(MAX_RECENT_EVENTS);
        }

        self.recent_events.insert(0, ev);
        if self.recent_events.len() > 200 {
            self.recent_events.truncate(200);
        }
    }

    /// Updates local in-memory state based on incoming AgentMessage protocol events.
    pub fn apply_agent_message(&mut self, msg: &agent_protocol::AgentMessage) {
        match msg {
            agent_protocol::AgentMessage::TaskStarted {
                agent_id,
                task_id,
                idempotency_key,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = TaskStatus::Executing;
                    rt.task.assigned_agent_id = Some(*agent_id);
                    rt.progress_percent = Some(0);
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Busy;
                    agent.current_task_id = Some(*task_id);
                }
                let display_name =
                    short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} is now EXECUTING."));

                let ev = crate::observability::CoordinatorEvent {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    event_type: "agent.task_started".to_string(),
                    project_id: self.active_project.as_ref().map(|p| p.id),
                    task_id: Some(*task_id),
                    agent_id: Some(*agent_id),
                    message: format!("Agent started task {display_name}"),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "task_id": task_id,
                        "idempotency_key": idempotency_key,
                    }),
                };
                self.push_recent_event(ev);
            }
            agent_protocol::AgentMessage::ProgressUpdate {
                task_id,
                percent,
                message,
                ..
            } => {
                let short_id = if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.progress_percent = Some(*percent);
                    rt.task.short_id.clone()
                } else {
                    task_id.to_string()[..8].to_string()
                };
                self.status_message = Some(format!("Task {short_id} ({percent}%): {message}"));

                let ev = crate::observability::CoordinatorEvent {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    event_type: "agent.progress_update".to_string(),
                    project_id: self.active_project.as_ref().map(|p| p.id),
                    task_id: Some(*task_id),
                    agent_id: None,
                    message: format!("Task {short_id} ({percent}%): {message}"),
                    payload: serde_json::json!({
                        "task_id": task_id,
                        "percent": percent,
                        "message": message,
                    }),
                };
                self.push_recent_event(ev);
            }
            agent_protocol::AgentMessage::Blocked {
                agent_id,
                task_id,
                reason,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = TaskStatus::Blocked;
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Blocked;
                }
                let display_name =
                    short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} is BLOCKED: {reason}"));

                let ev = crate::observability::CoordinatorEvent {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    event_type: "agent.task_blocked".to_string(),
                    project_id: self.active_project.as_ref().map(|p| p.id),
                    task_id: Some(*task_id),
                    agent_id: Some(*agent_id),
                    message: format!("Task {display_name} is BLOCKED: {reason}"),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "task_id": task_id,
                        "reason": reason,
                    }),
                };
                self.push_recent_event(ev);
            }
            agent_protocol::AgentMessage::Completed {
                agent_id,
                task_id,
                summary,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = TaskStatus::Completed;
                    rt.progress_percent = Some(100);
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Idle;
                    agent.current_task_id = None;
                }
                let display_name =
                    short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} COMPLETED: {summary}"));

                let ev = crate::observability::CoordinatorEvent {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    event_type: "agent.task_completed".to_string(),
                    project_id: self.active_project.as_ref().map(|p| p.id),
                    task_id: Some(*task_id),
                    agent_id: Some(*agent_id),
                    message: format!("Task {display_name} COMPLETED: {summary}"),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "task_id": task_id,
                        "summary": summary,
                    }),
                };
                self.push_recent_event(ev);
            }
            agent_protocol::AgentMessage::Failed {
                agent_id,
                task_id,
                error,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = TaskStatus::Failed;
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Error;
                    agent.current_task_id = None;
                }
                let display_name =
                    short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} FAILED: {error}"));

                let ev = crate::observability::CoordinatorEvent {
                    id: Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    event_type: "agent.task_failed".to_string(),
                    project_id: self.active_project.as_ref().map(|p| p.id),
                    task_id: Some(*task_id),
                    agent_id: Some(*agent_id),
                    message: format!("Task {display_name} FAILED: {error}"),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "task_id": task_id,
                        "error": error,
                    }),
                };
                self.push_recent_event(ev);
            }
            agent_protocol::AgentMessage::Heartbeat {
                agent_id,
                status,
                current_task_id,
                ..
            } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = match status {
                        agent_protocol::AgentStatus::Offline => crate::domain::AgentStatus::Offline,
                        agent_protocol::AgentStatus::Idle => crate::domain::AgentStatus::Idle,
                        agent_protocol::AgentStatus::Busy => crate::domain::AgentStatus::Busy,
                        agent_protocol::AgentStatus::Blocked => crate::domain::AgentStatus::Blocked,
                        agent_protocol::AgentStatus::Error => crate::domain::AgentStatus::Error,
                    };
                    agent.current_task_id = *current_task_id;
                    agent.last_seen = Some(chrono::Utc::now());
                }
            }
            _ => {}
        }
    }

    /// Updates local in-memory state based on coordinator events.
    pub fn apply_coordinator_event(&mut self, event: &crate::coordinator::CoordinatorEvent) {
        match event {
            crate::coordinator::CoordinatorEvent::TaskApproved { task_id } => {
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.human_decision = Some(ApprovalStatus::Approved);
                    rt.task.status = TaskStatus::Approved;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskRejected { task_id } => {
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.human_decision = Some(ApprovalStatus::Rejected);
                    rt.task.status = TaskStatus::Rejected;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskStatusChanged {
                task_id,
                new_status,
                ..
            } => {
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = *new_status;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskAssigned { task_id, agent_id } => {
                if let Some(rt) = self
                    .review_tasks
                    .iter_mut()
                    .find(|rt| rt.task.id == *task_id)
                {
                    rt.task.status = TaskStatus::Assigned;
                    rt.task.assigned_agent_id = Some(*agent_id);
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Busy;
                    agent.current_task_id = Some(*task_id);
                }
            }
            crate::coordinator::CoordinatorEvent::OverlapAcknowledged { warning_id } => {
                self.active_overlaps.retain(|w| w.id != *warning_id);
                for rt in &mut self.review_tasks {
                    for w in &mut rt.overlap_warnings {
                        if w.id == *warning_id {
                            w.acknowledged = true;
                        }
                    }
                }
                self.status_message = Some("Overlap warning acknowledged.".to_string());
            }
            crate::coordinator::CoordinatorEvent::TaskApprovalBlocked { reason, .. } => {
                self.status_message = Some(format!("Approval blocked: {reason}"));
            }
            crate::coordinator::CoordinatorEvent::CommandFailed { message } => {
                self.status_message = Some(format!("Command error: {message}"));
            }
            _ => {}
        }
    }

    /// Dispatches a high-level update event into state.
    pub fn apply_update(&mut self, event: TuiUpdateEvent) {
        match event {
            TuiUpdateEvent::AgentMessage(msg) => self.apply_agent_message(&msg),
            TuiUpdateEvent::CoordinatorEvent(evt) => self.apply_coordinator_event(&evt),
            TuiUpdateEvent::Tasks(tasks) => {
                self.review_tasks = tasks;
                self.selected_task_indices
                    .retain(|index| *index < self.review_tasks.len());
                if self.selected_task_index >= self.review_tasks.len() {
                    self.selected_task_index = self.review_tasks.len().saturating_sub(1);
                }
                if self.selected_dashboard_task_index >= self.review_tasks.len() {
                    self.selected_dashboard_task_index = self.review_tasks.len().saturating_sub(1);
                }
            }
            TuiUpdateEvent::Agents(agents) => {
                self.agents = agents;
                if self.selected_agent_index >= self.agents.len() {
                    self.selected_agent_index = self.agents.len().saturating_sub(1);
                }
            }
            TuiUpdateEvent::Overlaps(overlaps) => {
                self.active_overlaps = overlaps;
                if self.selected_overlap_index >= self.active_overlaps.len() {
                    self.selected_overlap_index = self.active_overlaps.len().saturating_sub(1);
                }
            }
            TuiUpdateEvent::StatusMessage(msg) => self.status_message = Some(msg),
            TuiUpdateEvent::Metrics(m) => self.metrics = Some(m),
            TuiUpdateEvent::Timeline(tl) => self.selected_timeline = Some(tl),
            TuiUpdateEvent::ObservabilityEvent(ev) => self.push_recent_event(ev),
            TuiUpdateEvent::RecentEvents(mut events) => {
                if events.len() > MAX_RECENT_EVENTS {
                    events.truncate(MAX_RECENT_EVENTS);
                }
                self.event_ring_buffer = VecDeque::from(events.clone());
                self.recent_events = events;
            }
            TuiUpdateEvent::Projects(projects) => {
                self.available_projects = projects;
                if self.selected_project_index >= self.available_projects.len() {
                    self.selected_project_index = self.available_projects.len().saturating_sub(1);
                }
            }
            TuiUpdateEvent::ActiveProject(project) => {
                self.active_project = Some(project);
            }
            TuiUpdateEvent::Connectivity { pg, nats } => {
                self.pg_connected = pg;
                self.nats_connected = nats;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_initial_app_state() {
        let state = AppState::new();
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);
        assert_eq!(state.input_mode, InputMode::Normal);
        assert!(!state.should_quit);
    }

    #[test]
    fn test_screen_navigation_cycle() {
        let mut state = AppState::new();
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::Settings);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);

        // Direct number keys (1:Dash, 2:Proj, 3:Plan, 4:Fleet, 5:Diag, 6:Settings)
        state.handle_key(make_key(KeyCode::Char('1')));
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);

        state.handle_key(make_key(KeyCode::Char('2')));
        assert_eq!(state.current_screen, CurrentScreen::ProjectManagement);

        state.handle_key(make_key(KeyCode::Char('3')));
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);

        state.handle_key(make_key(KeyCode::Char('4')));
        assert_eq!(state.current_screen, CurrentScreen::FleetInspector);

        state.handle_key(make_key(KeyCode::Char('5')));
        assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

        state.handle_key(make_key(KeyCode::Char('6')));
        assert_eq!(state.current_screen, CurrentScreen::Settings);
    }

    #[test]
    fn test_quit_action_emitted() {
        let mut state = AppState::new();
        let action = state.handle_key(make_key(KeyCode::Char('q')));
        assert_eq!(action, Some(TuiAction::Quit));
        assert!(state.should_quit);

        let mut state2 = AppState::new();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let action2 = state2.handle_key(ctrl_c);
        assert_eq!(action2, Some(TuiAction::Quit));
        assert!(state2.should_quit);
    }

    #[test]
    fn test_plan_review_navigation() {
        let mut state = AppState::new().with_test_data();
        assert_eq!(state.selected_task_index, 0);

        // Navigate Down
        state.handle_key(make_key(KeyCode::Down));
        assert_eq!(state.selected_task_index, 1);

        // Navigate Down with 'j'
        state.handle_key(make_key(KeyCode::Char('j')));
        assert_eq!(state.selected_task_index, 2);

        // Clamped at max index
        state.handle_key(make_key(KeyCode::Down));
        assert_eq!(state.selected_task_index, 2);

        // Navigate Up
        state.handle_key(make_key(KeyCode::Up));
        assert_eq!(state.selected_task_index, 1);

        // Navigate Up with 'k'
        state.handle_key(make_key(KeyCode::Char('k')));
        assert_eq!(state.selected_task_index, 0);

        // Clamped at 0
        state.handle_key(make_key(KeyCode::Up));
        assert_eq!(state.selected_task_index, 0);
    }

    #[test]
    fn test_ctrl_a_selects_all_tasks_and_space_toggles_without_approval() {
        let mut state = AppState::new().with_test_data();
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        state.handle_key(ctrl_a);
        assert_eq!(state.selected_task_indices.len(), state.review_tasks.len());
        assert!(state.pending_confirmation.is_none());

        state.handle_key(make_key(KeyCode::Char(' ')));
        assert!(!state
            .selected_task_indices
            .contains(&state.selected_task_index));
        assert!(state.pending_confirmation.is_none());

        state.handle_key(make_key(KeyCode::Char('A')));
        assert!(state.pending_confirmation.is_some());
    }

    #[test]
    fn test_mouse_click_selects_visible_plan_task_card() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::PlanReview;
        state.selected_task_index = 0;
        let click_second_card = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };

        state.handle_mouse(click_second_card);

        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_task_approval_emits_action_and_updates_local_status() {
        let mut state = AppState::new().with_test_data();
        let task0_id = state.review_tasks[0].task.id;

        let action = state.handle_key(make_key(KeyCode::Char('y')));
        assert_eq!(action, Some(TuiAction::ApproveTask { task_id: task0_id }));
        assert_eq!(
            state.review_tasks[0].human_decision,
            Some(ApprovalStatus::Approved)
        );
        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
        // Automatically advances to index 1
        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_task_rejection_emits_action_and_updates_local_status() {
        let mut state = AppState::new().with_test_data();
        let task0_id = state.review_tasks[0].task.id;

        let action = state.handle_key(make_key(KeyCode::Char('n')));
        assert_eq!(action, Some(TuiAction::RejectTask { task_id: task0_id }));
        assert_eq!(
            state.review_tasks[0].human_decision,
            Some(ApprovalStatus::Rejected)
        );
        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Rejected);
        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_task_edit_workflow_and_approval_confirmation() {
        let mut state = AppState::new().with_test_data();
        let task0_id = state.review_tasks[0].task.id;
        let original_desc = state.review_tasks[0].task.description.clone();

        // Press 'E' to enter edit mode
        let action = state.handle_key(make_key(KeyCode::Char('e')));
        assert_eq!(action, None);
        assert_eq!(state.input_mode, InputMode::EditingDescription);
        assert_eq!(state.edit_buffer, original_desc);

        // Type additional text: " (Revised)"
        for c in " (Revised)".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }
        assert!(state.edit_buffer.ends_with(" (Revised)"));

        // Press Enter to commit
        let commit_action = state.handle_key(make_key(KeyCode::Enter));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(
            commit_action,
            Some(TuiAction::EditTaskDescription {
                task_id: task0_id,
                new_description: format!("{original_desc} (Revised)"),
            })
        );
        assert_eq!(
            state.review_tasks[0].task.description,
            format!("{original_desc} (Revised)")
        );
        assert_eq!(
            state.review_tasks[0].human_decision,
            Some(ApprovalStatus::EditedAndApproved)
        );
    }

    #[test]
    fn test_task_edit_cancel_with_escape() {
        let mut state = AppState::new().with_test_data();
        let original_desc = state.review_tasks[0].task.description.clone();

        state.handle_key(make_key(KeyCode::Char('e')));
        state.handle_key(make_key(KeyCode::Char('X')));
        assert_eq!(state.input_mode, InputMode::EditingDescription);

        // Press Esc to discard
        let action = state.handle_key(make_key(KeyCode::Esc));
        assert_eq!(action, None);
        assert_eq!(state.input_mode, InputMode::Normal);
        // Original description remains intact
        assert_eq!(state.review_tasks[0].task.description, original_desc);
        assert_eq!(state.review_tasks[0].human_decision, None);
    }

    #[test]
    fn test_project_input_submission() {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::ProjectManagement;
        state.handle_key(make_key(KeyCode::Char('i')));
        assert_eq!(state.input_mode, InputMode::EnteringProject);

        // Type Project Name
        for c in "Mesh Proj".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }

        // Switch to Description field
        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.project_focus_field, 1);

        for c in "Test Desc".chars() {
            state.handle_key(make_key(KeyCode::Char(c)));
        }

        // Submit
        let action = state.handle_key(make_key(KeyCode::Enter));
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(
            action,
            Some(TuiAction::SubmitProject {
                name: "Mesh Proj".to_string(),
                description: "Test Desc".to_string(),
            })
        );
    }

    #[test]
    fn test_copy_project_prefills_creation_form_and_preserves_source() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::ProjectManagement;
        let source = state.available_projects[state.selected_project_index].clone();

        state.handle_key(make_key(KeyCode::Char('c')));

        assert_eq!(state.input_mode, InputMode::EnteringProject);
        assert_eq!(state.project_name_input, format!("{} (Copy)", source.name));
        assert_eq!(state.project_desc_input, source.description);
        assert_eq!(
            state.available_projects[state.selected_project_index].id,
            source.id
        );
        assert_eq!(state.project_focus_field, 0);
    }

    #[test]
    fn test_run_demo_shortcut_submits_builtin_demo_project() {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::ProjectManagement;

        assert_eq!(
            state.handle_key(make_key(KeyCode::Char('d'))),
            Some(TuiAction::SubmitProject {
                name: DEMO_PROJECT_NAME.to_string(),
                description: DEMO_PROJECT_DESCRIPTION.to_string(),
            })
        );
    }

    #[test]
    fn test_plan_review_overlap_acknowledgment() {
        let mut state = AppState::new().with_test_data();
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);
        assert_eq!(state.selected_task_index, 0);
        assert_eq!(state.review_tasks[0].overlap_warnings.len(), 1);
        assert!(!state.review_tasks[0].overlap_warnings[0].acknowledged);
        assert_eq!(state.active_overlaps.len(), 1);

        let warn_id = state.review_tasks[0].overlap_warnings[0].id;

        // Press 'a' to acknowledge overlap
        let action = state.handle_key(make_key(KeyCode::Char('a')));
        assert_eq!(
            action,
            Some(TuiAction::AcknowledgeOverlap {
                warning_id: warn_id
            })
        );
        assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);
        assert!(state.active_overlaps.is_empty());
    }

    #[test]
    fn test_dashboard_overlap_acknowledgment() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::Dashboard;
        assert_eq!(state.active_overlaps.len(), 1);

        let warn_id = state.active_overlaps[0].id;

        // Press 'a' in dashboard to acknowledge
        let action = state.handle_key(make_key(KeyCode::Char('a')));
        assert_eq!(
            action,
            Some(TuiAction::AcknowledgeOverlap {
                warning_id: warn_id
            })
        );
        assert!(state.active_overlaps.is_empty());
    }

    #[test]
    fn test_live_agent_message_projection() {
        let mut state = AppState::new().with_test_data();
        let agent_id = state.agents[0].id;
        let task_id = state.review_tasks[0].task.id;

        // Agent reports TaskStarted
        let start_msg = agent_protocol::AgentMessage::TaskStarted {
            agent_id,
            task_id,
            idempotency_key: "idem-1".to_string(),
            timestamp: chrono::Utc::now(),
        };
        state.apply_agent_message(&start_msg);

        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Executing);
        assert_eq!(state.review_tasks[0].task.assigned_agent_id, Some(agent_id));
        assert_eq!(state.agents[0].status, crate::domain::AgentStatus::Busy);
        assert_eq!(state.agents[0].current_task_id, Some(task_id));

        // Agent reports Completed
        let complete_msg = agent_protocol::AgentMessage::Completed {
            agent_id,
            task_id,
            summary: "Done successfully".to_string(),
            timestamp: chrono::Utc::now(),
        };
        state.apply_agent_message(&complete_msg);

        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Completed);
        assert_eq!(state.agents[0].status, crate::domain::AgentStatus::Idle);
        assert_eq!(state.agents[0].current_task_id, None);
    }

    #[test]
    fn test_phase21_screen_hierarchy_and_aliases() {
        let mut state = AppState::new();
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);
        assert_eq!(state.current_screen.title(), "1. Dashboard");

        state.current_screen = CurrentScreen::ProjectManagement;
        assert_eq!(state.current_screen.title(), "2. Project Management");

        state.current_screen = CurrentScreen::ProjectInput;
        assert_eq!(state.current_screen.title(), "2. Project Input");

        state.current_screen = CurrentScreen::FleetInspector;
        assert_eq!(state.current_screen.title(), "4. Fleet Inspector");

        let screen_alias: Screen = CurrentScreen::Dashboard;
        assert_eq!(screen_alias, CurrentScreen::Dashboard);
    }

    #[test]
    fn test_phase21_connectivity_and_project_updates() {
        let mut state = AppState::new();
        assert!(!state.pg_connected);
        assert!(!state.nats_connected);

        state.apply_update(TuiUpdateEvent::Connectivity {
            pg: true,
            nats: true,
        });
        assert!(state.pg_connected);
        assert!(state.nats_connected);

        let proj = Project {
            id: Uuid::new_v4(),
            name: "New Test Project".to_string(),
            description: "Test description".to_string(),
            status: crate::domain::ProjectStatus::Planning,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        state.apply_update(TuiUpdateEvent::Projects(vec![proj.clone()]));
        assert_eq!(state.available_projects.len(), 1);
        assert_eq!(state.selected_project_index, 0);

        state.apply_update(TuiUpdateEvent::ActiveProject(proj.clone()));
        assert_eq!(state.active_project.as_ref().map(|p| p.id), Some(proj.id));
    }

    #[test]
    fn test_phase21_active_agent_count_and_pending_approvals() {
        let state = AppState::new().with_test_data();
        // Agy data has 2 agents: 1 Idle, 1 Busy
        let (active, total) = state.active_agent_count();
        assert_eq!(total, 2);
        assert_eq!(active, 2);

        // Agy data has 3 tasks in HumanReview with no human_decision
        assert_eq!(state.pending_approvals_count(), 3);
    }

    #[test]
    fn test_phase21_clamp_all_indices_helper() {
        let mut state = AppState::new().with_test_data();
        state.selected_task_index = 999;
        state.selected_dashboard_task_index = 999;
        state.selected_agent_index = 999;
        state.selected_overlap_index = 999;
        state.selected_event_index = 999;
        state.selected_project_index = 999;

        state.clamp_all_indices();

        assert_eq!(state.selected_task_index, state.review_tasks.len() - 1);
        assert_eq!(
            state.selected_dashboard_task_index,
            state.review_tasks.len() - 1
        );
        assert_eq!(state.selected_agent_index, state.agents.len() - 1);
        assert_eq!(
            state.selected_overlap_index,
            state.active_overlaps.len() - 1
        );
        assert_eq!(state.selected_event_index, state.recent_events.len() - 1);
        assert_eq!(
            state.selected_project_index,
            state.available_projects.len() - 1
        );
    }

    #[test]
    fn test_phase21_recent_events_clamped_on_apply_update() {
        let mut state = AppState::new();
        let large_batch: Vec<crate::observability::CoordinatorEvent> = (0..500)
            .map(|i| crate::observability::CoordinatorEvent {
                id: Uuid::new_v4(),
                timestamp: chrono::Utc::now(),
                event_type: "burst.test".to_string(),
                project_id: None,
                task_id: None,
                agent_id: None,
                message: format!("Burst #{i}"),
                payload: serde_json::json!({ "i": i }),
            })
            .collect();

        state.apply_update(TuiUpdateEvent::RecentEvents(large_batch));
        assert!(state.event_ring_buffer.len() <= MAX_RECENT_EVENTS);
        assert_eq!(state.event_ring_buffer.len(), 300);
        assert!(state.recent_events.len() <= MAX_RECENT_EVENTS);
        assert_eq!(state.recent_events.len(), 300);
    }

    #[test]
    fn test_phase21_confirmation_modal_and_new_actions() {
        let mut state = AppState::new();
        let task_id = Uuid::new_v4();
        let proj_id = Uuid::new_v4();

        state.pending_confirmation = Some(ConfirmationModalState {
            title: "Confirm Cancellation".to_string(),
            message: "Are you sure you want to cancel this task?".to_string(),
            action: DestructiveAction::CancelTask {
                task_id,
                short_id: "PLAN-1".to_string(),
            },
        });

        assert!(state.pending_confirmation.is_some());
        assert_eq!(
            state.pending_confirmation.as_ref().unwrap().action,
            DestructiveAction::CancelTask {
                task_id,
                short_id: "PLAN-1".to_string()
            }
        );

        let action1 = TuiAction::LoadProject {
            project_id: proj_id,
        };
        let action2 = TuiAction::RetryTask { task_id };
        let action3 = TuiAction::InspectTaskTimeline { task_id };

        assert_eq!(
            action1,
            TuiAction::LoadProject {
                project_id: proj_id
            }
        );
        assert_eq!(action2, TuiAction::RetryTask { task_id });
        assert_eq!(action3, TuiAction::InspectTaskTimeline { task_id });
    }

    #[test]
    fn test_phase21_dag_jump_blocker_and_dependent() {
        let mut state = AppState::new().with_test_data();
        // In agy data:
        // task 0: Schema Migration (no deps)
        // task 1: User Service (depends on task 0)
        // task 2: Order Service (depends on task 0 and task 1)
        state.selected_task_index = 1;
        assert!(state.jump_to_blocker());
        assert_eq!(state.selected_task_index, 0);

        // At root, jump to blocker is safe no-op
        assert!(!state.jump_to_blocker());
        assert_eq!(state.selected_task_index, 0);

        // Jump to dependent
        assert!(state.jump_to_dependent());
        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_phase21_project_mgmt_key_navigation_and_load() {
        let mut state = AppState::new();
        state.current_screen = CurrentScreen::ProjectManagement;
        let p1 = Project {
            id: Uuid::new_v4(),
            name: "Project Alpha".to_string(),
            description: "Desc Alpha".to_string(),
            status: crate::domain::ProjectStatus::Active,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let p2 = Project {
            id: Uuid::new_v4(),
            name: "Project Beta".to_string(),
            description: "Desc Beta".to_string(),
            status: crate::domain::ProjectStatus::Planning,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        state.available_projects = vec![p1.clone(), p2.clone()];
        assert_eq!(state.selected_project_index, 0);

        // Navigate down with 'j'
        state.handle_key(make_key(KeyCode::Char('j')));
        assert_eq!(state.selected_project_index, 1);

        // Navigate up with 'k'
        state.handle_key(make_key(KeyCode::Char('k')));
        assert_eq!(state.selected_project_index, 0);

        // Press Enter to load project
        let action = state.handle_key(make_key(KeyCode::Enter));
        assert_eq!(action, Some(TuiAction::LoadProject { project_id: p1.id }));
        assert_eq!(state.active_project.as_ref().map(|p| p.id), Some(p1.id));
    }

    #[test]
    fn test_phase21_plan_review_retry_shortcut() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::PlanReview;
        let task_id = state.review_tasks[0].task.id;

        // When task is in HumanReview, 'r' does not retry, it refreshes data
        let action = state.handle_key(make_key(KeyCode::Char('r')));
        assert_eq!(action, Some(TuiAction::RefreshData));

        // When task is Failed, 'r' emits RetryTask
        state.review_tasks[0].task.status = TaskStatus::Failed;
        let retry_action = state.handle_key(make_key(KeyCode::Char('r')));
        assert_eq!(retry_action, Some(TuiAction::RetryTask { task_id }));

        // When task is Cancelled, 'r' also emits RetryTask
        state.review_tasks[0].task.status = TaskStatus::Cancelled;
        let retry_action2 = state.handle_key(make_key(KeyCode::Char('r')));
        assert_eq!(retry_action2, Some(TuiAction::RetryTask { task_id }));
    }

    #[test]
    fn test_phase21_confirmation_dialog_cancel_and_confirm_in_handle_key() {
        let mut state = AppState::new().with_test_data();
        let task_id = state.review_tasks[0].task.id;

        // Prompt cancel confirmation
        state.prompt_cancel_task(task_id);
        assert!(state.pending_confirmation.is_some());

        // Cancel with 'n'
        let action_cancel = state.handle_key(make_key(KeyCode::Char('n')));
        assert_eq!(action_cancel, None);
        assert!(state.pending_confirmation.is_none());

        // Prompt reject confirmation
        state.prompt_reject_task(task_id);
        assert!(state.pending_confirmation.is_some());

        // Confirm with 'y'
        let action_confirm = state.handle_key(make_key(KeyCode::Char('y')));
        assert_eq!(action_confirm, Some(TuiAction::RejectTask { task_id }));
        assert!(state.pending_confirmation.is_none());
    }

    #[test]
    fn test_phase21_topological_tiers_computation() {
        let state = AppState::new().with_test_data();
        let tiers = crate::tui::widgets::DagViewWidget::compute_topological_tiers(&state);

        // Root tier must contain task 0 (PLAN-1)
        assert!(!tiers.is_empty());
        assert!(tiers[0].contains(&state.review_tasks[0].task.id));
    }

    #[test]
    fn test_phase22_plan_review_focus_and_inspector_scroll() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::PlanReview;
        assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskList);
        assert_eq!(state.inspector_scroll_offset, 0);

        // Toggle focus with Right / 'l'
        state.handle_plan_review_key(make_key(KeyCode::Right));
        assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskInspector);

        // When inspector is focused, Up/Down/j/k scroll inspector offset
        state.handle_plan_review_key(make_key(KeyCode::Char('j')));
        assert_eq!(state.inspector_scroll_offset, 1);
        assert_eq!(state.selected_task_index, 0);

        state.handle_plan_review_key(make_key(KeyCode::Down));
        assert_eq!(state.inspector_scroll_offset, 2);

        state.handle_plan_review_key(make_key(KeyCode::PageDown));
        assert_eq!(state.inspector_scroll_offset, 7);

        state.handle_plan_review_key(make_key(KeyCode::PageUp));
        assert_eq!(state.inspector_scroll_offset, 2);

        state.handle_plan_review_key(make_key(KeyCode::Char('k')));
        assert_eq!(state.inspector_scroll_offset, 1);

        state.handle_plan_review_key(make_key(KeyCode::Up));
        assert_eq!(state.inspector_scroll_offset, 0);

        // Saturation at 0
        state.handle_plan_review_key(make_key(KeyCode::Up));
        assert_eq!(state.inspector_scroll_offset, 0);

        // Toggle back with Left / 'h'
        state.handle_plan_review_key(make_key(KeyCode::Left));
        assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskList);

        // When TaskList is focused, Down/j navigates task selection
        state.inspector_scroll_offset = 5;
        state.handle_plan_review_key(make_key(KeyCode::Char('j')));
        assert_eq!(state.selected_task_index, 1);
        assert_eq!(state.inspector_scroll_offset, 0); // auto-reset!

        // Shift+Down / Shift+J scrolls inspector regardless of pane focus
        let shift_down = KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT);
        state.handle_plan_review_key(shift_down);
        assert_eq!(state.inspector_scroll_offset, 1);

        let shift_up = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
        state.handle_plan_review_key(shift_up);
        assert_eq!(state.inspector_scroll_offset, 0);

        // When show_details_pane is false, focus toggle does not switch to inspector
        state.show_details_pane = false;
        state.handle_plan_review_key(make_key(KeyCode::Right));
        assert_eq!(state.plan_review_focus, PlanReviewFocus::TaskList);
    }

    #[test]
    fn test_phase22_mouse_scroll_plan_review_inspector_and_cards() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::PlanReview;
        state.show_details_pane = true;
        assert_eq!(state.selected_task_index, 0);
        assert_eq!(state.inspector_scroll_offset, 0);

        // Right pane scroll (column 60 >= 80 * 45 / 100 = 36) -> scrolls inspector
        let scroll_down_right = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 60,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        state.handle_mouse(scroll_down_right);
        assert_eq!(state.inspector_scroll_offset, 1);
        assert_eq!(state.selected_task_index, 0);

        let scroll_up_right = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 60,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        state.handle_mouse(scroll_up_right);
        assert_eq!(state.inspector_scroll_offset, 0);
        assert_eq!(state.selected_task_index, 0);

        // Left pane scroll (column 20 < 36) -> navigates tasks and resets inspector offset
        state.inspector_scroll_offset = 3;
        let scroll_down_left = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        state.handle_mouse(scroll_down_left);
        assert_eq!(state.selected_task_index, 1);
        assert_eq!(state.inspector_scroll_offset, 0);
    }

    #[test]
    fn test_phase22_selective_bulk_approval() {
        let mut state = AppState::new().with_test_data();
        state.current_screen = CurrentScreen::PlanReview;

        // Select task 0 and task 2 (out of 3 agy tasks)
        state.selected_task_indices.insert(0);
        state.selected_task_indices.insert(2);

        // Trigger prompt_approve_all_tasks()
        state.prompt_approve_all_tasks();
        assert!(state.pending_confirmation.is_some());
        let modal = state.pending_confirmation.as_ref().unwrap();
        assert_eq!(modal.title, "Confirm Bulk Task Approval");
        assert_eq!(modal.message, "Approve selected 2 task(s)?");

        // Confirm with 'y'
        let action = state.handle_key(make_key(KeyCode::Char('y')));
        assert_eq!(action, Some(TuiAction::ApproveAllTasks));
        assert!(state.pending_confirmation.is_none());
        assert!(state.selected_task_indices.is_empty()); // selection cleared!

        // Task 0 and Task 2 are approved
        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
        assert_eq!(
            state.review_tasks[0].human_decision,
            Some(ApprovalStatus::Approved)
        );
        assert_eq!(state.review_tasks[2].task.status, TaskStatus::Approved);
        assert_eq!(
            state.review_tasks[2].human_decision,
            Some(ApprovalStatus::Approved)
        );

        // Task 1 was NOT selected and remains in its initial status
        assert_eq!(state.review_tasks[1].task.status, TaskStatus::HumanReview);

        // Now with empty selection, prompts to approve remaining 1 pending task
        state.prompt_approve_all_tasks();
        assert!(state.pending_confirmation.is_some());
        let modal = state.pending_confirmation.as_ref().unwrap();
        assert!(modal.message.contains("1 pending tasks"));

        state.handle_key(make_key(KeyCode::Char('y')));
        assert_eq!(state.review_tasks[1].task.status, TaskStatus::Approved);

        // When all tasks are approved, prompt shows no pending tasks
        state.prompt_approve_all_tasks();
        assert!(state.pending_confirmation.is_none());
        assert_eq!(
            state.status_message.as_deref(),
            Some("No pending tasks to approve.")
        );
    }
}
