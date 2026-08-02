use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_device_protocol::DeviceProtocolVersion;
use transitguard_domain::{
    FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};

/// Durable state of one synchronization batch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SynchronizationBatchState {
    /// The batch is durably recorded and ready for submission.
    Prepared,

    /// The batch has been submitted and awaits resolution.
    InFlight,

    /// The same stable batch may be submitted again.
    RetryableFailure,

    /// The backend durably resolved the batch.
    Acknowledged,

    /// The batch received a final non-retryable failure.
    PermanentFailure,

    /// Automated processing cannot safely resolve the batch.
    ManualReview,
}

impl SynchronizationBatchState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::InFlight => "in_flight",
            Self::RetryableFailure => "retryable_failure",
            Self::Acknowledged => "acknowledged",
            Self::PermanentFailure => "permanent_failure",
            Self::ManualReview => "manual_review",
        }
    }

    fn parse(value: &str) -> Result<Self, ReaderSynchronizationError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "in_flight" => Ok(Self::InFlight),
            "retryable_failure" => Ok(Self::RetryableFailure),
            "acknowledged" => Ok(Self::Acknowledged),
            "permanent_failure" => Ok(Self::PermanentFailure),
            "manual_review" => Ok(Self::ManualReview),
            _ => Err(ReaderSynchronizationError::invalid_stored_value(
                "batch_state",
            )),
        }
    }
}

/// One ordered transaction reference inside a durable batch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SynchronizationBatchEntry {
    transaction_id: FareTransactionId,
    local_sequence_number: LocalSequenceNumber,
}

impl SynchronizationBatchEntry {
    /// Returns the stable fare-transaction identity.
    #[must_use]
    pub const fn transaction_id(self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader-local ordering value.
    #[must_use]
    pub const fn local_sequence_number(self) -> LocalSequenceNumber {
        self.local_sequence_number
    }
}

/// One durable reader synchronization batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynchronizationBatch {
    batch_id: SynchronizationBatchId,
    reader_id: ReaderId,
    protocol_version: DeviceProtocolVersion,
    first_local_sequence_number: LocalSequenceNumber,
    last_local_sequence_number: LocalSequenceNumber,
    state: SynchronizationBatchState,
    attempt_count: u32,
    next_retry_at_unix_milliseconds: Option<i64>,
    last_failure_category: Option<String>,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
    entries: Vec<SynchronizationBatchEntry>,
}

