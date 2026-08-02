use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_domain::{FareTransactionId, ReaderId};

use crate::{OfflineQueueState, QueuedOfflineTransaction, ReaderQueueError, load_offline_queue};

const RESTART_FAILURE_CATEGORY: &str = "reader_restart";

/// Stable failures produced by durable queue-state transitions.
#[derive(Debug, Error)]
pub enum ReaderQueueStateError {
    /// Queue operation times cannot predate the Unix epoch.
    #[error("reader queue operation time cannot be negative: {unix_milliseconds}")]
    NegativeOperationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// A failure category must contain a stable nonblank value.
    #[error("reader queue failure category must not be empty")]
    EmptyFailureCategory,

    /// Retry scheduling cannot move backward in time.
    #[error(
        "reader queue retry time {retry_at_unix_milliseconds} cannot precede update time {updated_at_unix_milliseconds}"
    )]
    RetryBeforeUpdate {
        /// Time at which retry becomes eligible.
        retry_at_unix_milliseconds: i64,

        /// Time at which the failure was recorded.
        updated_at_unix_milliseconds: i64,
    },

    /// Ready-queue reads must request at least one transaction.
    #[error("reader queue read limit must be greater than zero")]
    ZeroReadLimit,

    /// The requested state change was not valid for the durable record.
    #[error("offline transaction {transaction_id} does not permit transition `{transition}`")]
    InvalidTransition {
        /// Stable transaction identity.
        transaction_id: FareTransactionId,

        /// Stable transition name safe for logs.
        transition: &'static str,
    },

    /// An underlying queue read failed.
    #[error(transparent)]
    Queue(#[from] ReaderQueueError),

    /// A named SQLite transition operation failed.
    #[error("reader SQLite queue-state operation `{operation}` failed")]
    Database {
        /// Stable operation category safe for logs.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderQueueStateError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_transition(
        transaction_id: FareTransactionId,
        transition: &'static str,
    ) -> Self {
        Self::InvalidTransition {
            transaction_id,
            transition,
        }
    }
}

/// Marks a pending or eligible retryable transaction as in flight.
///
/// The attempt count increases only when the conditional update succeeds.
/// Retry metadata is cleared when the transaction begins another attempt.
pub async fn mark_offline_transaction_in_flight(
    pool: &SqlitePool,
    reader_id: ReaderId,
    transaction_id: FareTransactionId,
    attempted_at_unix_milliseconds: i64,
) -> Result<(), ReaderQueueStateError> {
    validate_operation_time(attempted_at_unix_milliseconds)?;

    let result = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            queue_state = 'in_flight',
            attempt_count = attempt_count + 1,
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = NULL,
            updated_at_unix_milliseconds = ?
        WHERE
            fare_transaction_id = ?
            AND reader_id = ?
            AND updated_at_unix_milliseconds <= ?
            AND (
                queue_state = 'pending'
                OR (
                    queue_state = 'retryable_failure'
                    AND (
                        next_retry_at_unix_milliseconds IS NULL
                        OR next_retry_at_unix_milliseconds <= ?
                    )
                )
            )
        "#,
    )
    .bind(attempted_at_unix_milliseconds)
    .bind(transaction_id.to_string())
    .bind(reader_id.to_string())
    .bind(attempted_at_unix_milliseconds)
    .bind(attempted_at_unix_milliseconds)
    .execute(pool)
    .await
    .map_err(|source| ReaderQueueStateError::database("mark transaction in flight", source))?;

    require_changed_row(result.rows_affected(), transaction_id, "mark_in_flight")
}

