//! Reconnect handling and retry utilities for resilient operations.

use anyhow::{Context, Result};
use sqlx::PgPool;
use std::future::Future;
use std::time::Duration;
use tracing::{error, warn};

/// Executes an asynchronous operation with exponential backoff retry.
pub async fn with_retry<T, F, Fut>(
    max_retries: u32,
    initial_delay: Duration,
    mut op: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0;
    let mut delay = initial_delay;

    loop {
        attempt += 1;
        match op().await {
            Ok(val) => return Ok(val),
            Err(e) if attempt >= max_retries => {
                error!(attempt, max_retries, error = %e, "Operation exhausted max retries");
                return Err(e);
            }
            Err(e) => {
                warn!(attempt, max_retries, retry_in = ?delay, error = %e, "Operation failed, retrying...");
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
        }
    }
}

pub struct ResilientConnection;

impl ResilientConnection {
    /// Validates the database connection pool by acquiring a connection and executing a ping.
    pub async fn verify_db(pool: &PgPool) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .context("Failed to verify database connection")?;
        Ok(())
    }

    /// Configures NATS connection options with retry backoff and connection event logging.
    pub fn nats_options() -> async_nats::ConnectOptions {
        async_nats::ConnectOptions::new()
            .connection_timeout(Duration::from_secs(5))
            .retry_on_initial_connect()
            .event_callback(|event| async move {
                match event {
                    async_nats::Event::Connected => tracing::info!("NATS client connected"),
                    async_nats::Event::Disconnected => tracing::warn!("NATS client disconnected"),
                    async_nats::Event::SlowConsumer(id) => {
                        tracing::warn!(consumer_id = id, "NATS slow consumer detected")
                    }
                    async_nats::Event::ServerError(err) => {
                        tracing::error!(error = %err, "NATS server error")
                    }
                    async_nats::Event::ClientError(err) => {
                        tracing::error!(error = %err, "NATS client error")
                    }
                    async_nats::Event::LameDuckMode => {
                        tracing::warn!("NATS server entered lame duck mode")
                    }
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_with_retry_succeeds_on_second_attempt() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let result = with_retry(3, Duration::from_millis(10), || {
            let attempts = attempts_clone.clone();
            async move {
                let prev = attempts.fetch_add(1, Ordering::SeqCst);
                if prev == 0 {
                    anyhow::bail!("Temporary failure");
                }
                Ok("success")
            }
        })
        .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_with_retry_fails_after_max_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<()> = with_retry(3, Duration::from_millis(5), || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("Permanent failure")
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}
