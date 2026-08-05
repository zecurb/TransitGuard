use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_domain::{ReaderId, SynchronizationBatchId};

use crate::{ReaderSynchronizationError, SynchronizationBatch, load_synchronization_batch};

/// Failures produced by final synchronization-batch transitions.
#[derive(Debug, Error)]
pub enum ReaderSynchronizationFailureError {
    /// Operation times cannot predate the Unix epoch.
    #[error(
        "reader synchronization failure time cannot be negative: \
         {unix_milliseconds}"
    )]
    NegativeOperationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// Failure categories must contain a stable value.
    #[error("reader synchronization failure category must not be empty")]
    EmptyFailureCategory,

    /// The batch was not awaiting a backend result.
    #[error(
        "synchronization batch {batch_id} does not permit transition \
         `{transition}`"
    )]
    InvalidTransition {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Stable transition category.
        transition: &'static str,
    },

    /// Not every associated queue entry could transition atomically.
    #[error(
        "synchronization batch {batch_id} expected {expected} queue \
         entries but updated {updated}"
    )]
    BatchEntryConflict {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Number of durable batch entries.
        expected: u64,

        /// Number of queue entries transitioned.
        updated: u64,
    },

    /// SQLite contained invalid lifecycle data.
    #[error(
        "reader synchronization failure contains an invalid value for \
         `{field}`"
    )]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// Loading the resulting durable batch failed.
    #[error(transparent)]
    Synchronization(#[from] ReaderSynchronizationError),

    /// A named SQLite final-failure operation failed.
    #[error(
        "reader SQLite synchronization-failure operation \
         `{operation}` failed"
    )]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderSynchronizationFailureError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_transition(
        batch_id: SynchronizationBatchId,
        transition: &'static str,
    ) -> Self {
        Self::InvalidTransition {
            batch_id,
            transition,
        }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

/// Records a final non-retryable failure for an in-flight batch.
///
/// The batch and every associated queue entry transition inside one
/// SQLite transaction.
pub async fn record_synchronization_permanent_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
) -> Result<SynchronizationBatch, ReaderSynchronizationFailureError> {
    record_final_synchronization_failure(
        pool,
        reader_id,
        batch_id,
        failure_category,
        updated_at_unix_milliseconds,
        "permanent_failure",
        "record_permanent_failure",
    )
    .await
}

/// Moves an unresolved in-flight batch and all of its entries into
/// durable manual review.
pub async fn record_synchronization_manual_review(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
) -> Result<SynchronizationBatch, ReaderSynchronizationFailureError> {
    record_final_synchronization_failure(
        pool,
        reader_id,
        batch_id,
        failure_category,
        updated_at_unix_milliseconds,
        "manual_review",
        "record_manual_review",
    )
    .await
}

async fn record_final_synchronization_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
    target_state: &'static str,
    transition: &'static str,
) -> Result<SynchronizationBatch, ReaderSynchronizationFailureError> {
    validate_operation_time(updated_at_unix_milliseconds)?;

    let failure_category = validate_failure_category(failure_category)?;

    let mut transaction = pool.begin().await.map_err(|source| {
        ReaderSynchronizationFailureError::database("begin final batch failure", source)
    })?;

    let batch_update = sqlx::query(
        r#"
        UPDATE synchronization_batches
        SET
            batch_state = ?,
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            batch_id = ?
            AND reader_id = ?
            AND batch_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(target_state)
    .bind(&failure_category)
    .bind(updated_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .bind(updated_at_unix_milliseconds)
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationFailureError::database("record final batch failure", source)
    })?;

    if batch_update.rows_affected() != 1 {
        return Err(ReaderSynchronizationFailureError::invalid_transition(
            batch_id, transition,
        ));
    }

    let expected_entries = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM synchronization_entries
        WHERE
            batch_id = ?
            AND reader_id = ?
        "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationFailureError::database("count final batch entries", source)
    })?;

    let expected_entries = u64::try_from(expected_entries).map_err(|_| {
        ReaderSynchronizationFailureError::invalid_stored_value("batch_entry_count")
    })?;

    if expected_entries == 0 {
        return Err(ReaderSynchronizationFailureError::invalid_stored_value(
            "batch_entry_count",
        ));
    }

    let queue_update = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            queue_state = ?,
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            reader_id = ?
            AND queue_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
            AND fare_transaction_id IN (
                SELECT entry.fare_transaction_id
                FROM synchronization_entries AS entry
                WHERE
                    entry.batch_id = ?
                    AND entry.reader_id = ?
            )
        "#,
    )
    .bind(target_state)
    .bind(&failure_category)
    .bind(updated_at_unix_milliseconds)
    .bind(reader_id.to_string())
    .bind(updated_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationFailureError::database("record final queue failures", source)
    })?;

    if queue_update.rows_affected() != expected_entries {
        return Err(ReaderSynchronizationFailureError::BatchEntryConflict {
            batch_id,
            expected: expected_entries,
            updated: queue_update.rows_affected(),
        });
    }

    transaction.commit().await.map_err(|source| {
        ReaderSynchronizationFailureError::database("commit final batch failure", source)
    })?;

    load_synchronization_batch(pool, reader_id, batch_id)
        .await
        .map_err(Into::into)
}

