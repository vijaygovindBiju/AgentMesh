use anyhow::{Context, Result};
use async_nats::Client;
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::db::repositories::AgentRepository;
use crate::domain::{AdapterType, NewAgent};
use crate::security::audit::{AuditEvent, AuditLogger};
use crate::security::auth::ApiKeyManager;
use agent_protocol::{AgentMessage, CoordinatorMessage};

pub const REGISTRATION_SUBJECT: &str = "coordinator.agents.register";

pub struct RegistrationHandler;

impl RegistrationHandler {
    /// Processes a single registration request and returns a RegisterResponse message.
    pub async fn process_registration(
        pool: &PgPool,
        msg: AgentMessage,
    ) -> Result<CoordinatorMessage> {
        let AgentMessage::Register {
            agent_id,
            human_owner,
            adapter_type,
            capabilities,
            profile,
            api_key,
        } = msg
        else {
            return Ok(CoordinatorMessage::RegisterResponse {
                status: "error".to_string(),
                nats_subject: None,
                error: Some("Invalid message type: expected Register".to_string()),
            });
        };

        info!(%agent_id, %human_owner, %adapter_type, "Processing agent registration request");

        let adapter = match adapter_type.to_lowercase().as_str() {
            "mock" => AdapterType::Mock,
            "agy" => AdapterType::Agy,
            other => {
                warn!(adapter = %other, "Unknown adapter type in registration");
                return Ok(CoordinatorMessage::RegisterResponse {
                    status: "error".to_string(),
                    nats_subject: None,
                    error: Some(format!("Unsupported adapter type: {other}")),
                });
            }
        };

        let nats_subject = format!("agents.{agent_id}.events");

        // Check if agent already exists
        match AgentRepository::find_by_id(pool, agent_id).await? {
            Some(existing) => {
                // Check if agent is revoked
                if existing.is_revoked {
                    let _ = AuditLogger::record(
                        pool,
                        &AuditEvent::new(
                            "agent",
                            Some(agent_id.to_string()),
                            "agent_register",
                            "agent",
                            Some(agent_id.to_string()),
                            "denied",
                            serde_json::json!({ "reason": "Agent key is revoked" }),
                        ),
                    )
                    .await;
                    warn!(%agent_id, "Registration failed: agent key is revoked");
                    return Ok(CoordinatorMessage::RegisterResponse {
                        status: "error".to_string(),
                        nats_subject: None,
                        error: Some("Agent key is revoked".to_string()),
                    });
                }

                // Check if agent key is expired
                if existing.is_key_expired() {
                    let _ = AuditLogger::record(
                        pool,
                        &AuditEvent::new(
                            "agent",
                            Some(agent_id.to_string()),
                            "agent_register",
                            "agent",
                            Some(agent_id.to_string()),
                            "denied",
                            serde_json::json!({ "reason": "Agent key has expired" }),
                        ),
                    )
                    .await;
                    warn!(%agent_id, "Registration failed: agent key has expired");
                    return Ok(CoordinatorMessage::RegisterResponse {
                        status: "error".to_string(),
                        nats_subject: None,
                        error: Some("Agent key has expired".to_string()),
                    });
                }

                // Verify API key against stored hash using constant-time check
                if !ApiKeyManager::verify_key(&api_key, &existing.api_key_hash) {
                    let _ = AuditLogger::log_auth_failure(
                        pool,
                        agent_id,
                        "Invalid API key during re-registration",
                    )
                    .await;
                    warn!(%agent_id, "Registration failed: invalid API key");
                    return Ok(CoordinatorMessage::RegisterResponse {
                        status: "error".to_string(),
                        nats_subject: None,
                        error: Some("Invalid API key".to_string()),
                    });
                }

                // Update profile if supplied
                if let Some(ref prof) = profile {
                    let _ = AgentRepository::update_capability_profile(pool, agent_id, prof).await;
                }

                let _ = AuditLogger::record(
                    pool,
                    &AuditEvent::new(
                        "agent",
                        Some(agent_id.to_string()),
                        "agent_register",
                        "agent",
                        Some(agent_id.to_string()),
                        "success",
                        serde_json::json!({ "re_registration": true }),
                    ),
                )
                .await;
            }
            None => {
                // Register new agent: hash API key if not already hashed
                let api_key_hash = if api_key.starts_with("am_ak_") {
                    ApiKeyManager::hash_key(&api_key)
                } else {
                    api_key
                };

                let new_agent = NewAgent {
                    human_owner: human_owner.clone(),
                    api_key_hash,
                    adapter_type: adapter,
                    capabilities,
                    nats_subject: nats_subject.clone(),
                    profile,
                    max_concurrency: Some(1),
                    role: Some("worker".to_string()),
                    permissions: Some(agent_protocol::security::PermissionBoundary::default()),
                    api_key_expires_at: None,
                };
                AgentRepository::create_with_id(pool, agent_id, &new_agent).await?;

                let _ = AuditLogger::record(
                    pool,
                    &AuditEvent::new(
                        "agent",
                        Some(agent_id.to_string()),
                        "agent_register",
                        "agent",
                        Some(agent_id.to_string()),
                        "success",
                        serde_json::json!({ "human_owner": human_owner }),
                    ),
                )
                .await;
            }
        }

        info!(%agent_id, "Agent registration successful");
        Ok(CoordinatorMessage::RegisterResponse {
            status: "ok".to_string(),
            nats_subject: Some(nats_subject),
            error: None,
        })
    }

