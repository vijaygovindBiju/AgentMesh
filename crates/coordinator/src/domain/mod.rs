//! AgentMesh coordinator domain model.
//!
//! This module contains all core domain types.
//!
//! ## Design rules
//!
//! - All types derive `Serialize + Deserialize` for serde compatibility.
//! - All struct types derive `sqlx::FromRow` for database mapping.
//! - All enum types derive `sqlx::Type` with a matching PostgreSQL enum type.
//! - `New*` types are used for creation; they omit id, timestamps, and DB defaults.
//! - Business logic (state transitions, guards) lives on domain types, not in repositories.

pub mod agent;
pub mod approval;
pub mod delivery;
pub mod event;
pub mod overlap;
pub mod project;
pub mod proposal;
pub mod task;

// ─── Convenience re-exports ───────────────────────────────────────────────────
// Callers can `use crate::domain::*` or import specific items.

pub use agent::{AdapterType, Agent, AgentStatus, NewAgent};
pub use approval::{ApprovalStatus, NewTaskApproval, TaskApproval};
pub use delivery::{AckKind, DeliveryStatus, NewTaskDelivery, TaskDelivery};
pub use event::{AgentEvent, AgentEventType, NewAgentEvent};
pub use overlap::{NewOverlapWarning, OverlapSeverity, OverlapWarning};
pub use project::{NewProject, Project, ProjectStatus};
pub use proposal::{NewProposal, Proposal, ProposalStatus};
pub use task::{DependencyKind, NewTask, NewTaskDependency, Task, TaskDependency, TaskStatus};
