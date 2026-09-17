use anyhow::{Context, Result};
use async_nats::jetstream::Context as JetStreamContext;
use async_nats::{Client, ConnectOptions};

/// Establishes a connection to the NATS server and returns the client and JetStream context.
pub async fn connect(
    nats_url: &str,
    auth_token: Option<&str>,
) -> Result<(Client, JetStreamContext)> {
    let mut options = ConnectOptions::new();

    if let Some(token) = auth_token {
        if !token.is_empty() {
            options = options.token(token.to_string());
        }
    }

    let client = options
        .connect(nats_url)
        .await
        .with_context(|| format!("Failed to connect to NATS at {nats_url}"))?;

    let jetstream = async_nats::jetstream::new(client.clone());

    Ok((client, jetstream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nats_client_connection() {
        let _ = dotenvy::dotenv();
        let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
        let nats_token = std::env::var("NATS_AUTH_TOKEN").unwrap_or_else(|_| "agentmesh_dev_token".to_string());

        match connect(&nats_url, Some(&nats_token)).await {
            Ok((client, _jetstream)) => {
                assert_eq!(client.connection_state(), async_nats::connection::State::Connected);
            }
            Err(e) => {
                eprintln!("Skipping NATS test (server not reachable): {e}");
            }
        }
    }
}