    /// Listens for registration requests on `coordinator.agents.register` (request-reply loop).
    pub async fn start_listener(client: Client, pool: PgPool) -> Result<()> {
        let mut subscriber = client
            .subscribe(REGISTRATION_SUBJECT.to_string())
            .await
            .with_context(|| format!("Failed to subscribe to {REGISTRATION_SUBJECT}"))?;

        use futures::StreamExt;
        while let Some(msg) = subscriber.next().await {
            let Some(reply) = msg.reply else {
                warn!("Registration message received without reply subject");
                continue;
            };

            let response = match serde_json::from_slice::<AgentMessage>(&msg.payload) {
                Ok(agent_msg) => Self::process_registration(&pool, agent_msg)
                    .await
                    .unwrap_or_else(|e| {
                        error!(error = %e, "Failed to process registration");
                        CoordinatorMessage::RegisterResponse {
                            status: "error".to_string(),
                            nats_subject: None,
                            error: Some(e.to_string()),
                        }
                    }),
                Err(e) => {
                    warn!(error = %e, "Malformed registration JSON");
                    CoordinatorMessage::RegisterResponse {
                        status: "error".to_string(),
                        nats_subject: None,
                        error: Some("Malformed JSON payload".to_string()),
                    }
                }
            };

            let reply_payload = serde_json::to_vec(&response).unwrap_or_default();
            if let Err(e) = client.publish(reply, reply_payload.into()).await {
                error!(error = %e, "Failed to send registration reply");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use uuid::Uuid;

    async fn setup_pool() -> Option<PgPool> {
        let _ = dotenvy::dotenv();
        let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string()
        });
        let pool = create_pool(&url).await.ok()?;
        run_migrations(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn test_process_registration() {
        let Some(pool) = setup_pool().await else {
            eprintln!("Skipping test: DB not reachable");
            return;
        };

        let agent_id = Uuid::new_v4();
        let reg_msg = AgentMessage::Register {
            agent_id,
            human_owner: "RegTestOwner".to_string(),
            adapter_type: "Mock".to_string(),
            capabilities: vec!["rust".to_string()],
            profile: None,
            api_key: "my_api_key_123".to_string(),
        };

        let resp = RegistrationHandler::process_registration(&pool, reg_msg)
            .await
            .expect("Registration should succeed");

        match resp {
            CoordinatorMessage::RegisterResponse {
                status,
                nats_subject,
                error,
            } => {
                assert_eq!(status, "ok");
                assert_eq!(nats_subject, Some(format!("agents.{agent_id}.events")));
                assert!(error.is_none());
            }
            _ => panic!("Unexpected response type"),
        }
    }
}
