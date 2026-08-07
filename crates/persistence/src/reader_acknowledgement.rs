use std::collections::HashSet;

use serde_json::json;
use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_device_protocol::DeviceProtocolVersion;
use transitguard_domain::{
    FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};

use crate::{ReaderSynchronizationError, SynchronizationBatch, load_synchronization_batch};

/// Backend resolution for one transaction inside a synchronization batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynchronizationEntryResolution {
    /// The backend accepted and durably processed the transaction.
    Acknowledged,

    /// The transaction can be submitted again after the specified time.
    RetryableFailure {
        /// Stable sanitized failure category.
        failure_category: String,

        /// Earliest permitted retry time.
        retry_at_unix_milliseconds: i64,
    },

    /// The backend returned a final non-retryable rejection.
    PermanentFailure {
        /// Stable sanitized failure category.
        failure_category: String,
    },

    /// Automated processing cannot safely resolve the transaction.
    ManualReview {
        /// Stable sanitized failure category.
        failure_category: String,
    },
}

/// One ordered transaction result in a synchronization acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynchronizationAcknowledgementEntry {
    transaction_id: FareTransactionId,
    local_sequence_number: LocalSequenceNumber,
    resolution: SynchronizationEntryResolution,
}

impl SynchronizationAcknowledgementEntry {
    /// Creates one acknowledgement entry.
    #[must_use]
    pub fn new(
        transaction_id: FareTransactionId,
        local_sequence_number: LocalSequenceNumber,
        resolution: SynchronizationEntryResolution,
    ) -> Self {
        Self {
            transaction_id,
            local_sequence_number,
            resolution,
        }
    }

    /// Returns the stable transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader-local sequence.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the backend resolution.
    #[must_use]
    pub const fn resolution(&self) -> &SynchronizationEntryResolution {
        &self.resolution
    }
}

/// One complete backend acknowledgement for a durable reader batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynchronizationAcknowledgement {
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    protocol_version: DeviceProtocolVersion,
    first_local_sequence_number: LocalSequenceNumber,
    last_local_sequence_number: LocalSequenceNumber,
    received_at_unix_milliseconds: i64,
    entries: Vec<SynchronizationAcknowledgementEntry>,
}

impl SynchronizationAcknowledgement {
    /// Creates and validates a synchronization acknowledgement.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reader_id: ReaderId,
        batch_id: SynchronizationBatchId,
        protocol_version: DeviceProtocolVersion,
        first_local_sequence_number: LocalSequenceNumber,
        last_local_sequence_number: LocalSequenceNumber,
        received_at_unix_milliseconds: i64,
        entries: Vec<SynchronizationAcknowledgementEntry>,
    ) -> Result<Self, ReaderAcknowledgementError> {
        if received_at_unix_milliseconds < 0 {
            return Err(ReaderAcknowledgementError::NegativeReceivedTime {
                unix_milliseconds: received_at_unix_milliseconds,
            });
        }

        if last_local_sequence_number < first_local_sequence_number {
            return Err(ReaderAcknowledgementError::InvalidSequenceRange);
        }

        if entries.is_empty() {
            return Err(ReaderAcknowledgementError::EmptyEntries);
        }

        let mut transaction_ids = HashSet::with_capacity(entries.len());

        let mut sequences = HashSet::with_capacity(entries.len());

        for entry in &entries {
            let transaction_id_text = entry.transaction_id().to_string();

            if !transaction_ids.insert(transaction_id_text) {
                return Err(ReaderAcknowledgementError::DuplicateTransaction {
                    transaction_id: entry.transaction_id(),
                });
            }

            let sequence = entry.local_sequence_number().value();

            if !sequences.insert(sequence) {
                return Err(ReaderAcknowledgementError::DuplicateSequence {
                    local_sequence_number: entry.local_sequence_number().value(),
                });
            }

            validate_resolution(
                entry.transaction_id(),
                entry.resolution(),
                received_at_unix_milliseconds,
            )?;
        }

        Ok(Self {
            reader_id,
            batch_id,
            protocol_version,
            first_local_sequence_number,
            last_local_sequence_number,
            received_at_unix_milliseconds,
            entries,
        })
    }

    /// Returns the reader receiving the acknowledgement.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the stable batch identity.
    #[must_use]
    pub const fn batch_id(&self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns the acknowledged protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the first sequence declared by the backend.
    #[must_use]
    pub const fn first_local_sequence_number(&self) -> LocalSequenceNumber {
        self.first_local_sequence_number
    }

    /// Returns the last sequence declared by the backend.
    #[must_use]
    pub const fn last_local_sequence_number(&self) -> LocalSequenceNumber {
        self.last_local_sequence_number
    }

    /// Returns when the acknowledgement reached the reader.
    #[must_use]
    pub const fn received_at_unix_milliseconds(&self) -> i64 {
        self.received_at_unix_milliseconds
    }

    /// Returns the ordered acknowledgement entries.
    #[must_use]
    pub fn entries(&self) -> &[SynchronizationAcknowledgementEntry] {
        &self.entries
    }
}