/// Records a retryable synchronization failure.
///
/// The transaction must currently be in flight. It becomes eligible again
/// only when its configured retry time is reached.
pub async fn record_retryable_queue_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    transaction_id: FareTransactionId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
    retry_at_unix_milliseconds: i64,
) -> Result<(), ReaderQueueStateError> {
    validate_operation_time(updated_at_unix_milliseconds)?;

    validate_operation_time(retry_at_unix_milliseconds)?;

    if retry_at_unix_milliseconds < updated_at_unix_milliseconds {
        return Err(ReaderQueueStateError::RetryBeforeUpdate {
            retry_at_unix_milliseconds,
            updated_at_unix_milliseconds,
        });
    }

    let failure_category = validate_failure_category(failure_category)?;

    let result = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            queue_state = 'retryable_failure',
            next_retry_at_unix_milliseconds = ?,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            fare_transaction_id = ?
            AND reader_id = ?
            AND queue_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(retry_at_unix_milliseconds)
    .bind(failure_category)
    .bind(updated_at_unix_milliseconds)
    .bind(transaction_id.to_string())
    .bind(reader_id.to_string())
    .bind(updated_at_unix_milliseconds)
    .execute(pool)
    .await
    .map_err(|source| ReaderQueueStateError::database("record retryable failure", source))?;

    require_changed_row(
        result.rows_affected(),
        transaction_id,
        "record_retryable_failure",
    )
}

/// Records a final non-retryable backend rejection.
///
/// Permanent failures remain durable and are never returned as ready work.
pub async fn record_permanent_queue_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    transaction_id: FareTransactionId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
) -> Result<(), ReaderQueueStateError> {
    record_final_failure(
        pool,
        reader_id,
        transaction_id,
        failure_category,
        updated_at_unix_milliseconds,
        OfflineQueueState::PermanentFailure,
    )
    .await
}

/// Moves an unresolved transaction into manual review.
///
/// Manual-review transactions remain durable and are never silently retried.
pub async fn record_manual_review_required(
    pool: &SqlitePool,
    reader_id: ReaderId,
    transaction_id: FareTransactionId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
) -> Result<(), ReaderQueueStateError> {
    record_final_failure(
        pool,
        reader_id,
        transaction_id,
        failure_category,
        updated_at_unix_milliseconds,
        OfflineQueueState::ManualReview,
    )
    .await
}

/// Recovers transactions that were in flight when the reader stopped.
///
/// Interrupted entries become immediately eligible retryable failures.
/// Their transaction identity, local sequence, and attempt count are
/// preserved.
pub async fn recover_interrupted_offline_queue(
    pool: &SqlitePool,
    reader_id: ReaderId,
    recovered_at_unix_milliseconds: i64,
) -> Result<u64, ReaderQueueStateError> {
    validate_operation_time(recovered_at_unix_milliseconds)?;

    let result = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            queue_state = 'retryable_failure',
            next_retry_at_unix_milliseconds = ?,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            reader_id = ?
            AND queue_state = 'in_flight'
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
    .map_err(|source| ReaderQueueStateError::database("recover interrupted queue", source))?;

    Ok(result.rows_affected())
}

/// Loads queue entries currently eligible for synchronization.
///
/// Results retain the queue's reader-local sequence ordering.
pub async fn load_ready_offline_transactions(
    pool: &SqlitePool,
    reader_id: ReaderId,
    now_unix_milliseconds: i64,
    limit: usize,
) -> Result<Vec<QueuedOfflineTransaction>, ReaderQueueStateError> {
    validate_operation_time(now_unix_milliseconds)?;

    if limit == 0 {
        return Err(ReaderQueueStateError::ZeroReadLimit);
    }

    let queue = load_offline_queue(pool, reader_id).await?;

    Ok(queue
        .into_iter()
        .filter(|transaction| is_ready(transaction, now_unix_milliseconds))
        .take(limit)
        .collect())
}

