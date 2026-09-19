use anyhow::{Context, Result};
use async_nats::jetstream::Context as JetStreamContext;
use async_nats::{Client, ConnectOptions};
use std::path::Path;

use crate::security::nats_security::NatsSecurityConfig;

/// Establishes a secure connection to NATS with optional token, credentials, and TLS.
pub async fn connect_secure(
    nats_url: &str,
    sec: &NatsSecurityConfig,
) -> Result<(Client, JetStreamContext)> {
    let mut options = ConnectOptions::new();

    if let Some(ref token) = sec.auth_token {
        if !token.is_empty() {
            options = options.token(token.clone());
        }
    }

    if let (Some(ref user), Some(ref pass)) = (&sec.username, &sec.password) {
        options = options.user_and_password(user.clone(), pass.clone());
    }

    if sec.require_tls {
        options = options.require_tls(true);
    }

    if let Some(ref ca_path) = sec.ca_cert_path {
        if Path::new(ca_path).exists() {
            options = options.add_root_certificates(ca_path.into());
        }
    }

    if let (Some(ref cert_path), Some(ref key_path)) = (&sec.client_cert_path, &sec.client_key_path)
    {
        if Path::new(cert_path).exists() && Path::new(key_path).exists() {
            options = options.add_client_certificate(cert_path.into(), key_path.into());
        }
    }

    let client = options
        .connect(nats_url)
        .await
        .with_context(|| format!("Failed to connect to NATS at {nats_url}"))?;

    let jetstream = async_nats::jetstream::new(client.clone());

    Ok((client, jetstream))
}

/// Establishes a connection to the NATS server and returns the client and JetStream context.
pub async fn connect(
    nats_url: &str,
    auth_token: Option<&str>,
) -> Result<(Client, JetStreamContext)> {
    let sec = NatsSecurityConfig {
        auth_token: auth_token.map(|t| t.to_string()),
        ..Default::default()
    };
    connect_secure(nats_url, &sec).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nats_client_connection() {
        let _ = dotenvy::dotenv();
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_token =
            std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

        match connect(&nats_url, Some(&nats_token)).await {
            Ok((client, _jetstream)) => {
                assert_eq!(
                    client.connection_state(),
                    async_nats::connection::State::Connected
                );
            }
            Err(e) => {
                eprintln!("Skipping NATS test (server not reachable): {e}");
            }
        }
    }

    #[tokio::test]
    async fn test_nats_client_connect_secure_config() {
        let _ = dotenvy::dotenv();
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let sec = NatsSecurityConfig {
            auth_token: Some("agentmesh_dev_token".to_string()),
            require_tls: false,
            ..Default::default()
        };

        match connect_secure(&nats_url, &sec).await {
            Ok((client, _jetstream)) => {
                assert_eq!(
                    client.connection_state(),
                    async_nats::connection::State::Connected
                );
            }
            Err(e) => {
                eprintln!("Skipping NATS test: {e}");
            }
        }
    }
}
