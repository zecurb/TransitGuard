use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};

use crate::{PersistenceError, PostgresConfig};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Connects to PostgreSQL using validated pool settings.
///
/// The returned pool has already established a database connection, which
/// causes invalid credentials and unreachable databases to fail during
/// startup instead of during the first business request.
pub async fn connect_postgres(config: &PostgresConfig) -> Result<PgPool, PersistenceError> {
    PgPoolOptions::new()
        .min_connections(config.min_connections())
        .max_connections(config.max_connections())
        .acquire_timeout(config.acquire_timeout())
        .connect(config.database_url())
        .await
        .map_err(|source| PersistenceError::database("connect", source))
}

/// Applies all embedded PostgreSQL migrations that have not yet run.
pub async fn run_postgres_migrations(pool: &PgPool) -> Result<(), PersistenceError> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(PersistenceError::migration)
}
