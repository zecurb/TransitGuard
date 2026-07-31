use sqlx::error::ErrorKind;
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

    /// A stored database value could not be reconstructed safely.
    #[error("PostgreSQL record contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A numeric database value exceeded the domain representation.
    #[error("PostgreSQL numeric value for `{field}` is outside the supported range")]
    NumericValueOutOfRange {
        /// Stable schema field name.
        field: &'static str,
    },

    /// An optimistic-concurrency or creation condition was not satisfied.
    #[error("PostgreSQL write condition failed for `{entity}`")]
    WriteConditionFailed {
        /// Stable entity category safe for logs.
        entity: &'static str,
    },

    /// Serialization of a persistence record failed.
    #[error("persistence serialization operation          `{operation}` failed")]
    Serialization {
        /// Stable serialization operation.
        operation: &'static str,

        /// Original serialization failure.
        #[source]
        source: serde_json::Error,
    },

    /// A database constraint rejected an otherwise valid write.
    #[error("PostgreSQL {kind} constraint rejected a write for `{entity}`")]
    ConstraintViolation {
        /// Stable entity category safe for logs.
        entity: &'static str,

        /// Sanitized constraint category.
        kind: &'static str,

        /// Original SQLx error retained as the source.
        #[source]
        source: sqlx::Error,
    },

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
    pub(crate) fn serialization(operation: &'static str, source: serde_json::Error) -> Self {
        Self::Serialization { operation, source }
    }

    pub(crate) fn write(
        operation: &'static str,
        entity: &'static str,
        source: sqlx::Error,
    ) -> Self {
        let error_kind = match &source {
            sqlx::Error::Database(database_error) => database_error.kind(),

            _ => {
                return Self::database(operation, source);
            }
        };

        match error_kind {
            ErrorKind::UniqueViolation => Self::WriteConditionFailed { entity },

            ErrorKind::ForeignKeyViolation => Self::ConstraintViolation {
                entity,
                kind: "foreign-key",
                source,
            },

            ErrorKind::NotNullViolation => Self::ConstraintViolation {
                entity,
                kind: "not-null",
                source,
            },

            ErrorKind::CheckViolation => Self::ConstraintViolation {
                entity,
                kind: "check",
                source,
            },

            ErrorKind::ExclusionViolation => Self::ConstraintViolation {
                entity,
                kind: "exclusion",
                source,
            },

            _ => Self::database(operation, source),
        }
    }

    pub(crate) const fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    pub(crate) const fn migration(source: sqlx::migrate::MigrateError) -> Self {
        Self::Migration { source }
    }
}
