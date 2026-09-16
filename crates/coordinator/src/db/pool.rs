use std::time::Duration;
use anyhow::{Context, Result};
use sqlx::postgres::{PgPool, PgPoolOptions};

/// Creates a default PostgreSQL connection pool.
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    create_pool_with_options(database_url, 10, 5).await
}

/// Creates a PostgreSQL connection pool with explicit configuration options.
pub async fn create_pool_with_options(
    database_url: &str,
    max_connections: u32,
    timeout_secs: u64,
) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(timeout_secs))
        .connect(database_url)
        .await
        .with_context(|| format!("Failed to connect to PostgreSQL database at {database_url}"))
}

/// Runs all pending SQL migrations from the `migrations/` directory.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .context("Failed to run database migrations")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_connection_and_migrations() {
        let _ = dotenvy::dotenv();
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh".to_string());

        // Attempt connection; if Postgres is reachable, run migrations and check
        match create_pool(&db_url).await {
            Ok(pool) => {
                let migration_result = run_migrations(&pool).await;
                assert!(migration_result.is_ok(), "Migrations should apply cleanly: {:?}", migration_result.err());

                // Verify query against projects table
                let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects")
                    .fetch_one(&pool)
                    .await
                    .expect("Should query projects table");
                assert!(row.0 >= 0, "Query succeeded and returned non-negative row count");
            }
            Err(e) => {
                eprintln!("Skipping DB test (Postgres not reachable at {db_url}): {e}");
            }
        }
    }
}
