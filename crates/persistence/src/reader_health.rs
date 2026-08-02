use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_domain::ReaderId;

/// Durable transaction counts grouped by queue state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReaderQueueHealthCounts {
    pending: u64,
    in_flight: u64,
    acknowledged: u64,
    retryable_failure: u64,
    permanent_failure: u64,
    manual_review: u64,
}

impl ReaderQueueHealthCounts {
    /// Returns transactions waiting for their first batch.
    #[must_use]
    pub const fn pending(self) -> u64 {
        self.pending
    }

    /// Returns transactions currently associated with active submissions.
    #[must_use]
    pub const fn in_flight(self) -> u64 {
        self.in_flight
    }

    /// Returns transactions accepted by the backend.
    #[must_use]
    pub const fn acknowledged(self) -> u64 {
        self.acknowledged
    }

    /// Returns transactions eligible for a future retry.
    #[must_use]
    pub const fn retryable_failure(self) -> u64 {
        self.retryable_failure
    }

    /// Returns transactions retained after a final failure.
    #[must_use]
    pub const fn permanent_failure(self) -> u64 {
        self.permanent_failure
    }

    /// Returns transactions retained for operator review.
    #[must_use]
    pub const fn manual_review(self) -> u64 {
        self.manual_review
    }

    /// Returns every durable offline transaction.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.pending
            .saturating_add(self.in_flight)
            .saturating_add(self.acknowledged)
            .saturating_add(self.retryable_failure)
            .saturating_add(self.permanent_failure)
            .saturating_add(self.manual_review)
    }

    /// Returns transactions that have not reached a final automated state.
    #[must_use]
    pub const fn unresolved(self) -> u64 {
        self.pending
            .saturating_add(self.in_flight)
            .saturating_add(self.retryable_failure)
            .saturating_add(self.manual_review)
    }

    /// Returns all retained failure and review records.
    #[must_use]
    pub const fn retained_failures(self) -> u64 {
        self.retryable_failure
            .saturating_add(self.permanent_failure)
            .saturating_add(self.manual_review)
    }
}

/// Durable synchronization work grouped by lifecycle state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReaderSynchronizationHealthCounts {
    prepared_batches: u64,
    in_flight_batches: u64,
    retryable_batches: u64,
    unapplied_acknowledgements: u64,
}

impl ReaderSynchronizationHealthCounts {
    /// Returns batches durably prepared for submission.
    #[must_use]
    pub const fn prepared_batches(self) -> u64 {
        self.prepared_batches
    }

    /// Returns submitted batches awaiting resolution.
    #[must_use]
    pub const fn in_flight_batches(self) -> u64 {
        self.in_flight_batches
    }

    /// Returns batches eligible for resubmission.
    #[must_use]
    pub const fn retryable_batches(self) -> u64 {
        self.retryable_batches
    }

    /// Returns stored acknowledgements not yet applied to the queue.
    #[must_use]
    pub const fn unapplied_acknowledgements(self) -> u64 {
        self.unapplied_acknowledgements
    }

    /// Returns every active synchronization batch.
    #[must_use]
    pub const fn active_batches(self) -> u64 {
        self.prepared_batches
            .saturating_add(self.in_flight_batches)
            .saturating_add(self.retryable_batches)
    }
}

/// Point-in-time durable health information for one reader database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderQueueHealthSnapshot {
    reader_id: ReaderId,
    observed_at_unix_milliseconds: i64,
    next_local_sequence: u64,
    last_acknowledged_sequence: u64,
    queue_counts: ReaderQueueHealthCounts,
    synchronization_counts: ReaderSynchronizationHealthCounts,
    lowest_unresolved_sequence: Option<u64>,
    oldest_unresolved_age_milliseconds: Option<u64>,
    next_retry_at_unix_milliseconds: Option<i64>,
}

impl ReaderQueueHealthSnapshot {
    /// Returns the reader represented by this snapshot.
    #[must_use]
    pub const fn reader_id(self) -> ReaderId {
        self.reader_id
    }

    /// Returns when the snapshot was observed.
    #[must_use]
    pub const fn observed_at_unix_milliseconds(self) -> i64 {
        self.observed_at_unix_milliseconds
    }

    /// Returns the sequence that will be allocated next.
    #[must_use]
    pub const fn next_local_sequence(self) -> u64 {
        self.next_local_sequence
    }

