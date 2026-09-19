use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use uuid::Uuid;

use crate::domain::{
    Agent, ApprovalStatus, OverlapWarning, Project, Task, TaskDependency, TaskStatus,
};

/// Actions emitted by the TUI to be handled by the outer application or coordinator.
/// The TUI NEVER performs side effects (DB writes, NATS publishes, LLM requests) directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiAction {
    /// Human approved a task
    ApproveTask { task_id: Uuid },
    /// Human rejected a task
    RejectTask { task_id: Uuid },
    /// Human edited task description inline
    EditTaskDescription {
        task_id: Uuid,
        new_description: String,
    },
    /// Human submitted a new project for AI planning
    SubmitProject {
        name: String,
        description: String,
    },
    /// Human acknowledged a detected resource overlap warning
    AcknowledgeOverlap { warning_id: Uuid },
    /// Human cancelled an active or assigned task
    CancelTask { task_id: Uuid },
    /// Request refresh of project / agent data from storage
    RefreshData,
    /// Human requested exit
    Quit,
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
}


/// Active top-level screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurrentScreen {
    #[default]
    ProjectInput,
    PlanReview,
    Dashboard,
    Diagnostics,
}

impl CurrentScreen {
    pub fn title(&self) -> &'static str {
        match self {
            CurrentScreen::ProjectInput => "1. Project Input",
            CurrentScreen::PlanReview => "2. Plan Review",
            CurrentScreen::Dashboard => "3. Dashboard",
            CurrentScreen::Diagnostics => "4. Diagnostics",
        }
    }
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
}

impl ReviewTaskState {
    pub fn new(task: Task) -> Self {
        Self {
            task,
            dependencies: Vec::new(),
            suggested_agent_name: None,
            overlap_warnings: Vec::new(),
            human_decision: None,
        }
    }
}

/// Central in-memory state of the TUI application
pub struct AppState {
    pub current_screen: CurrentScreen,
    pub input_mode: InputMode,

    // Project input fields
    pub project_name_input: String,
    pub project_desc_input: String,
    pub project_input_cursor: usize,
    /// 0 = Name, 1 = Description
    pub project_focus_field: usize,

    // Active project context
    pub active_project: Option<Project>,

    // Plan review state
    pub review_tasks: Vec<ReviewTaskState>,
    pub selected_task_index: usize,
    pub show_details_pane: bool,
    pub edit_buffer: String,
    pub edit_cursor: usize,

    // Dashboard / System context
    pub agents: Vec<Agent>,
    pub active_overlaps: Vec<OverlapWarning>,
    pub selected_overlap_index: usize,

    // Diagnostics / Observability
    pub metrics: Option<crate::observability::SystemMetrics>,
    pub recent_events: Vec<crate::observability::CoordinatorEvent>,
    pub selected_timeline: Option<crate::observability::TaskTimeline>,
    pub selected_event_index: usize,

    // Status bar & messaging
    pub status_message: Option<String>,
    pub should_quit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_screen: CurrentScreen::ProjectInput,
            input_mode: InputMode::Normal,

            project_name_input: String::new(),
            project_desc_input: String::new(),
            project_input_cursor: 0,
            project_focus_field: 0,

            active_project: None,

            review_tasks: Vec::new(),
            selected_task_index: 0,
            show_details_pane: true,
            edit_buffer: String::new(),
            edit_cursor: 0,

            agents: Vec::new(),
            active_overlaps: Vec::new(),
            selected_overlap_index: 0,

            metrics: None,
            recent_events: Vec::new(),
            selected_timeline: None,
            selected_event_index: 0,

