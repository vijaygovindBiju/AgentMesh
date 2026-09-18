//! Git repository coordination, agent workspaces, branch lifecycle,
//! resource modification tracking, and cross-agent conflict prevention.

pub mod branch;
pub mod changes;
pub mod completion;
pub mod conflict;
pub mod coordinator;
pub mod identity;
pub mod workspace;

pub use branch::BranchStrategy;
pub use changes::{ResourceTracker, UnexpectedChange};
pub use completion::{CompletionGitResult, CompletionManager};
pub use conflict::{ConcurrencySafety, ConflictDetector, GitConflictReport};
pub use coordinator::GitCoordinator;
pub use identity::RepositoryIdentity;
pub use workspace::AgentWorkspace;