    /// Returns the highest contiguous final sequence.
    #[must_use]
    pub const fn last_acknowledged_sequence(self) -> u64 {
        self.last_acknowledged_sequence
    }

    /// Returns queue-state counts.
    #[must_use]
    pub const fn queue_counts(self) -> ReaderQueueHealthCounts {
        self.queue_counts
    }

    /// Returns synchronization lifecycle counts.
    #[must_use]
    pub const fn synchronization_counts(self) -> ReaderSynchronizationHealthCounts {
        self.synchronization_counts
    }

    /// Returns the lowest unresolved reader-local sequence.
    #[must_use]
    pub const fn lowest_unresolved_sequence(self) -> Option<u64> {
        self.lowest_unresolved_sequence
    }

    /// Returns the age of the oldest unresolved transaction.
    #[must_use]
    pub const fn oldest_unresolved_age_milliseconds(self) -> Option<u64> {
        self.oldest_unresolved_age_milliseconds
    }

    /// Returns the earliest scheduled transaction retry.
    #[must_use]
    pub const fn next_retry_at_unix_milliseconds(self) -> Option<i64> {
        self.next_retry_at_unix_milliseconds
    }

    /// Returns whether durable synchronization work remains.
    #[must_use]
    pub const fn has_pending_work(self) -> bool {
        self.queue_counts.unresolved() > 0
            || self.synchronization_counts.active_batches() > 0
            || self.synchronization_counts.unapplied_acknowledgements() > 0
    }

    /// Returns whether retained records require operator attention.
    #[must_use]
    pub const fn requires_operator_attention(self) -> bool {
        self.queue_counts.permanent_failure() > 0 || self.queue_counts.manual_review() > 0
    }
}

/// Stable failures produced while loading reader health.
#[derive(Debug, Error)]
pub enum ReaderHealthError {
    /// Observation times cannot predate the Unix epoch.
    #[error("reader health observation time cannot be negative: {unix_milliseconds}")]
    NegativeObservationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// The requested reader database has not been bound.
    #[error("reader health state was not found for reader {reader_id}")]
    ReaderNotFound {
        /// Expected reader identity.
        reader_id: ReaderId,
    },

    /// A stored transaction timestamp was later than the observation.
    #[error(
        "reader health observation time {observed_at_unix_milliseconds} precedes stored queue activity at {stored_at_unix_milliseconds}"
    )]
    ObservationBeforeQueueActivity {
        /// Requested observation time.
        observed_at_unix_milliseconds: i64,

        /// Stored queue creation time.
        stored_at_unix_milliseconds: i64,
    },

    /// SQLite contained invalid health data.
    #[error("reader queue health contains an invalid stored value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A named SQLite health operation failed.
    #[error("reader SQLite health operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderHealthError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

#[derive(sqlx::FromRow)]
struct StoredReaderQueueHealth {
    next_local_sequence: i64,
    last_acknowledged_sequence: i64,
    pending_count: i64,
    in_flight_count: i64,
    acknowledged_count: i64,
    retryable_failure_count: i64,
    permanent_failure_count: i64,
    manual_review_count: i64,
    prepared_batch_count: i64,
    in_flight_batch_count: i64,
    retryable_batch_count: i64,
    unapplied_acknowledgement_count: i64,
    lowest_unresolved_sequence: Option<i64>,
    oldest_unresolved_created_at_unix_milliseconds: Option<i64>,
    next_retry_at_unix_milliseconds: Option<i64>,
}

