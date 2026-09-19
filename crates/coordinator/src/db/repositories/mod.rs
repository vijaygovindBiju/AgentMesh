//! Entity repositories for PostgreSQL operations.

pub mod agents;
pub mod audit;
pub mod deliveries;
pub mod events;
pub mod git_conflicts;
pub mod overlaps;
pub mod projects;
pub mod proposals;
pub mod tasks;
pub mod unexpected_resources;

pub use agents::AgentRepository;
pub use audit::AuditRepository;
pub use deliveries::TaskDeliveryRepository;
pub use events::AgentEventRepository;
pub use git_conflicts::{GitConflictRecord, GitConflictRepository};
pub use overlaps::OverlapWarningRepository;
pub use projects::ProjectRepository;
pub use proposals::ProposalRepository;
pub use tasks::{TaskGitContext, TaskRepository};
pub use unexpected_resources::{UnexpectedResourceRecord, UnexpectedResourceRepository};
