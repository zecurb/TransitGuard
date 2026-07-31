use std::error::Error as StandardError;

use thiserror::Error;

/// A thread-safe infrastructure error preserved as an application source.
pub type BoxError =
    Box<dyn StandardError + Send + Sync + 'static>;

/// A sanitized repository failure.
///
/// The public display message identifies the affected operation without
/// exposing database statements, credentials, connection strings, or raw
/// infrastructure details.
#[derive(Debug, Error)]
#[error("{entity} repository operation `{operation}` failed")]
pub struct RepositoryError {
    entity: &'static str,
    operation: &'static str,

    #[source]
    source: BoxError,
}

impl RepositoryError {
    /// Creates a repository error while preserving its underlying cause.
    pub fn new<E>(
        entity: &'static str,
        operation: &'static str,
        source: E,
    ) -> Self
    where
        E: StandardError + Send + Sync + 'static,
    {
        Self {
            entity,
            operation,
            source: Box::new(source),
        }
    }

    /// Returns the affected repository entity.
    #[must_use]
    pub const fn entity(&self) -> &'static str {
        self.entity
    }

    /// Returns the failed repository operation.
    #[must_use]
    pub const fn operation(&self) -> &'static str {
        self.operation
    }
}

/// Errors returned by application use cases.
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// A required domain entity was not found.
    #[error("{entity} was not found: {identifier}")]
    NotFound {
        /// Human-readable entity category.
        entity: &'static str,

        /// Stable identifier requested by the operation.
        identifier: String,
    },

    /// Existing state conflicts with the requested operation.
    #[error(
        "application operation `{operation}` cannot continue: {reason}"
    )]
    Conflict {
        /// Stable operation name.
        operation: &'static str,

        /// Sanitized conflict reason.
        reason: &'static str,
    },

    /// An infrastructure repository operation failed.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl ApplicationError {
    /// Creates an entity-not-found application error.
    #[must_use]
    pub fn not_found(
        entity: &'static str,
        identifier: impl Into<String>,
    ) -> Self {
        Self::NotFound {
            entity,
            identifier: identifier.into(),
        }
    }

    /// Creates an application conflict error.
    #[must_use]
    pub const fn conflict(
        operation: &'static str,
        reason: &'static str,
    ) -> Self {
        Self::Conflict {
            operation,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as StandardError;

    use thiserror::Error;

    use super::{
        ApplicationError,
        RepositoryError,
    };

    #[derive(Debug, Error)]
    #[error("database connection unavailable")]
    struct StorageUnavailable;

    #[test]
    fn repository_error_has_sanitized_display_text() {
        let error = RepositoryError::new(
            "transit account",
            "find by identifier",
            StorageUnavailable,
        );

        assert_eq!(
            error.to_string(),
            "transit account repository operation \
             `find by identifier` failed"
        );
        assert_eq!(error.entity(), "transit account");
        assert_eq!(
            error.operation(),
            "find by identifier"
        );
        assert!(
            StandardError::source(&error).is_some()
        );
    }

    #[test]
    fn application_not_found_error_contains_identifier() {
        let error = ApplicationError::not_found(
            "fare credential",
            "credential-123",
        );

        assert_eq!(
            error.to_string(),
            "fare credential was not found: credential-123"
        );
    }

    #[test]
    fn repository_error_converts_to_application_error() {
        let repository_error = RepositoryError::new(
            "reader equipment",
            "save",
            StorageUnavailable,
        );

        let application_error =
            ApplicationError::from(repository_error);

        assert!(matches!(
            application_error,
            ApplicationError::Repository(_)
        ));
    }
}