/// Result of durably storing one acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSynchronizationAcknowledgement {
    acknowledgement: SynchronizationAcknowledgement,
    replayed: bool,
}

impl StoredSynchronizationAcknowledgement {
    /// Returns the validated acknowledgement.
    #[must_use]
    pub const fn acknowledgement(&self) -> &SynchronizationAcknowledgement {
        &self.acknowledgement
    }

    /// Returns whether an identical durable acknowledgement already existed.
    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

/// Stable acknowledgement intake failures.
#[derive(Debug, Error)]
pub enum ReaderAcknowledgementError {
    /// Receipt times cannot predate the Unix epoch.
    #[error("synchronization acknowledgement time cannot be negative: {unix_milliseconds}")]
    NegativeReceivedTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// The declared sequence range is reversed.
    #[error("synchronization acknowledgement sequence range is invalid")]
    InvalidSequenceRange,

    /// An acknowledgement must contain at least one entry.
    #[error("synchronization acknowledgement must contain entries")]
    EmptyEntries,

    /// One transaction appeared more than once.
    #[error("synchronization acknowledgement contains duplicate transaction {transaction_id}")]
    DuplicateTransaction {
        /// Duplicated transaction identity.
        transaction_id: FareTransactionId,
    },

    /// One local sequence appeared more than once.
    #[error(
        "synchronization acknowledgement contains duplicate local sequence {local_sequence_number}"
    )]
    DuplicateSequence {
        /// Duplicated local sequence.
        local_sequence_number: u64,
    },

    /// A failure outcome used an empty category.
    #[error(
        "synchronization acknowledgement failure category is empty for transaction {transaction_id}"
    )]
    EmptyFailureCategory {
        /// Transaction with invalid failure metadata.
        transaction_id: FareTransactionId,
    },

    /// A retry was scheduled before acknowledgement receipt.
    #[error(
        "transaction {transaction_id} retry time {retry_at_unix_milliseconds} precedes acknowledgement time {received_at_unix_milliseconds}"
    )]
    RetryBeforeAcknowledgement {
        /// Transaction with invalid retry scheduling.
        transaction_id: FareTransactionId,

        /// Invalid retry time.
        retry_at_unix_milliseconds: i64,

        /// Acknowledgement receipt time.
        received_at_unix_milliseconds: i64,
    },

    /// The acknowledgement protocol did not match the durable batch.
    #[error("synchronization acknowledgement protocol mismatch for batch {batch_id}")]
    ProtocolMismatch {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// The acknowledgement range did not match the durable batch.
    #[error("synchronization acknowledgement sequence range mismatch for batch {batch_id}")]
    SequenceRangeMismatch {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// The acknowledgement did not contain the expected number of entries.
    #[error(
        "synchronization acknowledgement for batch {batch_id} expected {expected} entries but received {received}"
    )]
    EntryCountMismatch {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Number of durable batch entries.
        expected: usize,

        /// Number of acknowledgement entries.
        received: usize,
    },

    /// One acknowledgement entry did not match its durable batch position.
    #[error("synchronization acknowledgement entry {position} does not match batch {batch_id}")]
    EntryMismatch {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Zero-based mismatched entry position.
        position: usize,
    },

    /// A new acknowledgement requires an in-flight batch.
    #[error("synchronization batch {batch_id} is not awaiting acknowledgement")]
    BatchNotInFlight {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// A different acknowledgement was already stored for the batch.
    #[error("synchronization batch {batch_id} already has a conflicting acknowledgement")]
    ConflictingReplay {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// Canonical acknowledgement serialization failed.
    #[error("synchronization acknowledgement serialization failed")]
    Serialization {
        /// Original JSON failure.
        #[source]
        source: serde_json::Error,
    },

    /// Loading or decoding the durable batch failed.
    #[error(transparent)]
    Synchronization(#[from] ReaderSynchronizationError),

    /// A named SQLite acknowledgement operation failed.
    #[error("reader SQLite acknowledgement operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderAcknowledgementError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }
}

/// Validates and durably stores one backend acknowledgement.
///
/// An identical replay returns success without inserting duplicate rows.
/// A different acknowledgement for the same batch is rejected.
pub async fn store_synchronization_acknowledgement(
    pool: &SqlitePool,
    acknowledgement: &SynchronizationAcknowledgement,
) -> Result<StoredSynchronizationAcknowledgement, ReaderAcknowledgementError> {
    let batch = load_synchronization_batch(
        pool,
        acknowledgement.reader_id(),
        acknowledgement.batch_id(),
    )
    .await?;

    validate_against_batch(acknowledgement, &batch)?;

    let payload_json = canonical_payload(acknowledgement)?;

    let mut transaction = pool.begin().await.map_err(|source| {
        ReaderAcknowledgementError::database("begin acknowledgement storage", source)
    })?;

    let existing_payload = sqlx::query_scalar::<_, String>(
        r#"
            SELECT payload_json
            FROM synchronization_acknowledgements
            WHERE
                batch_id = ?
                AND reader_id = ?
            "#,
    )
    .bind(acknowledgement.batch_id().to_string())
    .bind(acknowledgement.reader_id().to_string())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementError::database("load existing acknowledgement", source)
    })?;

    if let Some(existing_payload) = existing_payload {
        if existing_payload != payload_json {
            return Err(ReaderAcknowledgementError::ConflictingReplay {
                batch_id: acknowledgement.batch_id(),
            });
        }

        drop(transaction);

        return Ok(StoredSynchronizationAcknowledgement {
            acknowledgement: acknowledgement.clone(),
            replayed: true,
        });
    }

    let current_batch_state = sqlx::query_scalar::<_, String>(
        r#"
            SELECT batch_state
            FROM synchronization_batches
            WHERE
                batch_id = ?
                AND reader_id = ?
            "#,
    )
    .bind(acknowledgement.batch_id().to_string())
    .bind(acknowledgement.reader_id().to_string())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementError::database("load acknowledgement batch state", source)
    })?;

    if current_batch_state.as_deref() != Some("in_flight") {
        return Err(ReaderAcknowledgementError::BatchNotInFlight {
            batch_id: acknowledgement.batch_id(),
        });
    }

    sqlx::query(
        r#"
        INSERT INTO synchronization_acknowledgements (
            batch_id,
            reader_id,
            protocol_version,
            first_local_sequence_number,
            last_local_sequence_number,
            received_at_unix_milliseconds,
            payload_json,
            applied_at_unix_milliseconds
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
        "#,
    )
    .bind(acknowledgement.batch_id().to_string())
    .bind(acknowledgement.reader_id().to_string())
    .bind(i64::from(acknowledgement.protocol_version().value()))
    .bind(sequence_to_i64(
        acknowledgement.first_local_sequence_number(),
    )?)
    .bind(sequence_to_i64(
        acknowledgement.last_local_sequence_number(),
    )?)
    .bind(acknowledgement.received_at_unix_milliseconds())
    .bind(payload_json)
    .execute(&mut *transaction)
    .await
    .map_err(|source| ReaderAcknowledgementError::database("insert acknowledgement", source))?;

    for (position, entry) in acknowledgement.entries().iter().enumerate() {
        let position =
            i64::try_from(position).map_err(|_| ReaderAcknowledgementError::EntryMismatch {
                batch_id: acknowledgement.batch_id(),
                position,
            })?;

        let (outcome, failure_category, retry_at_unix_milliseconds) =
            stored_resolution(entry.resolution());

        sqlx::query(
            r#"
            INSERT INTO synchronization_acknowledgement_entries (
                batch_id,
                reader_id,
                fare_transaction_id,
                local_sequence_number,
                entry_position,
                outcome,
                failure_category,
                retry_at_unix_milliseconds
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(acknowledgement.batch_id().to_string())
        .bind(acknowledgement.reader_id().to_string())
        .bind(entry.transaction_id().to_string())
        .bind(sequence_to_i64(entry.local_sequence_number())?)
        .bind(position)
        .bind(outcome)
        .bind(failure_category)
        .bind(retry_at_unix_milliseconds)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            ReaderAcknowledgementError::database("insert acknowledgement entry", source)
        })?;
    }

    transaction.commit().await.map_err(|source| {
        ReaderAcknowledgementError::database("commit acknowledgement storage", source)
    })?;

    Ok(StoredSynchronizationAcknowledgement {
        acknowledgement: acknowledgement.clone(),
        replayed: false,
    })
}