impl SynchronizationBatch {
    /// Returns the stable batch identity.
    #[must_use]
    pub const fn batch_id(&self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns the reader that owns the batch.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the project-owned protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the first sequence in the batch.
    #[must_use]
    pub const fn first_local_sequence_number(&self) -> LocalSequenceNumber {
        self.first_local_sequence_number
    }

    /// Returns the last sequence in the batch.
    #[must_use]
    pub const fn last_local_sequence_number(&self) -> LocalSequenceNumber {
        self.last_local_sequence_number
    }

    /// Returns the durable batch state.
    #[must_use]
    pub const fn state(&self) -> SynchronizationBatchState {
        self.state
    }

    /// Returns the number of submission attempts.
    #[must_use]
    pub const fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    /// Returns the next permitted retry time.
    #[must_use]
    pub const fn next_retry_at_unix_milliseconds(&self) -> Option<i64> {
        self.next_retry_at_unix_milliseconds
    }

    /// Returns the latest sanitized failure category.
    #[must_use]
    pub fn last_failure_category(&self) -> Option<&str> {
        self.last_failure_category.as_deref()
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at_unix_milliseconds(&self) -> i64 {
        self.created_at_unix_milliseconds
    }

    /// Returns the last update timestamp.
    #[must_use]
    pub const fn updated_at_unix_milliseconds(&self) -> i64 {
        self.updated_at_unix_milliseconds
    }

    /// Returns the ordered durable batch entries.
    #[must_use]
    pub fn entries(&self) -> &[SynchronizationBatchEntry] {
        &self.entries
    }
}

/// Stable synchronization persistence failures.
#[derive(Debug, Error)]
pub enum ReaderSynchronizationError {
    /// Operation times cannot predate the Unix epoch.
    #[error("reader synchronization time cannot be negative: {unix_milliseconds}")]
    NegativeOperationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// A synchronization batch must contain at least one entry.
    #[error("reader synchronization batch limit must be greater than zero")]
    ZeroBatchLimit,

    /// The requested limit cannot be represented by SQLite.
    #[error("reader synchronization batch limit {limit} is too large")]
    BatchLimitTooLarge {
        /// Unsupported requested limit.
        limit: usize,
    },

    /// No transaction was currently eligible for batching.
    #[error("reader {reader_id} has no eligible offline transactions")]
    NoEligibleTransactions {
        /// Reader whose queue was examined.
        reader_id: ReaderId,
    },

    /// The requested batch does not exist for the reader.
    #[error("synchronization batch {batch_id} was not found for reader {reader_id}")]
    BatchNotFound {
        /// Requested stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Expected reader identity.
        reader_id: ReaderId,
    },

    /// A queue entry changed before it could be reserved.
    #[error("offline transaction {transaction_id} could not be reserved for synchronization")]
    QueueStateConflict {
        /// Transaction that could not be reserved.
        transaction_id: FareTransactionId,
    },

    /// SQLite contained invalid synchronization data.
    #[error("reader synchronization storage contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A named SQLite synchronization operation failed.
    #[error("reader SQLite synchronization operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderSynchronizationError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

#[derive(sqlx::FromRow)]
struct CandidateTransaction {
    fare_transaction_id: String,
    local_sequence_number: i64,
}

#[derive(sqlx::FromRow)]
struct StoredSynchronizationBatch {
    batch_id: String,
    reader_id: String,
    protocol_version: i64,
    first_local_sequence_number: i64,
    last_local_sequence_number: i64,
    batch_state: String,
    attempt_count: i64,
    next_retry_at_unix_milliseconds: Option<i64>,
    last_failure_category: Option<String>,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
}

#[derive(sqlx::FromRow)]
struct StoredSynchronizationEntry {
    fare_transaction_id: String,
    local_sequence_number: i64,
    entry_position: i64,
}

/// Creates one durable, bounded synchronization batch.
///
/// Batch creation, entry association, and queue-state changes commit
/// atomically. Transactions already assigned to a batch are excluded so
/// retries can continue using their original stable batch identity.
pub async fn create_synchronization_batch(
    pool: &SqlitePool,
    reader_id: ReaderId,
    protocol_version: DeviceProtocolVersion,
    created_at_unix_milliseconds: i64,
    limit: usize,
) -> Result<SynchronizationBatch, ReaderSynchronizationError> {
    validate_operation_time(created_at_unix_milliseconds)?;

    if limit == 0 {
        return Err(ReaderSynchronizationError::ZeroBatchLimit);
    }

    let sqlite_limit = i64::try_from(limit)
        .map_err(|_| ReaderSynchronizationError::BatchLimitTooLarge { limit })?;

    let mut transaction = pool
        .begin()
        .await
        .map_err(|source| ReaderSynchronizationError::database("begin batch creation", source))?;

    let candidates = sqlx::query_as::<_, CandidateTransaction>(
        r#"
            SELECT
                queued_transaction.fare_transaction_id,
                queued_transaction.local_sequence_number
            FROM offline_transactions AS queued_transaction
            WHERE
                queued_transaction.reader_id = ?
                AND (
                    queued_transaction.queue_state = 'pending'
                    OR (
                        queued_transaction.queue_state =
                            'retryable_failure'
                        AND (
                            queued_transaction
                                .next_retry_at_unix_milliseconds
                                IS NULL
                            OR queued_transaction
                                .next_retry_at_unix_milliseconds
                                <= ?
                        )
                    )
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM synchronization_entries AS entry
                    WHERE
                        entry.fare_transaction_id =
                            queued_transaction.fare_transaction_id
                )
            ORDER BY
                queued_transaction.local_sequence_number
            LIMIT ?
            "#,
    )
    .bind(reader_id.to_string())
    .bind(created_at_unix_milliseconds)
    .bind(sqlite_limit)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|source| ReaderSynchronizationError::database("select batch candidates", source))?;

    if candidates.is_empty() {
        return Err(ReaderSynchronizationError::NoEligibleTransactions { reader_id });
    }

    let entries = candidates
        .into_iter()
        .map(decode_candidate)
        .collect::<Result<Vec<_>, _>>()?;

    let first_sequence = match entries.first() {
        Some(entry) => entry.local_sequence_number(),
        None => {
            return Err(ReaderSynchronizationError::invalid_stored_value(
                "batch_entries",
            ));
        }
    };

    let last_sequence = match entries.last() {
        Some(entry) => entry.local_sequence_number(),
        None => {
            return Err(ReaderSynchronizationError::invalid_stored_value(
                "batch_entries",
            ));
        }
    };

    let batch_id = SynchronizationBatchId::generate();

    sqlx::query(
        r#"
        INSERT INTO synchronization_batches (
            batch_id,
            reader_id,
            protocol_version,
            first_local_sequence_number,
            last_local_sequence_number,
            batch_state,
            attempt_count,
            next_retry_at_unix_milliseconds,
            last_failure_category,
            created_at_unix_milliseconds,
            updated_at_unix_milliseconds
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, 0, NULL, NULL, ?, ?
        )
        "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .bind(i64::from(protocol_version.value()))
    .bind(sequence_to_i64(first_sequence)?)
    .bind(sequence_to_i64(last_sequence)?)
    .bind(SynchronizationBatchState::Prepared.as_str())
    .bind(created_at_unix_milliseconds)
    .bind(created_at_unix_milliseconds)
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationError::database("insert synchronization batch", source)
    })?;

    for (position, entry) in entries.iter().copied().enumerate() {
        let position = i64::try_from(position)
            .map_err(|_| ReaderSynchronizationError::invalid_stored_value("entry_position"))?;

        let sequence = sequence_to_i64(entry.local_sequence_number())?;

        let update = sqlx::query(
            r#"
            UPDATE offline_transactions
            SET
                queue_state = 'in_flight',
                next_retry_at_unix_milliseconds = NULL,
                last_failure_category = NULL,
                updated_at_unix_milliseconds = ?
            WHERE
                fare_transaction_id = ?
                AND reader_id = ?
                AND local_sequence_number = ?
                AND updated_at_unix_milliseconds <= ?
                AND (
                    queue_state = 'pending'
                    OR (
                        queue_state =
                            'retryable_failure'
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
        .bind(created_at_unix_milliseconds)
        .bind(entry.transaction_id().to_string())
        .bind(reader_id.to_string())
        .bind(sequence)
        .bind(created_at_unix_milliseconds)
        .bind(created_at_unix_milliseconds)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            ReaderSynchronizationError::database("reserve batch transaction", source)
        })?;

        if update.rows_affected() != 1 {
            return Err(ReaderSynchronizationError::QueueStateConflict {
                transaction_id: entry.transaction_id(),
            });
        }

        sqlx::query(
            r#"
            INSERT INTO synchronization_entries (
                batch_id,
                reader_id,
                fare_transaction_id,
                local_sequence_number,
                entry_position
            )
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(batch_id.to_string())
        .bind(reader_id.to_string())
        .bind(entry.transaction_id().to_string())
        .bind(sequence)
        .bind(position)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            ReaderSynchronizationError::database("insert synchronization entry", source)
        })?;
    }

    transaction
        .commit()
        .await
        .map_err(|source| ReaderSynchronizationError::database("commit batch creation", source))?;

    Ok(SynchronizationBatch {
        batch_id,
        reader_id,
        protocol_version,
        first_local_sequence_number: first_sequence,
        last_local_sequence_number: last_sequence,
        state: SynchronizationBatchState::Prepared,
        attempt_count: 0,
        next_retry_at_unix_milliseconds: None,
        last_failure_category: None,
        created_at_unix_milliseconds,
        updated_at_unix_milliseconds: created_at_unix_milliseconds,
        entries,
    })
}

/// Loads one durable synchronization batch and its ordered entries.
pub async fn load_synchronization_batch(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
) -> Result<SynchronizationBatch, ReaderSynchronizationError> {
    let stored = sqlx::query_as::<_, StoredSynchronizationBatch>(
        r#"
            SELECT
                batch_id,
                reader_id,
                protocol_version,
                first_local_sequence_number,
                last_local_sequence_number,
                batch_state,
                attempt_count,
                next_retry_at_unix_milliseconds,
                last_failure_category,
                created_at_unix_milliseconds,
                updated_at_unix_milliseconds
            FROM synchronization_batches
            WHERE
                batch_id = ?
                AND reader_id = ?
            "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(|source| ReaderSynchronizationError::database("load synchronization batch", source))?
    .ok_or(ReaderSynchronizationError::BatchNotFound {
        batch_id,
        reader_id,
    })?;

    let stored_entries = sqlx::query_as::<_, StoredSynchronizationEntry>(
        r#"
            SELECT
                fare_transaction_id,
                local_sequence_number,
                entry_position
            FROM synchronization_entries
            WHERE
                batch_id = ?
                AND reader_id = ?
            ORDER BY entry_position
            "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_all(pool)
    .await
    .map_err(|source| {
        ReaderSynchronizationError::database("load synchronization entries", source)
    })?;

    decode_stored_batch(stored, stored_entries)
}

fn decode_candidate(
    stored: CandidateTransaction,
) -> Result<SynchronizationBatchEntry, ReaderSynchronizationError> {
    let transaction_id = stored
        .fare_transaction_id
        .parse::<FareTransactionId>()
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("fare_transaction_id"))?;

    let sequence = decode_sequence(stored.local_sequence_number, "local_sequence_number")?;

    Ok(SynchronizationBatchEntry {
        transaction_id,
        local_sequence_number: sequence,
    })
}

fn decode_stored_batch(
    stored: StoredSynchronizationBatch,
    stored_entries: Vec<StoredSynchronizationEntry>,
) -> Result<SynchronizationBatch, ReaderSynchronizationError> {
    let batch_id = stored
        .batch_id
        .parse::<SynchronizationBatchId>()
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("batch_id"))?;

    let reader_id = stored
        .reader_id
        .parse::<ReaderId>()
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("reader_id"))?;

