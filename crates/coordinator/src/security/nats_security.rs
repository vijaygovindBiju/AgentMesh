//! Secure NATS Configuration, Subject Authorization, and TLS Configuration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::security::SecurityError;

/// NATS Security and TLS connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NatsSecurityConfig {
    /// Optional static authentication token
    pub auth_token: Option<String>,
    /// Optional NATS username
    pub username: Option<String>,
    /// Optional NATS password
    pub password: Option<String>,
    /// Whether TLS is strictly required for broker connections
    pub require_tls: bool,
    /// Path to custom CA root certificate file (PEM)
    pub ca_cert_path: Option<String>,
    /// Path to client certificate file (PEM) for mTLS
    pub client_cert_path: Option<String>,
    /// Path to client private key file (PEM) for mTLS
    pub client_key_path: Option<String>,
}

/// Enforces subject access control for connected agents over NATS.
pub struct NatsSubjectAuthorizer;

impl NatsSubjectAuthorizer {
    /// Validates whether an agent with ID `agent_id` is authorized to publish to `subject`.
    pub fn validate_agent_publish(agent_id: Uuid, subject: &str) -> Result<(), SecurityError> {
        let expected_events = format!("agents.{agent_id}.events");
        let expected_heartbeat = format!("agents.{agent_id}.heartbeat");

        if subject == expected_events || subject == expected_heartbeat {
            Ok(())
        } else {
            Err(SecurityError::UnauthorizedAction {
                agent_id,
                action: "nats_publish".to_string(),
                reason: format!("Agent unauthorized to publish to subject '{subject}' (must be '{expected_events}')"),
            })
        }
    }

    /// Validates whether an agent with ID `agent_id` is authorized to subscribe to `subject`.
    pub fn validate_agent_subscribe(agent_id: Uuid, subject: &str) -> Result<(), SecurityError> {
        let expected_tasks = format!("agents.{agent_id}.tasks");

        // Prevent wildcard sniffing (e.g. "agents.>")
        if subject.contains('*') || subject.contains('>') {
            return Err(SecurityError::UnauthorizedAction {
                agent_id,
                action: "nats_subscribe_wildcard".to_string(),
                reason: "Wildcard subscriptions are strictly prohibited for agent connections"
                    .to_string(),
            });
        }

        if subject == expected_tasks {
            Ok(())
        } else {
            Err(SecurityError::UnauthorizedAction {
                agent_id,
                action: "nats_subscribe".to_string(),
                reason: format!("Agent unauthorized to subscribe to subject '{subject}' (must be '{expected_tasks}')"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_subject_authorization() {
        let agent_id = Uuid::new_v4();
        let valid_publish = format!("agents.{agent_id}.events");
        assert!(NatsSubjectAuthorizer::validate_agent_publish(agent_id, &valid_publish).is_ok());

        let rogue_publish = "agents.other-agent.events";
        assert!(NatsSubjectAuthorizer::validate_agent_publish(agent_id, rogue_publish).is_err());

        let valid_sub = format!("agents.{agent_id}.tasks");
        assert!(NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, &valid_sub).is_ok());

        // Wildcard attempt
        assert!(NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, "agents.>").is_err());
        assert!(
            NatsSubjectAuthorizer::validate_agent_subscribe(agent_id, "coordinator.*").is_err()
        );
    }
}
