use crate::{ApplicationError, ApplicationTransaction};

/// Rolls back a transaction and returns the original application error.
///
/// A rollback failure replaces the original error because the application can
/// no longer guarantee that provisional state was discarded successfully.
pub(super) async fn rollback_with<T>(
    transaction: Box<dyn ApplicationTransaction>,
    original_error: ApplicationError,
) -> Result<T, ApplicationError> {
    match transaction.rollback().await {
        Ok(()) => Err(original_error),

        Err(rollback_error) => Err(ApplicationError::from(rollback_error)),
    }
}
