use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_domain::{ReaderId, SynchronizationBatchId};

use crate::{ReaderSynchronizationError, SynchronizationBatch, load_synchronization_batch};

const RESTART_FAILURE_CATEGORY: &str = "reader_restart";

/// Failures produced by synchronization-batch lifecycle operations.
#[derive(Debug, Error)]
pub enum ReaderSynchronizationStateError {
    /// Operation times cannot predate the Unix epoch.
    #[error("reader synchronization operation time cannot be negative: {unix_milliseconds}")]
    NegativeOperationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// Failure categories must contain a stable value.
    #[error("reader synchronization failure category must not be empty")]
    EmptyFailureCategory,

    /// Retry scheduling cannot move backward.
    #[error(
        "synchronization retry time {retry_at_unix_milliseconds} cannot precede update time {updated_at_unix_milliseconds}"
    )]
    RetryBeforeUpdate {
        /// Time when retry becomes eligible.
        retry_at_unix_milliseconds: i64,

        /// Time when the failure was recorded.
        updated_at_unix_milliseconds: i64,
    },

    /// Ready-batch reads require a positive limit.
    #[error("reader synchronization read limit must be greater than zero")]
    ZeroReadLimit,

    /// The requested read limit cannot be represented by SQLite.
    #[error("reader synchronization read limit {limit} is too large")]
    ReadLimitTooLarge {
        /// Unsupported requested limit.
        limit: usize,
    },

    /// The current durable state does not permit the transition.
    #[error("synchronization batch {batch_id} does not permit transition `{transition}`")]
    InvalidTransition {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Stable transition category safe for logs.
        transition: &'static str,
    },

    /// Not every batch entry could be updated atomically.
    #[error(
        "synchronization batch {batch_id} expected {expected} queue entries but updated {updated}"
    )]
    BatchEntryConflict {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Expected number of entries.
        expected: u64,

        /// Number of entries actually updated.
        updated: u64,
    },

    /// SQLite contained invalid lifecycle metadata.
    #[error("reader synchronization lifecycle contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// Loading or decoding a synchronization batch failed.
    #[error(transparent)]
    Synchronization(#[from] ReaderSynchronizationError),

    /// A named SQLite lifecycle operation failed.
    #[error("reader SQLite synchronization-state operation `{operation}` failed")]
    Database {
        /// Stable operation category safe for logs.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderSynchronizationStateError {
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

/// Marks a prepared or eligible retryable batch as submitted.
///
/// The stable batch ID and entry identities are preserved. Batch and
/// transaction attempt counters advance inside one SQLite transaction.
pub async fn mark_synchronization_batch_in_flight(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    attempted_at_unix_milliseconds: i64,
) -> Result<SynchronizationBatch, ReaderSynchronizationStateError> {
    validate_operation_time(attempted_at_unix_milliseconds)?;

    let mut transaction = pool.begin().await.map_err(|source| {
        ReaderSynchronizationStateError::database("begin batch submission", source)
    })?;

    let batch_update = sqlx::query(
        r#"
        UPDATE synchronization_batches
        SET
            batch_state = 'in_flight',
            attempt_count = attempt_count + 1,
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = NULL,
            updated_at_unix_milliseconds = ?
        WHERE
            batch_id = ?
            AND reader_id = ?
            AND updated_at_unix_milliseconds <= ?
            AND (
                batch_state = 'prepared'
                OR (
                    batch_state = 'retryable_failure'
                    AND (
                        next_retry_at_unix_milliseconds
                            IS NULL
                        OR next_retry_at_unix_milliseconds
                            <= ?
                    )
                )
            )
        "#,
    )
    .bind(attempted_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .bind(attempted_at_unix_milliseconds)
    .bind(attempted_at_unix_milliseconds)
    .execute(&mut *transaction)
    .await
    .map_err(|source| ReaderSynchronizationStateError::database("mark batch in flight", source))?;

    if batch_update.rows_affected() != 1 {
        return Err(ReaderSynchronizationStateError::invalid_transition(
            batch_id,
            "mark_in_flight",
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
    .map_err(|source| ReaderSynchronizationStateError::database("count batch entries", source))?;

    let expected_entries = u64::try_from(expected_entries)
        .map_err(|_| ReaderSynchronizationStateError::invalid_stored_value("batch_entry_count"))?;

    if expected_entries == 0 {
        return Err(ReaderSynchronizationStateError::invalid_stored_value(
            "batch_entry_count",
        ));
    }

    let queue_update = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            attempt_count = attempt_count + 1,
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
    .bind(attempted_at_unix_milliseconds)
    .bind(reader_id.to_string())
    .bind(attempted_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationStateError::database("increment batch entry attempts", source)
    })?;

    if queue_update.rows_affected() != expected_entries {
        return Err(ReaderSynchronizationStateError::BatchEntryConflict {
            batch_id,
            expected: expected_entries,
            updated: queue_update.rows_affected(),
        });
    }

    transaction.commit().await.map_err(|source| {
        ReaderSynchronizationStateError::database("commit batch submission", source)
    })?;

    load_synchronization_batch(pool, reader_id, batch_id)
        .await
        .map_err(Into::into)
}

/// Records a transport or temporary backend failure.
///
/// The batch remains associated with the same entries and becomes
/// eligible for resubmission at the configured retry time.
pub async fn record_synchronization_retryable_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
    retry_at_unix_milliseconds: i64,
) -> Result<SynchronizationBatch, ReaderSynchronizationStateError> {
    validate_operation_time(updated_at_unix_milliseconds)?;

    validate_operation_time(retry_at_unix_milliseconds)?;

    if retry_at_unix_milliseconds < updated_at_unix_milliseconds {
        return Err(ReaderSynchronizationStateError::RetryBeforeUpdate {
            retry_at_unix_milliseconds,
            updated_at_unix_milliseconds,
        });
    }

    let failure_category = validate_failure_category(failure_category)?;

    let result = sqlx::query(
        r#"
        UPDATE synchronization_batches
        SET
            batch_state = 'retryable_failure',
            next_retry_at_unix_milliseconds = ?,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            batch_id = ?
            AND reader_id = ?
            AND batch_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(retry_at_unix_milliseconds)
    .bind(failure_category)
    .bind(updated_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .bind(updated_at_unix_milliseconds)
    .execute(pool)
    .await
    .map_err(|source| {
        ReaderSynchronizationStateError::database("record retryable batch failure", source)
    })?;

    if result.rows_affected() != 1 {
        return Err(ReaderSynchronizationStateError::invalid_transition(
            batch_id,
            "record_retryable_failure",
        ));
    }

    load_synchronization_batch(pool, reader_id, batch_id)
        .await
        .map_err(Into::into)
}

/// Recovers batches that were awaiting acknowledgement when the reader
/// stopped.
///
/// Recovery preserves batch IDs, transaction IDs, entry ordering, and
/// attempt counters.
pub async fn recover_interrupted_synchronization_batches(
    pool: &SqlitePool,
    reader_id: ReaderId,
    recovered_at_unix_milliseconds: i64,
) -> Result<u64, ReaderSynchronizationStateError> {
    validate_operation_time(recovered_at_unix_milliseconds)?;

    let result = sqlx::query(
        r#"
        UPDATE synchronization_batches
        SET
            batch_state = 'retryable_failure',
            next_retry_at_unix_milliseconds = ?,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            reader_id = ?
            AND batch_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(recovered_at_unix_milliseconds)
    .bind(RESTART_FAILURE_CATEGORY)
    .bind(recovered_at_unix_milliseconds)
    .bind(reader_id.to_string())
    .bind(recovered_at_unix_milliseconds)
    .execute(pool)
    .await
    .map_err(|source| {
        ReaderSynchronizationStateError::database("recover interrupted batches", source)
    })?;

    Ok(result.rows_affected())
}

/// Loads prepared and retry-eligible batches in sequence order.
pub async fn load_ready_synchronization_batches(
    pool: &SqlitePool,
    reader_id: ReaderId,
    now_unix_milliseconds: i64,
    limit: usize,
) -> Result<Vec<SynchronizationBatch>, ReaderSynchronizationStateError> {
    validate_operation_time(now_unix_milliseconds)?;

    if limit == 0 {
        return Err(ReaderSynchronizationStateError::ZeroReadLimit);
    }

    let sqlite_limit = i64::try_from(limit)
        .map_err(|_| ReaderSynchronizationStateError::ReadLimitTooLarge { limit })?;

    let stored_ids = sqlx::query_scalar::<_, String>(
        r#"
            SELECT batch_id
            FROM synchronization_batches
            WHERE
                reader_id = ?
                AND (
                    batch_state = 'prepared'
                    OR (
                        batch_state =
                            'retryable_failure'
                        AND (
                            next_retry_at_unix_milliseconds
                                IS NULL
                            OR next_retry_at_unix_milliseconds
                                <= ?
                        )
                    )
                )
            ORDER BY
                first_local_sequence_number,
                created_at_unix_milliseconds,
                batch_id
            LIMIT ?
            "#,
    )
    .bind(reader_id.to_string())
    .bind(now_unix_milliseconds)
    .bind(sqlite_limit)
    .fetch_all(pool)
    .await
    .map_err(|source| {
        ReaderSynchronizationStateError::database("load ready batch identities", source)
    })?;

    let mut batches = Vec::with_capacity(stored_ids.len());

    for stored_id in stored_ids {
        let batch_id = stored_id
            .parse::<SynchronizationBatchId>()
            .map_err(|_| ReaderSynchronizationStateError::invalid_stored_value("batch_id"))?;

        batches.push(load_synchronization_batch(pool, reader_id, batch_id).await?);
    }

    Ok(batches)
}

fn validate_operation_time(unix_milliseconds: i64) -> Result<(), ReaderSynchronizationStateError> {
    if unix_milliseconds < 0 {
        return Err(ReaderSynchronizationStateError::NegativeOperationTime { unix_milliseconds });
    }

    Ok(())
}

fn validate_failure_category(
    failure_category: &str,
) -> Result<String, ReaderSynchronizationStateError> {
    let failure_category = failure_category.trim();

    if failure_category.is_empty() {
        return Err(ReaderSynchronizationStateError::EmptyFailureCategory);
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
        ReaderSynchronizationError, SynchronizationBatchState, bind_reader_database,
        connect_reader_sqlite, create_synchronization_batch, enqueue_offline_transaction,
        load_offline_queue, run_reader_sqlite_migrations,
    };

    use super::{
        ReaderSynchronizationStateError, load_ready_synchronization_batches,
        mark_synchronization_batch_in_flight, record_synchronization_retryable_failure,
        recover_interrupted_synchronization_batches,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-sync-state-{name}-{}.sqlite3",
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
                "kind":
                    "offline_fare_transaction"
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

    async fn open_database(
        name: &str,
        reader_id: ReaderId,
    ) -> (PathBuf, ReaderSqliteConfig, SqlitePool) {
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

        (path, config, pool)
    }

    async fn enqueue(pool: &SqlitePool, reader_id: ReaderId) {
        if let Err(error) = enqueue_offline_transaction(pool, reader_id, &draft()).await {
            panic!("queue insertion failed: {error}");
        }
    }

    #[tokio::test]
    async fn retries_preserve_batch_and_transaction_identifiers() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("stable-retry", reader_id).await;

        enqueue(&pool, reader_id).await;
        enqueue(&pool, reader_id).await;

        let prepared = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("batch creation failed: {error}")
            }
        };

        assert_eq!(prepared.attempt_count(), 0);

        let initial_entries = prepared.entries().to_vec();

        let first_submission = match mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            prepared.batch_id(),
            TEST_TIME + 300,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("first submission failed: {error}")
            }
        };

        assert_eq!(first_submission.batch_id(), prepared.batch_id());

        assert_eq!(first_submission.entries(), initial_entries.as_slice());

        assert_eq!(
            first_submission.state(),
            SynchronizationBatchState::InFlight
        );

        assert_eq!(first_submission.attempt_count(), 1);

        let retryable = match record_synchronization_retryable_failure(
            &pool,
            reader_id,
            prepared.batch_id(),
            "network_timeout",
            TEST_TIME + 400,
            TEST_TIME + 600,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("retryable failure failed: {error}")
            }
        };

        assert_eq!(retryable.batch_id(), prepared.batch_id());

        assert_eq!(retryable.entries(), initial_entries.as_slice());

        assert_eq!(
            retryable.state(),
            SynchronizationBatchState::RetryableFailure
        );

        assert_eq!(retryable.attempt_count(), 1);

        assert_eq!(
            retryable.next_retry_at_unix_milliseconds(),
            Some(TEST_TIME + 600)
        );

        assert_eq!(retryable.last_failure_category(), Some("network_timeout"));

        let before_retry =
            match load_ready_synchronization_batches(&pool, reader_id, TEST_TIME + 599, 10).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("ready-batch load failed: {error}")
                }
            };

        assert!(before_retry.is_empty());

        let ready =
            match load_ready_synchronization_batches(&pool, reader_id, TEST_TIME + 600, 10).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("ready-batch load failed: {error}")
                }
            };

        assert_eq!(ready.len(), 1);

        assert_eq!(ready[0].batch_id(), prepared.batch_id());

        assert_eq!(ready[0].entries(), initial_entries.as_slice());

        let second_submission = match mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            prepared.batch_id(),
            TEST_TIME + 600,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("second submission failed: {error}")
            }
        };

        assert_eq!(second_submission.batch_id(), prepared.batch_id());

        assert_eq!(second_submission.attempt_count(), 2);

        assert_eq!(second_submission.entries(), initial_entries.as_slice());

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
            assert_eq!(transaction.queue_state(), OfflineQueueState::InFlight);

            assert_eq!(transaction.attempt_count(), 2);
        }

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn restart_recovery_reuses_existing_batch() {
        let reader_id = ReaderId::generate();

        let (path, config, first_pool) = open_database("restart", reader_id).await;

        enqueue(&first_pool, reader_id).await;

        let prepared = match create_synchronization_batch(
            &first_pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                first_pool.close().await;
                remove_database(&path);

                panic!("batch creation failed: {error}")
            }
        };

        let submitted = mark_synchronization_batch_in_flight(
            &first_pool,
            reader_id,
            prepared.batch_id(),
            TEST_TIME + 300,
        )
        .await;

        assert!(submitted.is_ok());

        first_pool.close().await;

        let second_pool = match connect_reader_sqlite(&config).await {
            Ok(value) => value,

            Err(error) => {
                remove_database(&path);

                panic!("reopen failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&second_pool).await {
            second_pool.close().await;
            remove_database(&path);

            panic!("reopen migration failed: {error}");
        }

        if let Err(error) = bind_reader_database(&second_pool, &identity(reader_id)).await {
            second_pool.close().await;
            remove_database(&path);

            panic!("reopen binding failed: {error}");
        }

        let recovered = match recover_interrupted_synchronization_batches(
            &second_pool,
            reader_id,
            TEST_TIME + 1_000,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                second_pool.close().await;
                remove_database(&path);

                panic!("batch recovery failed: {error}")
            }
        };

        assert_eq!(recovered, 1);

        let ready = match load_ready_synchronization_batches(
            &second_pool,
            reader_id,
            TEST_TIME + 1_000,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                second_pool.close().await;
                remove_database(&path);

                panic!("ready-batch load failed: {error}")
            }
        };

        assert_eq!(ready.len(), 1);

        assert_eq!(ready[0].batch_id(), prepared.batch_id());

        assert_eq!(ready[0].entries(), prepared.entries());

        assert_eq!(ready[0].attempt_count(), 1);

        assert_eq!(
            ready[0].state(),
            SynchronizationBatchState::RetryableFailure
        );

        assert_eq!(ready[0].last_failure_category(), Some("reader_restart"));

        let replacement = create_synchronization_batch(
            &second_pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 1_100,
            10,
        )
        .await;

        assert!(matches!(
            replacement,
            Err(
                ReaderSynchronizationError::
                    NoEligibleTransactions {
                        reader_id:
                            failed_reader,
                    }
            ) if failed_reader == reader_id
        ));

        second_pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn retryable_failure_requires_submitted_batch() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("invalid-retry", reader_id).await;

        enqueue(&pool, reader_id).await;

        let prepared = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("batch creation failed: {error}")
            }
        };

        let result = record_synchronization_retryable_failure(
            &pool,
            reader_id,
            prepared.batch_id(),
            "network_timeout",
            TEST_TIME + 300,
            TEST_TIME + 500,
        )
        .await;

        assert!(matches!(
            result,
            Err(
                ReaderSynchronizationStateError::
                    InvalidTransition {
                        batch_id,
                        ..
                    }
            ) if batch_id == prepared.batch_id()
        ));

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn ready_batches_are_bounded_and_ordered() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("ready-order", reader_id).await;

        enqueue(&pool, reader_id).await;
        enqueue(&pool, reader_id).await;

        let first = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            1,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("first batch failed: {error}")
            }
        };

        let second = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 300,
            1,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("second batch failed: {error}")
            }
        };

        let ready =
            match load_ready_synchronization_batches(&pool, reader_id, TEST_TIME + 400, 1).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("ready-batch load failed: {error}")
                }
            };

        assert_eq!(ready.len(), 1);

        assert_eq!(ready[0].batch_id(), first.batch_id());

        assert_ne!(first.batch_id(), second.batch_id());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn submission_rolls_back_when_batch_entry_is_not_in_flight() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("entry-conflict", reader_id).await;

        enqueue(&pool, reader_id).await;
        enqueue(&pool, reader_id).await;

        let prepared = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("batch creation failed: {error}")
            }
        };

        let conflicting_transaction_id = prepared.entries()[0].transaction_id();

        let state_update = sqlx::query(
            r#"
            UPDATE offline_transactions
            SET
                queue_state = 'retryable_failure',
                next_retry_at_unix_milliseconds = ?,
                last_failure_category = 'test_conflict',
                updated_at_unix_milliseconds = ?
            WHERE
                fare_transaction_id = ?
                AND reader_id = ?
                AND queue_state = 'in_flight'
            "#,
        )
        .bind(TEST_TIME + 500)
        .bind(TEST_TIME + 250)
        .bind(conflicting_transaction_id.to_string())
        .bind(reader_id.to_string())
        .execute(&pool)
        .await;

        let state_update = match state_update {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("queue-state preparation failed: {error}")
            }
        };

        assert_eq!(state_update.rows_affected(), 1);

        let result = mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            prepared.batch_id(),
            TEST_TIME + 300,
        )
        .await;

        assert!(matches!(
            result,
            Err(
                ReaderSynchronizationStateError::
                    BatchEntryConflict {
                        batch_id,
                        expected: 2,
                        updated: 1,
                    }
            ) if batch_id == prepared.batch_id()
        ));

        let ready =
            match load_ready_synchronization_batches(&pool, reader_id, TEST_TIME + 300, 10).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("ready-batch load failed: {error}")
                }
            };

        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].batch_id(), prepared.batch_id());

        assert_eq!(ready[0].state(), SynchronizationBatchState::Prepared);

        assert_eq!(ready[0].attempt_count(), 0);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue.len(), 2);

        assert!(
            queue
                .iter()
                .all(|transaction| transaction.attempt_count() == 0)
        );

        assert_eq!(
            queue
                .iter()
                .filter(|transaction| { transaction.queue_state() == OfflineQueueState::InFlight })
                .count(),
            1
        );

        assert_eq!(
            queue
                .iter()
                .filter(|transaction| {
                    transaction.queue_state() == OfflineQueueState::RetryableFailure
                })
                .count(),
            1
        );

        pool.close().await;
        remove_database(&path);
    }
}