fn validate_operation_time(
    unix_milliseconds: i64,
) -> Result<(), ReaderSynchronizationFailureError> {
    if unix_milliseconds < 0 {
        return Err(ReaderSynchronizationFailureError::NegativeOperationTime { unix_milliseconds });
    }

    Ok(())
}

fn validate_failure_category(
    failure_category: &str,
) -> Result<String, ReaderSynchronizationFailureError> {
    let failure_category = failure_category.trim();

    if failure_category.is_empty() {
        return Err(ReaderSynchronizationFailureError::EmptyFailureCategory);
    }

    Ok(failure_category.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
    };

    use serde_json::json;
    use sqlx::SqlitePool;
    use transitguard_device_protocol::DeviceProtocolVersion;
    use transitguard_domain::{
        Currency, EventTime, FareApprovalReason, FareCredentialId, FareDecision, FarePolicyVersion,
        FareTransactionId, Money, ReaderId,
    };
    use uuid::Uuid;

    use crate::{
        OfflineQueueState, OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig,
        SynchronizationBatchState, bind_reader_database, connect_reader_sqlite,
        create_synchronization_batch, enqueue_offline_transaction, load_offline_queue,
        load_ready_synchronization_batches, load_synchronization_batch,
        mark_synchronization_batch_in_flight, run_reader_sqlite_migrations,
    };

    use super::{
        ReaderSynchronizationFailureError, record_synchronization_manual_review,
        record_synchronization_permanent_failure,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    #[derive(Clone, Copy)]
    enum FinalTarget {
        PermanentFailure,
        ManualReview,
    }

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-sync-final-{name}-{}.sqlite3",
            Uuid::now_v7()
        ))
    }

    fn related_path(path: &Path, suffix: &str) -> PathBuf {
        let mut value = OsString::from(path.as_os_str());
        value.push(suffix);
        PathBuf::from(value)
    }

    fn remove_database(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(related_path(path, "-wal"));
        let _ = std::fs::remove_file(related_path(path, "-shm"));
    }

    fn event_time() -> EventTime {
        match EventTime::from_unix_milliseconds(TEST_TIME) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid event time failed: {error}")
            }
        }
    }

    fn policy_version() -> FarePolicyVersion {
        match FarePolicyVersion::new(1) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid policy version failed: {error}")
            }
        }
    }

    fn decision() -> FareDecision {
        match FareDecision::approved(
            Money::from_minor_units(250, Currency::Usd),
            FareApprovalReason::OfflineProvisional,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid decision failed: {error}")
            }
        }
    }

    fn draft() -> OfflineTransactionDraft {
        match OfflineTransactionDraft::new(
            FareTransactionId::generate(),
            FareCredentialId::generate(),
            event_time(),
            policy_version(),
            decision(),
            json!({
                "schema_version": 1,
                "kind": "offline_fare_transaction"
            }),
            TEST_TIME + 100,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid draft failed: {error}")
            }
        }
    }

    fn identity(reader_id: ReaderId) -> ReaderDatabaseIdentity {
        match ReaderDatabaseIdentity::new(
            reader_id,
            "development",
            "0.1.0",
            DeviceProtocolVersion::CURRENT,
            TEST_TIME,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid identity failed: {error}")
            }
        }
    }

    async fn open_database(name: &str, reader_id: ReaderId) -> (PathBuf, SqlitePool) {
        let path = database_path(name);

        let config = match ReaderSqliteConfig::new(path.clone()) {
            Ok(value) => value,
            Err(error) => {
                panic!("configuration failed: {error}")
            }
        };

        let pool = match connect_reader_sqlite(&config).await {
            Ok(value) => value,
            Err(error) => {
                remove_database(&path);
                panic!("connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&pool).await {
            pool.close().await;
            remove_database(&path);
            panic!("migration failed: {error}");
        }

        if let Err(error) = bind_reader_database(&pool, &identity(reader_id)).await {
            pool.close().await;
            remove_database(&path);
            panic!("identity binding failed: {error}");
        }

        (path, pool)
    }

    async fn prepare_submitted_batch(
        pool: &SqlitePool,
        reader_id: ReaderId,
    ) -> crate::SynchronizationBatch {
        for _ in 0..2 {
            if let Err(error) = enqueue_offline_transaction(pool, reader_id, &draft()).await {
                panic!("queue insertion failed: {error}");
            }
        }

        let batch = match create_synchronization_batch(
            pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            2,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                panic!("batch creation failed: {error}")
            }
        };

        match mark_synchronization_batch_in_flight(
            pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 300,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                panic!("batch submission failed: {error}")
            }
        }
    }

    async fn assert_final_failure(target: FinalTarget, test_name: &str, failure_category: &str) {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database(test_name, reader_id).await;

        let submitted = prepare_submitted_batch(&pool, reader_id).await;

        let result = match target {
            FinalTarget::PermanentFailure => {
                record_synchronization_permanent_failure(
                    &pool,
                    reader_id,
                    submitted.batch_id(),
                    failure_category,
                    TEST_TIME + 400,
                )
                .await
            }

            FinalTarget::ManualReview => {
                record_synchronization_manual_review(
                    &pool,
                    reader_id,
                    submitted.batch_id(),
                    failure_category,
                    TEST_TIME + 400,
                )
                .await
            }
        };

        let completed = match result {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("final transition failed: {error}")
            }
        };

        let (batch_state, queue_state) = match target {
            FinalTarget::PermanentFailure => (
                SynchronizationBatchState::PermanentFailure,
                OfflineQueueState::PermanentFailure,
            ),

            FinalTarget::ManualReview => (
                SynchronizationBatchState::ManualReview,
                OfflineQueueState::ManualReview,
            ),
        };

        assert_eq!(completed.state(), batch_state);
        assert_eq!(completed.last_failure_category(), Some(failure_category));
        assert_eq!(completed.next_retry_at_unix_milliseconds(), None);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue.len(), 2);

        for transaction in queue {
            assert_eq!(transaction.queue_state(), queue_state);
            assert_eq!(transaction.last_failure_category(), Some(failure_category));
            assert_eq!(transaction.next_retry_at_unix_milliseconds(), None);
        }

        let ready =
            match load_ready_synchronization_batches(&pool, reader_id, TEST_TIME + 1_000, 10).await
            {
                Ok(value) => value,
                Err(error) => {
                    pool.close().await;
                    remove_database(&path);
                    panic!("ready-batch load failed: {error}")
                }
            };

        assert!(ready.is_empty());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn final_batch_failures_update_entries_atomically() {
        assert_final_failure(
            FinalTarget::PermanentFailure,
            "permanent",
            "unsupported_protocol",
        )
        .await;

        assert_final_failure(
            FinalTarget::ManualReview,
            "manual-review",
            "manual_review_required",
        )
        .await;
    }

    #[tokio::test]
    async fn final_batch_failure_rolls_back_on_entry_conflict() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("rollback", reader_id).await;

        let submitted = prepare_submitted_batch(&pool, reader_id).await;

        let conflicting_transaction = submitted.entries()[0].transaction_id();

        let update = sqlx::query(
            r#"
            UPDATE offline_transactions
            SET queue_state = 'retryable_failure'
            WHERE
                fare_transaction_id = ?
                AND reader_id = ?
            "#,
        )
        .bind(conflicting_transaction.to_string())
        .bind(reader_id.to_string())
        .execute(&pool)
        .await;

        if let Err(error) = update {
            pool.close().await;
            remove_database(&path);
            panic!("conflict setup failed: {error}");
        }

        let result = record_synchronization_permanent_failure(
            &pool,
            reader_id,
            submitted.batch_id(),
            "backend_validation_failure",
            TEST_TIME + 400,
        )
        .await;

        assert!(matches!(
            result,
            Err(ReaderSynchronizationFailureError::BatchEntryConflict {
                expected: 2,
                updated: 1,
                ..
            })
        ));

        let batch = match load_synchronization_batch(&pool, reader_id, submitted.batch_id()).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("batch reload failed: {error}")
            }
        };

        assert_eq!(batch.state(), SynchronizationBatchState::InFlight);
        assert_eq!(batch.last_failure_category(), None);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue reload failed: {error}")
            }
        };

        assert_eq!(
            queue
                .iter()
                .filter(|entry| { entry.queue_state() == OfflineQueueState::InFlight })
                .count(),
            1
        );

        assert_eq!(
            queue
                .iter()
                .filter(|entry| { entry.queue_state() == OfflineQueueState::RetryableFailure })
                .count(),
            1
        );

        pool.close().await;
        remove_database(&path);
    }
}