fn validate_against_batch(
    acknowledgement: &SynchronizationAcknowledgement,
    batch: &SynchronizationBatch,
) -> Result<(), ReaderAcknowledgementError> {
    if acknowledgement.protocol_version() != batch.protocol_version() {
        return Err(ReaderAcknowledgementError::ProtocolMismatch {
            batch_id: batch.batch_id(),
        });
    }

    if acknowledgement.first_local_sequence_number() != batch.first_local_sequence_number()
        || acknowledgement.last_local_sequence_number() != batch.last_local_sequence_number()
    {
        return Err(ReaderAcknowledgementError::SequenceRangeMismatch {
            batch_id: batch.batch_id(),
        });
    }

    if acknowledgement.entries().len() != batch.entries().len() {
        return Err(ReaderAcknowledgementError::EntryCountMismatch {
            batch_id: batch.batch_id(),
            expected: batch.entries().len(),
            received: acknowledgement.entries().len(),
        });
    }

    for (position, (expected, received)) in batch
        .entries()
        .iter()
        .zip(acknowledgement.entries())
        .enumerate()
    {
        if expected.transaction_id() != received.transaction_id()
            || expected.local_sequence_number() != received.local_sequence_number()
        {
            return Err(ReaderAcknowledgementError::EntryMismatch {
                batch_id: batch.batch_id(),
                position,
            });
        }
    }

    Ok(())
}