/// Loads a point-in-time health snapshot from durable reader state.
pub async fn load_reader_queue_health(
    pool: &SqlitePool,
    reader_id: ReaderId,
    observed_at_unix_milliseconds: i64,
) -> Result<ReaderQueueHealthSnapshot, ReaderHealthError> {
    if observed_at_unix_milliseconds < 0 {
        return Err(ReaderHealthError::NegativeObservationTime {
            unix_milliseconds: observed_at_unix_milliseconds,
        });
    }

    let stored = sqlx::query_as::<_, StoredReaderQueueHealth>(
        r#"
        SELECT
            reader.next_local_sequence,
            reader.last_acknowledged_sequence,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'pending'
            ) AS pending_count,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'in_flight'
            ) AS in_flight_count,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'acknowledged'
            ) AS acknowledged_count,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'retryable_failure'
            ) AS retryable_failure_count,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'permanent_failure'
            ) AS permanent_failure_count,

            (
                SELECT COUNT(*)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'manual_review'
            ) AS manual_review_count,

            (
                SELECT COUNT(*)
                FROM synchronization_batches
                WHERE
                    reader_id = reader.reader_id
                    AND batch_state = 'prepared'
            ) AS prepared_batch_count,

            (
                SELECT COUNT(*)
                FROM synchronization_batches
                WHERE
                    reader_id = reader.reader_id
                    AND batch_state = 'in_flight'
            ) AS in_flight_batch_count,

            (
                SELECT COUNT(*)
                FROM synchronization_batches
                WHERE
                    reader_id = reader.reader_id
                    AND batch_state = 'retryable_failure'
            ) AS retryable_batch_count,

            (
                SELECT COUNT(*)
                FROM synchronization_acknowledgements
                WHERE
                    reader_id = reader.reader_id
                    AND applied_at_unix_milliseconds IS NULL
            ) AS unapplied_acknowledgement_count,

            (
                SELECT MIN(local_sequence_number)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state IN (
                        'pending',
                        'in_flight',
                        'retryable_failure',
                        'manual_review'
                    )
            ) AS lowest_unresolved_sequence,

            (
                SELECT MIN(created_at_unix_milliseconds)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state IN (
                        'pending',
                        'in_flight',
                        'retryable_failure',
                        'manual_review'
                    )
            ) AS oldest_unresolved_created_at_unix_milliseconds,

            (
                SELECT MIN(next_retry_at_unix_milliseconds)
                FROM offline_transactions
                WHERE
                    reader_id = reader.reader_id
                    AND queue_state = 'retryable_failure'
            ) AS next_retry_at_unix_milliseconds

        FROM reader_state AS reader
        WHERE
            reader.singleton = 1
            AND reader.reader_id = ?
        "#,
    )
    .bind(reader_id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|source| ReaderHealthError::database("load queue health", source))?
    .ok_or(ReaderHealthError::ReaderNotFound { reader_id })?;

    decode_health_snapshot(reader_id, observed_at_unix_milliseconds, stored)
}

fn decode_health_snapshot(
    reader_id: ReaderId,
    observed_at_unix_milliseconds: i64,
    stored: StoredReaderQueueHealth,
) -> Result<ReaderQueueHealthSnapshot, ReaderHealthError> {
    let next_local_sequence = positive_u64(stored.next_local_sequence, "next_local_sequence")?;

    let last_acknowledged_sequence = nonnegative_u64(
        stored.last_acknowledged_sequence,
        "last_acknowledged_sequence",
    )?;

    if last_acknowledged_sequence >= next_local_sequence {
        return Err(ReaderHealthError::invalid_stored_value(
            "last_acknowledged_sequence",
        ));
    }

    let queue_counts = ReaderQueueHealthCounts {
        pending: count(stored.pending_count, "pending_count")?,
        in_flight: count(stored.in_flight_count, "in_flight_count")?,
        acknowledged: count(stored.acknowledged_count, "acknowledged_count")?,
        retryable_failure: count(stored.retryable_failure_count, "retryable_failure_count")?,
        permanent_failure: count(stored.permanent_failure_count, "permanent_failure_count")?,
        manual_review: count(stored.manual_review_count, "manual_review_count")?,
    };

    let synchronization_counts = ReaderSynchronizationHealthCounts {
        prepared_batches: count(stored.prepared_batch_count, "prepared_batch_count")?,
        in_flight_batches: count(stored.in_flight_batch_count, "in_flight_batch_count")?,
        retryable_batches: count(stored.retryable_batch_count, "retryable_batch_count")?,
        unapplied_acknowledgements: count(
            stored.unapplied_acknowledgement_count,
            "unapplied_acknowledgement_count",
        )?,
    };

    let lowest_unresolved_sequence = optional_positive_u64(
        stored.lowest_unresolved_sequence,
        "lowest_unresolved_sequence",
    )?;

    let oldest_created_at = optional_nonnegative_i64(
        stored.oldest_unresolved_created_at_unix_milliseconds,
        "oldest_unresolved_created_at_unix_milliseconds",
    )?;

    if lowest_unresolved_sequence.is_some() != oldest_created_at.is_some() {
        return Err(ReaderHealthError::invalid_stored_value(
            "unresolved_queue_metadata",
        ));
    }

    let oldest_unresolved_age_milliseconds = match oldest_created_at {
        Some(created_at) => {
            if created_at > observed_at_unix_milliseconds {
                return Err(ReaderHealthError::ObservationBeforeQueueActivity {
                    observed_at_unix_milliseconds,
                    stored_at_unix_milliseconds: created_at,
                });
            }

            Some(
                u64::try_from(observed_at_unix_milliseconds - created_at).map_err(|_| {
                    ReaderHealthError::invalid_stored_value("oldest_unresolved_age_milliseconds")
                })?,
            )
        }

        None => None,
    };

    let next_retry_at_unix_milliseconds = optional_nonnegative_i64(
        stored.next_retry_at_unix_milliseconds,
        "next_retry_at_unix_milliseconds",
    )?;

    Ok(ReaderQueueHealthSnapshot {
        reader_id,
        observed_at_unix_milliseconds,
        next_local_sequence,
        last_acknowledged_sequence,
        queue_counts,
        synchronization_counts,
        lowest_unresolved_sequence,
        oldest_unresolved_age_milliseconds,
        next_retry_at_unix_milliseconds,
    })
}