    let protocol_value = u16::try_from(stored.protocol_version)
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("protocol_version"))?;

    let protocol_version = DeviceProtocolVersion::new(protocol_value)
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("protocol_version"))?;

    let first_sequence = decode_sequence(
        stored.first_local_sequence_number,
        "first_local_sequence_number",
    )?;

    let last_sequence = decode_sequence(
        stored.last_local_sequence_number,
        "last_local_sequence_number",
    )?;

    if last_sequence < first_sequence {
        return Err(ReaderSynchronizationError::invalid_stored_value(
            "last_local_sequence_number",
        ));
    }

    let state = SynchronizationBatchState::parse(&stored.batch_state)?;

    let attempt_count = u32::try_from(stored.attempt_count)
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("attempt_count"))?;

    if matches!(
        stored.next_retry_at_unix_milliseconds,
        Some(value) if value < 0
    ) {
        return Err(ReaderSynchronizationError::invalid_stored_value(
            "next_retry_at_unix_milliseconds",
        ));
    }

    if stored.created_at_unix_milliseconds < 0 {
        return Err(ReaderSynchronizationError::invalid_stored_value(
            "created_at_unix_milliseconds",
        ));
    }

    if stored.updated_at_unix_milliseconds < stored.created_at_unix_milliseconds {
        return Err(ReaderSynchronizationError::invalid_stored_value(
            "updated_at_unix_milliseconds",
        ));
    }

    let mut entries = Vec::with_capacity(stored_entries.len());

    for (expected_position, stored_entry) in stored_entries.into_iter().enumerate() {
        let actual_position = usize::try_from(stored_entry.entry_position)
            .map_err(|_| ReaderSynchronizationError::invalid_stored_value("entry_position"))?;

        if actual_position != expected_position {
            return Err(ReaderSynchronizationError::invalid_stored_value(
                "entry_position",
            ));
        }

        entries.push(decode_candidate(CandidateTransaction {
            fare_transaction_id: stored_entry.fare_transaction_id,
            local_sequence_number: stored_entry.local_sequence_number,
        })?);
    }

    let first_entry = match entries.first() {
        Some(entry) => entry,
        None => {
            return Err(ReaderSynchronizationError::invalid_stored_value(
                "batch_entries",
            ));
        }
    };

    let last_entry = match entries.last() {
        Some(entry) => entry,
        None => {
            return Err(ReaderSynchronizationError::invalid_stored_value(
                "batch_entries",
            ));
        }
    };

    if first_entry.local_sequence_number() != first_sequence
        || last_entry.local_sequence_number() != last_sequence
    {
        return Err(ReaderSynchronizationError::invalid_stored_value(
            "batch_sequence_range",
        ));
    }

    Ok(SynchronizationBatch {
        batch_id,
        reader_id,
        protocol_version,
        first_local_sequence_number: first_sequence,
        last_local_sequence_number: last_sequence,
        state,
        attempt_count,
        next_retry_at_unix_milliseconds: stored.next_retry_at_unix_milliseconds,
        last_failure_category: stored.last_failure_category,
        created_at_unix_milliseconds: stored.created_at_unix_milliseconds,
        updated_at_unix_milliseconds: stored.updated_at_unix_milliseconds,
        entries,
    })
}