fn validate_resolution(
    transaction_id: FareTransactionId,
    resolution: &SynchronizationEntryResolution,
    received_at_unix_milliseconds: i64,
) -> Result<(), ReaderAcknowledgementError> {
    match resolution {
        SynchronizationEntryResolution::Acknowledged => Ok(()),

        SynchronizationEntryResolution::RetryableFailure {
            failure_category,
            retry_at_unix_milliseconds,
        } => {
            validate_failure_category(transaction_id, failure_category)?;

            if *retry_at_unix_milliseconds < received_at_unix_milliseconds {
                return Err(ReaderAcknowledgementError::RetryBeforeAcknowledgement {
                    transaction_id,
                    retry_at_unix_milliseconds: *retry_at_unix_milliseconds,
                    received_at_unix_milliseconds,
                });
            }

            Ok(())
        }

        SynchronizationEntryResolution::PermanentFailure { failure_category }
        | SynchronizationEntryResolution::ManualReview { failure_category } => {
            validate_failure_category(transaction_id, failure_category)
        }
    }
}

fn validate_failure_category(
    transaction_id: FareTransactionId,
    failure_category: &str,
) -> Result<(), ReaderAcknowledgementError> {
    if failure_category.trim().is_empty() {
        return Err(ReaderAcknowledgementError::EmptyFailureCategory { transaction_id });
    }

    Ok(())
}