fn count(value: i64, field: &'static str) -> Result<u64, ReaderHealthError> {
    nonnegative_u64(value, field)
}

fn positive_u64(value: i64, field: &'static str) -> Result<u64, ReaderHealthError> {
    let value = nonnegative_u64(value, field)?;

    if value == 0 {
        return Err(ReaderHealthError::invalid_stored_value(field));
    }

    Ok(value)
}

fn nonnegative_u64(value: i64, field: &'static str) -> Result<u64, ReaderHealthError> {
    u64::try_from(value).map_err(|_| ReaderHealthError::invalid_stored_value(field))
}

fn optional_positive_u64(
    value: Option<i64>,
    field: &'static str,
) -> Result<Option<u64>, ReaderHealthError> {
    value.map(|value| positive_u64(value, field)).transpose()
}

fn optional_nonnegative_i64(
    value: Option<i64>,
    field: &'static str,
) -> Result<Option<i64>, ReaderHealthError> {
    match value {
        Some(value) if value < 0 => Err(ReaderHealthError::invalid_stored_value(field)),

        value => Ok(value),
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
        OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig,
        SynchronizationAcknowledgement, SynchronizationAcknowledgementEntry,
        SynchronizationEntryResolution, bind_reader_database, connect_reader_sqlite,
        create_synchronization_batch, enqueue_offline_transaction,
        mark_synchronization_batch_in_flight, run_reader_sqlite_migrations,
        store_synchronization_acknowledgement,
    };

    use super::{ReaderHealthError, load_reader_queue_health};

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-reader-health-{name}-{}.sqlite3",
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

    async fn enqueue_many(pool: &SqlitePool, reader_id: ReaderId, count: usize) {
        for _ in 0..count {
            if let Err(error) = enqueue_offline_transaction(pool, reader_id, &draft()).await {
                panic!("queue insertion failed: {error}");
            }
        }
    }

    #[tokio::test]
    async fn empty_reader_reports_zero_health() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("empty", reader_id).await;

        let snapshot = match load_reader_queue_health(&pool, reader_id, TEST_TIME + 500).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("health load failed: {error}");
            }
        };

        assert_eq!(snapshot.next_local_sequence(), 1);
        assert_eq!(snapshot.last_acknowledged_sequence(), 0);
        assert_eq!(snapshot.queue_counts().total(), 0);
        assert_eq!(snapshot.synchronization_counts().active_batches(), 0);
        assert_eq!(
            snapshot
                .synchronization_counts()
                .unapplied_acknowledgements(),
            0
        );
        assert_eq!(snapshot.lowest_unresolved_sequence(), None);
        assert_eq!(snapshot.oldest_unresolved_age_milliseconds(), None);
        assert_eq!(snapshot.next_retry_at_unix_milliseconds(), None);
        assert!(!snapshot.has_pending_work());
        assert!(!snapshot.requires_operator_attention());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn health_reports_mixed_queue_and_batch_states() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("mixed-states", reader_id).await;

        enqueue_many(&pool, reader_id, 5).await;

        let batch = create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            2,
        )
        .await;

        assert!(batch.is_ok());

        let updated = sqlx::query(
            r#"
            UPDATE offline_transactions
            SET
                queue_state = CASE local_sequence_number
                    WHEN 3 THEN 'retryable_failure'
                    WHEN 4 THEN 'permanent_failure'
                    WHEN 5 THEN 'manual_review'
                END,
                next_retry_at_unix_milliseconds =
                    CASE local_sequence_number
                        WHEN 3 THEN ?
                        ELSE NULL
                    END,
                last_failure_category =
                    CASE local_sequence_number
                        WHEN 3 THEN 'backend_timeout'
                        WHEN 4 THEN 'invalid_envelope'
                        WHEN 5 THEN 'operator_investigation'
                    END,
                updated_at_unix_milliseconds = ?
            WHERE
                reader_id = ?
                AND local_sequence_number BETWEEN 3 AND 5
            "#,
        )
        .bind(TEST_TIME + 1_000)
        .bind(TEST_TIME + 300)
        .bind(reader_id.to_string())
        .execute(&pool)
        .await;

        assert!(matches!(
            updated,
            Ok(result) if result.rows_affected() == 3
        ));

        let snapshot = match load_reader_queue_health(&pool, reader_id, TEST_TIME + 500).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("health load failed: {error}");
            }
        };

        let queue = snapshot.queue_counts();

        assert_eq!(queue.pending(), 0);
        assert_eq!(queue.in_flight(), 2);
        assert_eq!(queue.acknowledged(), 0);
        assert_eq!(queue.retryable_failure(), 1);
        assert_eq!(queue.permanent_failure(), 1);
        assert_eq!(queue.manual_review(), 1);
        assert_eq!(queue.total(), 5);
        assert_eq!(queue.unresolved(), 4);
        assert_eq!(queue.retained_failures(), 3);

        let synchronization = snapshot.synchronization_counts();

        assert_eq!(synchronization.prepared_batches(), 1);
        assert_eq!(synchronization.in_flight_batches(), 0);
        assert_eq!(synchronization.retryable_batches(), 0);
        assert_eq!(synchronization.active_batches(), 1);

        assert_eq!(snapshot.lowest_unresolved_sequence(), Some(1));
        assert_eq!(snapshot.oldest_unresolved_age_milliseconds(), Some(400));
        assert_eq!(
            snapshot.next_retry_at_unix_milliseconds(),
            Some(TEST_TIME + 1_000)
        );
        assert!(snapshot.has_pending_work());
        assert!(snapshot.requires_operator_attention());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn unapplied_acknowledgement_is_visible() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("unapplied-ack", reader_id).await;

        enqueue_many(&pool, reader_id, 1).await;

        let prepared = match create_synchronization_batch(
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
                panic!("batch creation failed: {error}");
            }
        };

        let batch = match mark_synchronization_batch_in_flight(
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
                panic!("batch submission failed: {error}");
            }
        };

        let entry = batch.entries()[0];

        let acknowledgement = match SynchronizationAcknowledgement::new(
            reader_id,
            batch.batch_id(),
            batch.protocol_version(),
            batch.first_local_sequence_number(),
            batch.last_local_sequence_number(),
            TEST_TIME + 400,
            vec![SynchronizationAcknowledgementEntry::new(
                entry.transaction_id(),
                entry.local_sequence_number(),
                SynchronizationEntryResolution::Acknowledged,
            )],
        ) {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("acknowledgement creation failed: {error}");
            }
        };

        let stored = store_synchronization_acknowledgement(&pool, &acknowledgement).await;

        assert!(stored.is_ok());

        let snapshot = match load_reader_queue_health(&pool, reader_id, TEST_TIME + 500).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("health load failed: {error}");
            }
        };

        assert_eq!(snapshot.queue_counts().in_flight(), 1);
        assert_eq!(snapshot.synchronization_counts().in_flight_batches(), 1);
        assert_eq!(
            snapshot
                .synchronization_counts()
                .unapplied_acknowledgements(),
            1
        );
        assert!(snapshot.has_pending_work());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn unknown_reader_health_is_rejected() {
        let bound_reader = ReaderId::generate();
        let missing_reader = ReaderId::generate();

        let (path, pool) = open_database("missing-reader", bound_reader).await;

        let result = load_reader_queue_health(&pool, missing_reader, TEST_TIME + 500).await;

        assert!(matches!(
            result,
            Err(ReaderHealthError::ReaderNotFound {
                reader_id,
            }) if reader_id == missing_reader
        ));

        pool.close().await;
        remove_database(&path);
    }
}