            status_message: Some("Welcome to AgentMesh Coordinator. Press [Tab] to switch screens.".to_string()),
            should_quit: false,
        }
    }

    /// Helper to seed mock data for UI testing and demonstration
    pub fn with_mock_data(mut self) -> Self {
        let proj_id = Uuid::new_v4();
        let agent1_id = Uuid::new_v4();
        let agent2_id = Uuid::new_v4();

        self.active_project = Some(Project {
            id: proj_id,
            name: "Cloud Migration Mesh".to_string(),
            description: "Decompose and migrate monolith services to cloud native workers".to_string(),
            status: crate::domain::ProjectStatus::Planning,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });

        self.agents = vec![
            Agent::mock(
                agent1_id,
                "agent-backend (Alice)",
                crate::domain::AdapterType::Agy,
                vec!["backend".to_string(), "rust".to_string()],
                crate::domain::AgentStatus::Idle,
            ),
            Agent::mock(
                agent2_id,
                "agent-infra (Bob)",
                crate::domain::AdapterType::Mock,
                vec!["infra".to_string(), "nats".to_string(), "docker".to_string()],
                crate::domain::AgentStatus::Busy,
            ),
        ];

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
        self.current_screen = CurrentScreen::PlanReview;
        self
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

        match self.input_mode {
            InputMode::Normal => self.handle_normal_mode_key(key),
            InputMode::EditingDescription => self.handle_editing_key(key),
            InputMode::EnteringProject => self.handle_project_input_key(key),
        }
    }

    fn handle_normal_mode_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.should_quit = true;
                Some(TuiAction::Quit)
            }
            KeyCode::Tab => {
                self.cycle_screen();
                None
            }
            KeyCode::Char('1') => {
                self.current_screen = CurrentScreen::ProjectInput;
                None
            }
            KeyCode::Char('2') => {
                self.current_screen = CurrentScreen::PlanReview;
                None
            }
            KeyCode::Char('3') => {
                self.current_screen = CurrentScreen::Dashboard;
                None
            }
            KeyCode::Char('4') => {
                self.current_screen = CurrentScreen::Diagnostics;
                None
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.status_message = Some("Refreshed data.".to_string());
                Some(TuiAction::RefreshData)
            }
            _ => match self.current_screen {
                CurrentScreen::PlanReview => self.handle_plan_review_key(key),
                CurrentScreen::ProjectInput => {
                    if key.code == KeyCode::Enter || key.code == KeyCode::Char('i') {
                        self.input_mode = InputMode::EnteringProject;
                        self.status_message = Some("Entering project details. [Tab] switch field, [Enter] submit, [Esc] cancel.".to_string());
                    }
                    None
                }
                CurrentScreen::Dashboard => self.handle_dashboard_key(key),
                CurrentScreen::Diagnostics => self.handle_diagnostics_key(key),
            },
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
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
        }
    }

    fn handle_diagnostics_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_event_index > 0 {
                    self.selected_event_index -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.recent_events.is_empty()
                    && self.selected_event_index + 1 < self.recent_events.len()
                {
                    self.selected_event_index += 1;
                }
                None
            }
            _ => None,
        }
    }

    fn handle_plan_review_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        if self.review_tasks.is_empty() {
            return None;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_task_index > 0 {
                    self.selected_task_index -= 1;
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_task_index + 1 < self.review_tasks.len() {
                    self.selected_task_index += 1;
                }
                None
            }
            KeyCode::Enter => {
                self.show_details_pane = !self.show_details_pane;
                None
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
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
                let task_id = self.review_tasks[self.selected_task_index].task.id;
                let short_id = self.review_tasks[self.selected_task_index].task.short_id.clone();
                self.review_tasks[self.selected_task_index].human_decision = Some(ApprovalStatus::Approved);
                self.review_tasks[self.selected_task_index].task.status = TaskStatus::Approved;
                self.status_message = Some(format!("Task {short_id} marked Approved."));
                
                // Advance selection if not at end
                if self.selected_task_index + 1 < self.review_tasks.len() {
                    self.selected_task_index += 1;
                }
                Some(TuiAction::ApproveTask { task_id })
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let task_id = self.review_tasks[self.selected_task_index].task.id;
                let short_id = self.review_tasks[self.selected_task_index].task.short_id.clone();
                self.review_tasks[self.selected_task_index].human_decision = Some(ApprovalStatus::Rejected);
                self.status_message = Some(format!("Task {short_id} marked Rejected."));
                
                if self.selected_task_index + 1 < self.review_tasks.len() {
                    self.selected_task_index += 1;
                }
                Some(TuiAction::RejectTask { task_id })
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                let current_desc = self.review_tasks[self.selected_task_index].task.description.clone();
                self.edit_buffer = current_desc;
                self.edit_cursor = self.edit_buffer.len();
                self.input_mode = InputMode::EditingDescription;
                self.status_message = Some("Editing description inline. [Enter] save & approve, [Esc] cancel.".to_string());
                None
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                let task_id = self.review_tasks[self.selected_task_index].task.id;
                let short_id = self.review_tasks[self.selected_task_index].task.short_id.clone();
                self.status_message = Some(format!("Cancelling task {short_id}..."));
                Some(TuiAction::CancelTask { task_id })
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
                let task_id = self.review_tasks[self.selected_task_index].task.id;
                let short_id = self.review_tasks[self.selected_task_index].task.short_id.clone();
                let new_desc = self.edit_buffer.trim().to_string();

                self.review_tasks[self.selected_task_index].task.description = new_desc.clone();
                // Acceptance criteria: [E] edit description, Enter confirms edit + approves
                self.review_tasks[self.selected_task_index].human_decision = Some(ApprovalStatus::EditedAndApproved);
                self.review_tasks[self.selected_task_index].task.status = TaskStatus::Approved;
                self.status_message = Some(format!("Updated {short_id} description & marked Approved."));

                Some(TuiAction::EditTaskDescription {
                    task_id,
                    new_description: new_desc,
                })
            }
            KeyCode::Backspace => {
                if self.edit_cursor > 0 && !self.edit_buffer.is_empty() {
                    self.edit_cursor -= 1;
                    self.edit_buffer.remove(self.edit_cursor);
                }
                None
            }
            KeyCode::Left => {
                if self.edit_cursor > 0 {
                    self.edit_cursor -= 1;
                }
                None
            }
            KeyCode::Right => {
                if self.edit_cursor < self.edit_buffer.len() {
                    self.edit_cursor += 1;
                }
                None
            }
            KeyCode::Char(c) => {
                self.edit_buffer.insert(self.edit_cursor, c);
                self.edit_cursor += 1;
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
                self.status_message = Some(format!("Submitted project '{name}' for decomposition."));

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
            CurrentScreen::ProjectInput => CurrentScreen::PlanReview,
            CurrentScreen::PlanReview => CurrentScreen::Dashboard,
            CurrentScreen::Dashboard => CurrentScreen::Diagnostics,
            CurrentScreen::Diagnostics => CurrentScreen::ProjectInput,
        };
    }

    pub fn selected_task(&self) -> Option<&ReviewTaskState> {
        self.review_tasks.get(self.selected_task_index)
    }

    /// Updates local in-memory state based on incoming AgentMessage protocol events.
    pub fn apply_agent_message(&mut self, msg: &agent_protocol::AgentMessage) {
        match msg {
            agent_protocol::AgentMessage::TaskStarted {
                agent_id,
                task_id,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.task.status = TaskStatus::Executing;
                    rt.task.assigned_agent_id = Some(*agent_id);
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Busy;
                    agent.current_task_id = Some(*task_id);
                }
                let display_name = short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} is now EXECUTING."));
            }
            agent_protocol::AgentMessage::ProgressUpdate {
                task_id,
                percent,
                message,
                ..
            } => {
                let short_id = self
                    .review_tasks
                    .iter()
                    .find(|rt| rt.task.id == *task_id)
                    .map(|rt| rt.task.short_id.clone())
                    .unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {short_id} ({percent}%): {message}"));
            }
            agent_protocol::AgentMessage::Blocked {
                agent_id,
                task_id,
                reason,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.task.status = TaskStatus::Blocked;
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Blocked;
                }
                let display_name = short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} is BLOCKED: {reason}"));
            }
            agent_protocol::AgentMessage::Completed {
                agent_id,
                task_id,
                summary,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.task.status = TaskStatus::Completed;
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Idle;
                    agent.current_task_id = None;
                }
                let display_name = short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} COMPLETED: {summary}"));
            }
            agent_protocol::AgentMessage::Failed {
                agent_id,
                task_id,
                error,
                ..
            } => {
                let mut short_id_opt = None;
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.task.status = TaskStatus::Failed;
                    short_id_opt = Some(rt.task.short_id.clone());
                }
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = crate::domain::AgentStatus::Error;
                    agent.current_task_id = None;
                }
                let display_name = short_id_opt.unwrap_or_else(|| task_id.to_string()[..8].to_string());
                self.status_message = Some(format!("Task {display_name} FAILED: {error}"));
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
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.human_decision = Some(ApprovalStatus::Approved);
                    rt.task.status = TaskStatus::Approved;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskRejected { task_id } => {
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.human_decision = Some(ApprovalStatus::Rejected);
                    rt.task.status = TaskStatus::Rejected;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskStatusChanged {
                task_id,
                new_status,
                ..
            } => {
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
                    rt.task.status = *new_status;
                }
            }
            crate::coordinator::CoordinatorEvent::TaskAssigned { task_id, agent_id } => {
                if let Some(rt) = self.review_tasks.iter_mut().find(|rt| rt.task.id == *task_id) {
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
            TuiUpdateEvent::Tasks(tasks) => self.review_tasks = tasks,
            TuiUpdateEvent::Agents(agents) => self.agents = agents,
            TuiUpdateEvent::Overlaps(overlaps) => self.active_overlaps = overlaps,
            TuiUpdateEvent::StatusMessage(msg) => self.status_message = Some(msg),
            TuiUpdateEvent::Metrics(m) => self.metrics = Some(m),
            TuiUpdateEvent::Timeline(tl) => self.selected_timeline = Some(tl),
            TuiUpdateEvent::ObservabilityEvent(ev) => {
                self.recent_events.insert(0, ev);
                if self.recent_events.len() > 100 {
                    self.recent_events.truncate(100);
                }
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
        assert_eq!(state.current_screen, CurrentScreen::ProjectInput);
        assert_eq!(state.input_mode, InputMode::Normal);
        assert!(!state.should_quit);
    }

    #[test]
    fn test_screen_navigation_cycle() {
        let mut state = AppState::new();
        assert_eq!(state.current_screen, CurrentScreen::ProjectInput);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

        state.handle_key(make_key(KeyCode::Tab));
        assert_eq!(state.current_screen, CurrentScreen::ProjectInput);

        // Direct number keys
        state.handle_key(make_key(KeyCode::Char('2')));
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);

        state.handle_key(make_key(KeyCode::Char('3')));
        assert_eq!(state.current_screen, CurrentScreen::Dashboard);

        state.handle_key(make_key(KeyCode::Char('4')));
        assert_eq!(state.current_screen, CurrentScreen::Diagnostics);

        state.handle_key(make_key(KeyCode::Char('1')));
        assert_eq!(state.current_screen, CurrentScreen::ProjectInput);
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
        let mut state = AppState::new().with_mock_data();
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
    fn test_task_approval_emits_action_and_updates_local_status() {
        let mut state = AppState::new().with_mock_data();
        let task0_id = state.review_tasks[0].task.id;

        let action = state.handle_key(make_key(KeyCode::Char('y')));
        assert_eq!(action, Some(TuiAction::ApproveTask { task_id: task0_id }));
        assert_eq!(state.review_tasks[0].human_decision, Some(ApprovalStatus::Approved));
        assert_eq!(state.review_tasks[0].task.status, TaskStatus::Approved);
        // Automatically advances to index 1
        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_task_rejection_emits_action_and_updates_local_status() {
        let mut state = AppState::new().with_mock_data();
        let task0_id = state.review_tasks[0].task.id;

        let action = state.handle_key(make_key(KeyCode::Char('n')));
        assert_eq!(action, Some(TuiAction::RejectTask { task_id: task0_id }));
        assert_eq!(state.review_tasks[0].human_decision, Some(ApprovalStatus::Rejected));
        assert_eq!(state.selected_task_index, 1);
    }

    #[test]
    fn test_task_edit_workflow_and_approval_confirmation() {
        let mut state = AppState::new().with_mock_data();
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
        assert_eq!(state.review_tasks[0].task.description, format!("{original_desc} (Revised)"));
        assert_eq!(state.review_tasks[0].human_decision, Some(ApprovalStatus::EditedAndApproved));
    }

    #[test]
    fn test_task_edit_cancel_with_escape() {
        let mut state = AppState::new().with_mock_data();
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
    fn test_plan_review_overlap_acknowledgment() {
        let mut state = AppState::new().with_mock_data();
        assert_eq!(state.current_screen, CurrentScreen::PlanReview);
        assert_eq!(state.selected_task_index, 0);
        assert_eq!(state.review_tasks[0].overlap_warnings.len(), 1);
        assert!(!state.review_tasks[0].overlap_warnings[0].acknowledged);
        assert_eq!(state.active_overlaps.len(), 1);

        let warn_id = state.review_tasks[0].overlap_warnings[0].id;

        // Press 'a' to acknowledge overlap
        let action = state.handle_key(make_key(KeyCode::Char('a')));
        assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id: warn_id }));
        assert!(state.review_tasks[0].overlap_warnings[0].acknowledged);
        assert!(state.active_overlaps.is_empty());
    }

    #[test]
    fn test_dashboard_overlap_acknowledgment() {
        let mut state = AppState::new().with_mock_data();
        state.current_screen = CurrentScreen::Dashboard;
        assert_eq!(state.active_overlaps.len(), 1);

        let warn_id = state.active_overlaps[0].id;

        // Press 'a' in dashboard to acknowledge
        let action = state.handle_key(make_key(KeyCode::Char('a')));
        assert_eq!(action, Some(TuiAction::AcknowledgeOverlap { warning_id: warn_id }));
        assert!(state.active_overlaps.is_empty());
    }

    #[test]
    fn test_live_agent_message_projection() {
        let mut state = AppState::new().with_mock_data();
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
}