fn canonical_payload(
    acknowledgement: &SynchronizationAcknowledgement,
) -> Result<String, ReaderAcknowledgementError> {
    let entries = acknowledgement
        .entries()
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let (outcome, failure_category, retry_at_unix_milliseconds) =
                stored_resolution(entry.resolution());

            json!({
                "entry_position": position,
                "fare_transaction_id":
                    entry.transaction_id().to_string(),
                "local_sequence_number":
                    entry.local_sequence_number().value(),
                "outcome": outcome,
                "failure_category": failure_category,
                "retry_at_unix_milliseconds":
                    retry_at_unix_milliseconds,
            })
        })
        .collect::<Vec<_>>();

    serde_json::to_string(&json!({
        "schema_version": 1,
        "reader_id":
            acknowledgement.reader_id().to_string(),
        "batch_id":
            acknowledgement.batch_id().to_string(),
        "protocol_version":
            acknowledgement.protocol_version().value(),
        "first_local_sequence_number":
            acknowledgement
                .first_local_sequence_number()
                .value(),
        "last_local_sequence_number":
            acknowledgement
                .last_local_sequence_number()
                .value(),
        "received_at_unix_milliseconds":
            acknowledgement
                .received_at_unix_milliseconds(),
        "entries": entries,
    }))
    .map_err(|source| ReaderAcknowledgementError::Serialization { source })
}

fn stored_resolution(
    resolution: &SynchronizationEntryResolution,
) -> (&'static str, Option<&str>, Option<i64>) {
    match resolution {
        SynchronizationEntryResolution::Acknowledged => ("acknowledged", None, None),

        SynchronizationEntryResolution::RetryableFailure {
            failure_category,
            retry_at_unix_milliseconds,
        } => (
            "retryable_failure",
            Some(failure_category.as_str()),
            Some(*retry_at_unix_milliseconds),
        ),

        SynchronizationEntryResolution::PermanentFailure { failure_category } => {
            ("permanent_failure", Some(failure_category.as_str()), None)
        }

        SynchronizationEntryResolution::ManualReview { failure_category } => {
            ("manual_review", Some(failure_category.as_str()), None)
        }
    }
}

