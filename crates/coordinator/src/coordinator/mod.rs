pub mod assignment;
pub mod commands;
pub mod engine;
pub mod overlap;
pub mod state;

pub use assignment::{AssignmentResult, AssignmentService};
pub use commands::{ApprovalGateError, CommandHandler, CoordinatorCommand, CoordinatorEvent};
pub use engine::CoordinatorCore;
pub use overlap::OverlapDetector;
pub use state::CoordinatorState;

