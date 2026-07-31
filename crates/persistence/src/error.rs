use thiserror::Error;

/// Stable failures exposed by the persistence boundary.
///
/// Error messages intentionally exclude SQL parameters, database URLs,
/// credentials, and other sensitive connection details.
#[derive(Debug, Error)]
pub enum PersistenceError {
    /// No usable PostgreSQL URL was supplied.
    #[error("PostgreSQL database URL must not be empty")]
    EmptyDatabaseUrl,

    /// A pool cannot have zero maximum connections.
    #[error(
        "PostgreSQL maximum connection count must \
         be greater than zero"
    )]
    ZeroMaxConnections,

    /// The configured pool bounds are inconsistent.
    #[error(
        "PostgreSQL minimum connection count \
         {minimum} exceeds maximum {maximum}"
    )]
    InvalidPoolBounds {
        /// Requested minimum pool size.
        minimum: u32,

        /// Requested maximum pool size.
        maximum: u32,
    },

    /// Connection acquisition must have a bounded positive timeout.
    #[error(
        "PostgreSQL acquire timeout must be \
         greater than zero"
    )]
    ZeroAcquireTimeout,

    /// SQLx failed during a named database operation.
    #[error("PostgreSQL operation `{operation}` failed")]
    Database {
        /// Stable operation category safe for logs.
        operation: &'static str,

        /// Original SQLx error retained as the source.
        #[source]
        source: sqlx::Error,
    },

    /// A versioned PostgreSQL migration failed.
    #[error("PostgreSQL migration failed")]
    Migration {
        /// Original migration failure.
        #[source]
        source: sqlx::migrate::MigrateError,
    },
}

impl PersistenceError {
    pub(crate) const fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    pub(crate) const fn migration(source: sqlx::migrate::MigrateError) -> Self {
        Self::Migration { source }
    }
}