async fn record_final_failure(
    pool: &SqlitePool,
    reader_id: ReaderId,
    transaction_id: FareTransactionId,
    failure_category: &str,
    updated_at_unix_milliseconds: i64,
    target_state: OfflineQueueState,
) -> Result<(), ReaderQueueStateError> {
    validate_operation_time(updated_at_unix_milliseconds)?;

    let failure_category = validate_failure_category(failure_category)?;

    let target_state_text = match target_state {
        OfflineQueueState::PermanentFailure => "permanent_failure",
        OfflineQueueState::ManualReview => "manual_review",
        _ => {
            return Err(ReaderQueueStateError::invalid_transition(
                transaction_id,
                "unsupported_final_state",
            ));
        }
    };

    let result = sqlx::query(
        r#"
        UPDATE offline_transactions
        SET
            queue_state = ?,
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = ?,
            updated_at_unix_milliseconds = ?
        WHERE
            fare_transaction_id = ?
            AND reader_id = ?
            AND queue_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(target_state_text)
    .bind(failure_category)
    .bind(updated_at_unix_milliseconds)
    .bind(transaction_id.to_string())
    .bind(reader_id.to_string())
    .bind(updated_at_unix_milliseconds)
    .execute(pool)
    .await
    .map_err(|source| ReaderQueueStateError::database("record final queue failure", source))?;

    require_changed_row(result.rows_affected(), transaction_id, target_state_text)
}

fn validate_operation_time(unix_milliseconds: i64) -> Result<(), ReaderQueueStateError> {
    if unix_milliseconds < 0 {
        return Err(ReaderQueueStateError::NegativeOperationTime { unix_milliseconds });
    }

    Ok(())
}

fn validate_failure_category(failure_category: &str) -> Result<String, ReaderQueueStateError> {
    let failure_category = failure_category.trim();

    if failure_category.is_empty() {
        return Err(ReaderQueueStateError::EmptyFailureCategory);
    }

    Ok(failure_category.to_owned())
}

fn require_changed_row(
    rows_affected: u64,
    transaction_id: FareTransactionId,
    transition: &'static str,
) -> Result<(), ReaderQueueStateError> {
    if rows_affected == 1 {
        return Ok(());
    }

    Err(ReaderQueueStateError::invalid_transition(
        transaction_id,
        transition,
    ))
}

fn is_ready(transaction: &QueuedOfflineTransaction, now_unix_milliseconds: i64) -> bool {
    match transaction.queue_state() {
        OfflineQueueState::Pending => true,

        OfflineQueueState::RetryableFailure => transaction
            .next_retry_at_unix_milliseconds()
            .is_none_or(|retry_at| retry_at <= now_unix_milliseconds),

        OfflineQueueState::InFlight
        | OfflineQueueState::Acknowledged
        | OfflineQueueState::PermanentFailure
        | OfflineQueueState::ManualReview => false,
    }
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
        OfflineQueueState, OfflineTransactionDraft, QueuedOfflineTransaction,
        ReaderDatabaseIdentity, ReaderSqliteConfig, bind_reader_database, connect_reader_sqlite,
        enqueue_offline_transaction, load_offline_queue, run_reader_sqlite_migrations,
    };

    use super::{
        ReaderQueueStateError, load_ready_offline_transactions, mark_offline_transaction_in_flight,
        record_manual_review_required, record_permanent_queue_failure,
        record_retryable_queue_failure, recover_interrupted_offline_queue,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn temporary_database_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-state-{test_name}-{}.sqlite3",
            Uuid::now_v7()
        ))
    }

    fn related_path(path: &Path, suffix: &str) -> PathBuf {
        let mut value = OsString::from(path.as_os_str());

        value.push(suffix);

        PathBuf::from(value)
    }

    fn remove_database_files(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(related_path(path, "-wal"));
        let _ = std::fs::remove_file(related_path(path, "-shm"));
    }

    fn valid_event_time() -> EventTime {
        match EventTime::from_unix_milliseconds(TEST_TIME) {
            Ok(value) => value,

            Err(error) => {
                panic!("valid event time failed: {error}")
            }
        }
    }

    fn valid_policy_version() -> FarePolicyVersion {
        match FarePolicyVersion::new(1) {
            Ok(value) => value,

            Err(error) => {
                panic!("valid policy version failed: {error}")
            }
        }
    }

    fn valid_decision() -> FareDecision {
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

    fn draft(transaction_id: FareTransactionId) -> OfflineTransactionDraft {
        match OfflineTransactionDraft::new(
            transaction_id,
            FareCredentialId::generate(),
            valid_event_time(),
            valid_policy_version(),
            valid_decision(),
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

    async fn open_bound_database(
        test_name: &str,
        reader_id: ReaderId,
    ) -> (PathBuf, ReaderSqliteConfig, SqlitePool) {
        let path = temporary_database_path(test_name);

        let config = match ReaderSqliteConfig::new(path.clone()) {
            Ok(value) => value,

            Err(error) => {
                panic!("valid configuration failed: {error}")
            }
        };

        let pool = match connect_reader_sqlite(&config).await {
            Ok(value) => value,

            Err(error) => {
                remove_database_files(&path);

                panic!("connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&pool).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("migrations failed: {error}");
        }

        let expected_identity = identity(reader_id);

        if let Err(error) = bind_reader_database(&pool, &expected_identity).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("identity binding failed: {error}");
        }

        (path, config, pool)
    }

    async fn enqueue_one(pool: &SqlitePool, reader_id: ReaderId) -> QueuedOfflineTransaction {
        match enqueue_offline_transaction(pool, reader_id, &draft(FareTransactionId::generate()))
            .await
        {
            Ok(value) => value,

            Err(error) => {
                panic!("queue insertion failed: {error}")
            }
        }
    }

    async fn load_one(pool: &SqlitePool, reader_id: ReaderId) -> QueuedOfflineTransaction {
        let queue = match load_offline_queue(pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                panic!("queue load failed: {error}")
            }
        };

        let [transaction] = queue.as_slice() else {
            panic!("expected exactly one transaction");
        };

        transaction.clone()
    }

    #[tokio::test]
    async fn pending_transaction_moves_to_in_flight() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("pending-in-flight", reader_id).await;

        let queued = enqueue_one(&pool, reader_id).await;

        let result = mark_offline_transaction_in_flight(
            &pool,
            reader_id,
            queued.transaction_id(),
            TEST_TIME + 200,
        )
        .await;

        assert!(result.is_ok());

        let loaded = load_one(&pool, reader_id).await;

        assert_eq!(loaded.queue_state(), OfflineQueueState::InFlight);

        assert_eq!(loaded.attempt_count(), 1);

        assert_eq!(
            loaded.local_sequence_number(),
            queued.local_sequence_number()
        );

        assert_eq!(loaded.transaction_id(), queued.transaction_id());

        assert_eq!(loaded.next_retry_at_unix_milliseconds(), None);

        assert_eq!(loaded.last_failure_category(), None);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn retryable_failure_respects_retry_time() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("retry-time", reader_id).await;

        let queued = enqueue_one(&pool, reader_id).await;

        let transaction_id = queued.transaction_id();

        let start =
            mark_offline_transaction_in_flight(&pool, reader_id, transaction_id, TEST_TIME + 200)
                .await;

        assert!(start.is_ok());

        let failure = record_retryable_queue_failure(
            &pool,
            reader_id,
            transaction_id,
            "network_timeout",
            TEST_TIME + 300,
            TEST_TIME + 500,
        )
        .await;

        assert!(failure.is_ok());

        let before_retry =
            match load_ready_offline_transactions(&pool, reader_id, TEST_TIME + 499, 10).await {
                Ok(value) => value,

                Err(error) => {
                    panic!("ready queue load failed: {error}")
                }
            };

        assert!(before_retry.is_empty());

        let at_retry =
            match load_ready_offline_transactions(&pool, reader_id, TEST_TIME + 500, 10).await {
                Ok(value) => value,

                Err(error) => {
                    panic!("ready queue load failed: {error}")
                }
            };

        assert_eq!(at_retry.len(), 1);

        let restart_attempt =
            mark_offline_transaction_in_flight(&pool, reader_id, transaction_id, TEST_TIME + 500)
                .await;

        assert!(restart_attempt.is_ok());

        let loaded = load_one(&pool, reader_id).await;

        assert_eq!(loaded.queue_state(), OfflineQueueState::InFlight);

        assert_eq!(loaded.attempt_count(), 2);

        assert_eq!(loaded.next_retry_at_unix_milliseconds(), None);

        assert_eq!(loaded.last_failure_category(), None);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn restart_recovery_preserves_identity_and_sequence() {
        let reader_id = ReaderId::generate();

        let (path, config, first_pool) = open_bound_database("restart-recovery", reader_id).await;

        let queued = enqueue_one(&first_pool, reader_id).await;

        let mark_result = mark_offline_transaction_in_flight(
            &first_pool,
            reader_id,
            queued.transaction_id(),
            TEST_TIME + 200,
        )
        .await;

        assert!(mark_result.is_ok());

        first_pool.close().await;

        let second_pool = match connect_reader_sqlite(&config).await {
            Ok(value) => value,

            Err(error) => {
                remove_database_files(&path);

                panic!("reopen failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&second_pool).await {
            second_pool.close().await;
            remove_database_files(&path);

            panic!("reopen migrations failed: {error}");
        }

        let expected_identity = identity(reader_id);

        if let Err(error) = bind_reader_database(&second_pool, &expected_identity).await {
            second_pool.close().await;
            remove_database_files(&path);

            panic!("reopen identity binding failed: {error}");
        }

        let recovered =
            match recover_interrupted_offline_queue(&second_pool, reader_id, TEST_TIME + 1_000)
                .await
            {
                Ok(value) => value,

                Err(error) => {
                    second_pool.close().await;
                    remove_database_files(&path);

                    panic!("restart recovery failed: {error}")
                }
            };

        assert_eq!(recovered, 1);

        let loaded = load_one(&second_pool, reader_id).await;

        assert_eq!(loaded.transaction_id(), queued.transaction_id());

        assert_eq!(
            loaded.local_sequence_number(),
            queued.local_sequence_number()
        );

        assert_eq!(loaded.queue_state(), OfflineQueueState::RetryableFailure);

        assert_eq!(loaded.attempt_count(), 1);

        assert_eq!(
            loaded.next_retry_at_unix_milliseconds(),
            Some(TEST_TIME + 1_000)
        );

        assert_eq!(loaded.last_failure_category(), Some("reader_restart"));

        let ready =
            match load_ready_offline_transactions(&second_pool, reader_id, TEST_TIME + 1_000, 10)
                .await
            {
                Ok(value) => value,

                Err(error) => {
                    second_pool.close().await;
                    remove_database_files(&path);

                    panic!("ready queue load failed: {error}")
                }
            };

        assert_eq!(ready.len(), 1);

        second_pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn final_failures_are_retained_but_not_ready() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("final-failures", reader_id).await;

        let permanent = enqueue_one(&pool, reader_id).await;

        let permanent_start = mark_offline_transaction_in_flight(
            &pool,
            reader_id,
            permanent.transaction_id(),
            TEST_TIME + 200,
        )
        .await;

        assert!(permanent_start.is_ok());

        let permanent_result = record_permanent_queue_failure(
            &pool,
            reader_id,
            permanent.transaction_id(),
            "invalid_envelope",
            TEST_TIME + 300,
        )
        .await;

        assert!(permanent_result.is_ok());

        let review = enqueue_one(&pool, reader_id).await;

        let review_start = mark_offline_transaction_in_flight(
            &pool,
            reader_id,
            review.transaction_id(),
            TEST_TIME + 400,
        )
        .await;

        assert!(review_start.is_ok());

        let review_result = record_manual_review_required(
            &pool,
            reader_id,
            review.transaction_id(),
            "sequence_investigation",
            TEST_TIME + 500,
        )
        .await;

        assert!(review_result.is_ok());

        let ready =
            match load_ready_offline_transactions(&pool, reader_id, TEST_TIME + 10_000, 10).await {
                Ok(value) => value,

                Err(error) => {
                    panic!("ready queue load failed: {error}")
                }
            };

        assert!(ready.is_empty());

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue.len(), 2);

        assert_eq!(queue[0].queue_state(), OfflineQueueState::PermanentFailure);

        assert_eq!(queue[1].queue_state(), OfflineQueueState::ManualReview);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn invalid_state_transition_is_rejected() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("invalid-transition", reader_id).await;

        let queued = enqueue_one(&pool, reader_id).await;

        let result = record_retryable_queue_failure(
            &pool,
            reader_id,
            queued.transaction_id(),
            "network_timeout",
            TEST_TIME + 200,
            TEST_TIME + 300,
        )
        .await;

        assert!(matches!(
            result,
            Err(
                ReaderQueueStateError::
                    InvalidTransition {
                        transaction_id,
                        ..
                    }
            ) if transaction_id
                == queued.transaction_id()
        ));

        let loaded = load_one(&pool, reader_id).await;

        assert_eq!(loaded.queue_state(), OfflineQueueState::Pending);

        assert_eq!(loaded.attempt_count(), 0);

        pool.close().await;
        remove_database_files(&path);
    }
}
