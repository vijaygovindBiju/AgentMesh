use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::spec::TaskSpec;
use crate::status::AgentStatus;

/// Messages published by an agent to the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    /// Agent registration request (sent to coordinator.agents.register).
    Register {
        agent_id: Uuid,
        human_owner: String,
        adapter_type: String,
        capabilities: Vec<String>,
        api_key: String,
    },
    /// Agent acknowledged task assignment and started work.
    TaskStarted {
        agent_id: Uuid,
        task_id: Uuid,
        idempotency_key: String,
        timestamp: DateTime<Utc>,
    },
    /// Periodic progress report from agent.
    ProgressUpdate {
        agent_id: Uuid,
        task_id: Uuid,
        message: String,
        percent: u8,
        timestamp: DateTime<Utc>,
    },
    /// Agent encountered a blocker and cannot proceed.
    Blocked {
        agent_id: Uuid,
        task_id: Uuid,
        reason: String,
        blocking_task_id: Option<Uuid>,
        timestamp: DateTime<Utc>,
    },
    /// Agent successfully completed the task.
    Completed {
        agent_id: Uuid,
        task_id: Uuid,
        summary: String,
        timestamp: DateTime<Utc>,
    },
    /// Agent failed to complete the task.
    Failed {
        agent_id: Uuid,
        task_id: Uuid,
        error: String,
        timestamp: DateTime<Utc>,
    },
    /// Agent periodic heartbeat.
    Heartbeat {
        agent_id: Uuid,
        status: AgentStatus,
        current_task_id: Option<Uuid>,
        timestamp: DateTime<Utc>,
    },
}

/// Messages sent from the coordinator to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CoordinatorMessage {
    /// Assignment of a task to the agent.
    TaskAssignment {
        #[serde(flatten)]
        spec: TaskSpec,
    },
    /// Explicit cancellation of an assigned task.
    TaskCancelled {
        task_id: Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    /// Instruction to pause/wait for a dependency blocker.
    WaitForDependency {
        task_id: Uuid,
        blocking_task_id: Uuid,
        message: String,
        timestamp: DateTime<Utc>,
    },
    /// Response to an agent registration request.
    RegisterResponse {
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        nats_subject: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_message_serde_round_trip() {
        let now = Utc::now();
        let agent_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        let messages = vec![
            AgentMessage::Register {
                agent_id,
                human_owner: "Alice".to_string(),
                adapter_type: "Mock".to_string(),
                capabilities: vec!["rust".to_string()],
                api_key: "secret".to_string(),
            },
            AgentMessage::TaskStarted {
                agent_id,
                task_id,
                idempotency_key: format!("{}:1", task_id),
                timestamp: now,
            },
            AgentMessage::ProgressUpdate {
                agent_id,
                task_id,
                message: "Halfway done".to_string(),
                percent: 50,
                timestamp: now,
            },
            AgentMessage::Blocked {
                agent_id,
                task_id,
                reason: "Waiting for schema".to_string(),
                blocking_task_id: Some(Uuid::new_v4()),
                timestamp: now,
            },
            AgentMessage::Completed {
                agent_id,
                task_id,
                summary: "All tests pass".to_string(),
                timestamp: now,
            },
            AgentMessage::Failed {
                agent_id,
                task_id,
                error: "Build failure".to_string(),
                timestamp: now,
            },
            AgentMessage::Heartbeat {
                agent_id,
                status: AgentStatus::Busy,
                current_task_id: Some(task_id),
                timestamp: now,
            },
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).expect("Serialize failed");
            let recovered: AgentMessage = serde_json::from_str(&json).expect("Deserialize failed");
            assert_eq!(msg, recovered);
        }
    }

    #[test]
    fn test_coordinator_message_serde_round_trip() {
        let now = Utc::now();
        let task_id = Uuid::new_v4();
        let blocker_id = Uuid::new_v4();

        let spec = TaskSpec::new(
            task_id,
            "TASK-001",
            "Setup DB",
            "Run migrations",
            vec!["migrations/".to_string()],
            vec![],
            format!("{}:1", task_id),
        );

        let messages = vec![
            CoordinatorMessage::TaskAssignment { spec },
            CoordinatorMessage::TaskCancelled {
                task_id,
                reason: "Cancelled by human".to_string(),
                timestamp: now,
            },
            CoordinatorMessage::WaitForDependency {
                task_id,
                blocking_task_id: blocker_id,
                message: "Wait for blocker".to_string(),
                timestamp: now,
            },
            CoordinatorMessage::RegisterResponse {
                status: "ok".to_string(),
                nats_subject: Some(format!("agents.{}.events", Uuid::new_v4())),
                error: None,
            },
            CoordinatorMessage::RegisterResponse {
                status: "error".to_string(),
                nats_subject: None,
                error: Some("Invalid key".to_string()),
            },
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).expect("Serialize failed");
            let recovered: CoordinatorMessage = serde_json::from_str(&json).expect("Deserialize failed");
            assert_eq!(msg, recovered);
        }
    }
}