fn decode_sequence(
    value: i64,
    field: &'static str,
) -> Result<LocalSequenceNumber, ReaderSynchronizationError> {
    let value = u64::try_from(value)
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value(field))?;

    LocalSequenceNumber::new(value)
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value(field))
}

fn sequence_to_i64(sequence: LocalSequenceNumber) -> Result<i64, ReaderSynchronizationError> {
    i64::try_from(sequence.value())
        .map_err(|_| ReaderSynchronizationError::invalid_stored_value("local_sequence_number"))
}

fn validate_operation_time(unix_milliseconds: i64) -> Result<(), ReaderSynchronizationError> {
    if unix_milliseconds < 0 {
        return Err(ReaderSynchronizationError::NegativeOperationTime { unix_milliseconds });
    }

    Ok(())
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
        bind_reader_database, connect_reader_sqlite, enqueue_offline_transaction,
        load_offline_queue, recover_interrupted_offline_queue, run_reader_sqlite_migrations,
    };

    use super::{
        ReaderSynchronizationError, SynchronizationBatchState, create_synchronization_batch,
        load_synchronization_batch,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-sync-{name}-{}.sqlite3",
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
    async fn batch_creation_is_bounded_and_ordered() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("bounded", reader_id).await;

        enqueue(&pool, reader_id).await;
        enqueue(&pool, reader_id).await;
        enqueue(&pool, reader_id).await;

        let batch = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            2,
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

        assert_eq!(batch.state(), SynchronizationBatchState::Prepared);
        assert_eq!(batch.entries().len(), 2);
        assert_eq!(batch.entries()[0].local_sequence_number().value(), 1);
        assert_eq!(batch.entries()[1].local_sequence_number().value(), 2);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue[0].queue_state(), OfflineQueueState::InFlight);
        assert_eq!(queue[1].queue_state(), OfflineQueueState::InFlight);
        assert_eq!(queue[2].queue_state(), OfflineQueueState::Pending);
        assert_eq!(queue[0].attempt_count(), 0);
        assert_eq!(queue[1].attempt_count(), 0);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn batch_identity_survives_reopen() {
        let reader_id = ReaderId::generate();

        let (path, config, first_pool) = open_database("reopen", reader_id).await;

        enqueue(&first_pool, reader_id).await;

        let expected = match create_synchronization_batch(
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

        let loaded =
            match load_synchronization_batch(&second_pool, reader_id, expected.batch_id()).await {
                Ok(value) => value,
                Err(error) => {
                    second_pool.close().await;
                    remove_database(&path);
                    panic!("batch reload failed: {error}")
                }
            };

        assert_eq!(loaded, expected);

        second_pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn batches_do_not_duplicate_transactions() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("no-duplicates", reader_id).await;

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

        assert_ne!(first.batch_id(), second.batch_id());
        assert_eq!(first.entries()[0].local_sequence_number().value(), 1);
        assert_eq!(second.entries()[0].local_sequence_number().value(), 2);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn queue_recovery_does_not_detach_batched_entries() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("recovery-boundary", reader_id).await;

        enqueue(&pool, reader_id).await;

        let batch = create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await;

        assert!(batch.is_ok());

        let recovered =
            match recover_interrupted_offline_queue(&pool, reader_id, TEST_TIME + 500).await {
                Ok(value) => value,
                Err(error) => {
                    pool.close().await;
                    remove_database(&path);
                    panic!("queue recovery failed: {error}")
                }
            };

        assert_eq!(recovered, 0);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue[0].queue_state(), OfflineQueueState::InFlight);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn empty_queue_cannot_create_batch() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_database("empty", reader_id).await;

        let result = create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            10,
        )
        .await;

        assert!(matches!(
            result,
            Err(
                ReaderSynchronizationError::
                    NoEligibleTransactions {
                        reader_id:
                            failed_reader,
                    }
            ) if failed_reader == reader_id
        ));

        pool.close().await;
        remove_database(&path);
    }
}