fn sequence_to_i64(sequence: LocalSequenceNumber) -> Result<i64, ReaderAcknowledgementError> {
    i64::try_from(sequence.value()).map_err(|_| ReaderAcknowledgementError::InvalidSequenceRange)
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
        OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig, SynchronizationBatch,
        apply_synchronization_acknowledgement, bind_reader_database, connect_reader_sqlite,
        create_synchronization_batch, enqueue_offline_transaction,
        mark_synchronization_batch_in_flight, run_reader_sqlite_migrations,
    };

    use super::{
        ReaderAcknowledgementError, SynchronizationAcknowledgement,
        SynchronizationAcknowledgementEntry, SynchronizationEntryResolution,
        store_synchronization_acknowledgement,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-ack-{name}-{}.sqlite3",
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

    async fn submitted_batch(
        pool: &SqlitePool,
        reader_id: ReaderId,
        entry_count: usize,
    ) -> SynchronizationBatch {
        for _ in 0..entry_count {
            if let Err(error) = enqueue_offline_transaction(pool, reader_id, &draft()).await {
                panic!("queue insertion failed: {error}");
            }
        }

        let prepared = match create_synchronization_batch(
            pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            entry_count,
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
            prepared.batch_id(),
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

    fn acknowledgement(
        batch: &SynchronizationBatch,
        resolutions: Vec<SynchronizationEntryResolution>,
    ) -> SynchronizationAcknowledgement {
        let entries = batch
            .entries()
            .iter()
            .zip(resolutions)
            .map(|(entry, resolution)| {
                SynchronizationAcknowledgementEntry::new(
                    entry.transaction_id(),
                    entry.local_sequence_number(),
                    resolution,
                )
            })
            .collect();

        match SynchronizationAcknowledgement::new(
            batch.reader_id(),
            batch.batch_id(),
            batch.protocol_version(),
            batch.first_local_sequence_number(),
            batch.last_local_sequence_number(),
            TEST_TIME + 400,
            entries,
        ) {
            Ok(value) => value,

            Err(error) => {
                panic!("acknowledgement creation failed: {error}")
            }
        }
    }

    #[tokio::test]
    async fn acknowledgement_is_stored_durably() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("durable", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 2).await;

        let value = acknowledgement(
            &batch,
            vec![
                SynchronizationEntryResolution::Acknowledged,
                SynchronizationEntryResolution::RetryableFailure {
                    failure_category: String::from("backend_timeout"),
                    retry_at_unix_milliseconds: TEST_TIME + 1_000,
                },
            ],
        );

        let stored = match store_synchronization_acknowledgement(&pool, &value).await {
            Ok(result) => result,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement storage failed: {error}")
            }
        };

        assert!(!stored.replayed());

        let acknowledgement_count = match sqlx::query_scalar::<_, i64>(
            r#"
                SELECT COUNT(*)
                FROM synchronization_acknowledgements
                "#,
        )
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement count failed: {error}")
            }
        };

        let entry_count = match sqlx::query_scalar::<_, i64>(
            r#"
                SELECT COUNT(*)
                FROM synchronization_acknowledgement_entries
                "#,
        )
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement entry count failed: {error}")
            }
        };

        assert_eq!(acknowledgement_count, 1);
        assert_eq!(entry_count, 2);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn identical_acknowledgement_replay_is_idempotent() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("idempotent", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1).await;

        let value = acknowledgement(&batch, vec![SynchronizationEntryResolution::Acknowledged]);

        let first = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(matches!(
            first,
            Ok(ref stored) if !stored.replayed()
        ));

        let second = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(matches!(
            second,
            Ok(ref stored) if stored.replayed()
        ));

        let count = match sqlx::query_scalar::<_, i64>(
            r#"
                SELECT COUNT(*)
                FROM synchronization_acknowledgements
                "#,
        )
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement count failed: {error}")
            }
        };

        assert_eq!(count, 1);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn acknowledgement_replays_remain_idempotent_after_application() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("replay-after-application", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1).await;

        let value = acknowledgement(&batch, vec![SynchronizationEntryResolution::Acknowledged]);

        let first = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(matches!(
            first,
            Ok(ref stored) if !stored.replayed()
        ));

        let applied = apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 500,
        )
        .await;

        assert!(matches!(
            applied,
            Ok(report) if report.applied_now()
        ));

        let replay = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(matches!(
            replay,
            Ok(ref stored) if stored.replayed()
        ));

        let conflicting = acknowledgement(
            &batch,
            vec![SynchronizationEntryResolution::PermanentFailure {
                failure_category: String::from("invalid_envelope"),
            }],
        );

        let conflict = store_synchronization_acknowledgement(&pool, &conflicting).await;

        assert!(matches!(
            conflict,
            Err(
                ReaderAcknowledgementError::ConflictingReplay {
                    batch_id,
                }
            ) if batch_id == batch.batch_id()
        ));

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn conflicting_acknowledgement_replay_is_rejected() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("conflict", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1).await;

        let accepted = acknowledgement(&batch, vec![SynchronizationEntryResolution::Acknowledged]);

        let first = store_synchronization_acknowledgement(&pool, &accepted).await;

        assert!(first.is_ok());

        let conflicting = acknowledgement(
            &batch,
            vec![SynchronizationEntryResolution::PermanentFailure {
                failure_category: String::from("invalid_envelope"),
            }],
        );

        let result = store_synchronization_acknowledgement(&pool, &conflicting).await;

        assert!(matches!(
            result,
            Err(
                ReaderAcknowledgementError::
                    ConflictingReplay {
                        batch_id,
                    }
            ) if batch_id == batch.batch_id()
        ));

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn acknowledgement_constraints_require_failure_metadata() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("failure-metadata-constraints", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 2).await;

        let value = acknowledgement(
            &batch,
            vec![
                SynchronizationEntryResolution::RetryableFailure {
                    failure_category: String::from("backend_timeout"),
                    retry_at_unix_milliseconds: TEST_TIME + 1_000,
                },
                SynchronizationEntryResolution::PermanentFailure {
                    failure_category: String::from("invalid_envelope"),
                },
            ],
        );

        let stored = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(stored.is_ok());

        let retryable_without_category = sqlx::query(
            r#"
            UPDATE synchronization_acknowledgement_entries
            SET failure_category = NULL
            WHERE
                batch_id = ?
                AND entry_position = 0
            "#,
        )
        .bind(batch.batch_id().to_string())
        .execute(&pool)
        .await;

        assert!(retryable_without_category.is_err());

        let retryable_without_time = sqlx::query(
            r#"
            UPDATE synchronization_acknowledgement_entries
            SET retry_at_unix_milliseconds = NULL
            WHERE
                batch_id = ?
                AND entry_position = 0
            "#,
        )
        .bind(batch.batch_id().to_string())
        .execute(&pool)
        .await;

        assert!(retryable_without_time.is_err());

        let permanent_without_category = sqlx::query(
            r#"
            UPDATE synchronization_acknowledgement_entries
            SET failure_category = NULL
            WHERE
                batch_id = ?
                AND entry_position = 1
            "#,
        )
        .bind(batch.batch_id().to_string())
        .execute(&pool)
        .await;

        assert!(permanent_without_category.is_err());

        let permanent_with_retry_time = sqlx::query(
            r#"
            UPDATE synchronization_acknowledgement_entries
            SET retry_at_unix_milliseconds = ?
            WHERE
                batch_id = ?
                AND entry_position = 1
            "#,
        )
        .bind(TEST_TIME + 2_000)
        .bind(batch.batch_id().to_string())
        .execute(&pool)
        .await;

        assert!(permanent_with_retry_time.is_err());

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn protocol_mismatch_is_rejected_before_storage() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("protocol-mismatch", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1).await;

        let entries = vec![SynchronizationAcknowledgementEntry::new(
            batch.entries()[0].transaction_id(),
            batch.entries()[0].local_sequence_number(),
            SynchronizationEntryResolution::Acknowledged,
        )];

        let other_protocol = match DeviceProtocolVersion::new(2) {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("protocol construction failed: {error}")
            }
        };

        let value = match SynchronizationAcknowledgement::new(
            reader_id,
            batch.batch_id(),
            other_protocol,
            batch.first_local_sequence_number(),
            batch.last_local_sequence_number(),
            TEST_TIME + 400,
            entries,
        ) {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement creation failed: {error}")
            }
        };

        let result = store_synchronization_acknowledgement(&pool, &value).await;

        assert!(matches!(
            result,
            Err(
                ReaderAcknowledgementError::
                    ProtocolMismatch {
                        batch_id,
                    }
            ) if batch_id == batch.batch_id()
        ));

        let count = match sqlx::query_scalar::<_, i64>(
            r#"
                SELECT COUNT(*)
                FROM synchronization_acknowledgements
                "#,
        )
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement count failed: {error}")
            }
        };

        assert_eq!(count, 0);

        pool.close().await;
        remove_database(&path);
    }
}
