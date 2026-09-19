//! Observability and diagnostics subsystem for AgentMesh.
//!
//! Provides structured logging correlation, task execution timelines, agent activity tracking,
//! NATS delivery visibility, failure diagnostics, coordinator event streaming, and system metrics.

pub mod delivery_visibility;
pub mod diagnostics;
pub mod events;
pub mod logging;
pub mod metrics;
pub mod timeline;

pub use delivery_visibility::{DeliveryAttemptInfo, DeliveryDiagnostics};
pub use diagnostics::{FailureDiagnostics, RemediationAdvice};
pub use events::{CoordinatorEvent, CoordinatorEventRepository};
pub use logging::TraceContext;
pub use metrics::{MetricsCollector, SystemMetrics};
pub use timeline::{AgentActivityItem, AgentTimeline, TaskTimeline, TaskTimelineItem, TimelineService};
