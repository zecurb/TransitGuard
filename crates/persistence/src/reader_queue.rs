use sqlx::{SqlitePool, error::ErrorKind};
use thiserror::Error;
use transitguard_domain::{
    EventTime, FareCredentialId, FareDecision, FarePolicyVersion, FareTransactionId,
    LocalSequenceNumber, ReaderId,
};

/// Durable lifecycle state of one offline transaction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OfflineQueueState {
    /// The transaction is eligible for synchronization.
    Pending,

    /// The transaction belongs to a durable synchronization attempt.
    InFlight,

    /// The backend returned a final successful resolution.
    Acknowledged,

    /// The transaction may be attempted again.
    RetryableFailure,

    /// The backend returned a final rejection.
    PermanentFailure,

    /// Automated processing cannot safely resolve the transaction.
    ManualReview,
}

impl OfflineQueueState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "in_flight",
            Self::Acknowledged => "acknowledged",
            Self::RetryableFailure => "retryable_failure",
            Self::PermanentFailure => "permanent_failure",
            Self::ManualReview => "manual_review",
        }
    }

    fn parse(value: &str) -> Result<Self, ReaderQueueError> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_flight" => Ok(Self::InFlight),
            "acknowledged" => Ok(Self::Acknowledged),
            "retryable_failure" => Ok(Self::RetryableFailure),
            "permanent_failure" => Ok(Self::PermanentFailure),
            "manual_review" => Ok(Self::ManualReview),
            _ => Err(ReaderQueueError::invalid_stored_value("queue_state")),
        }
    }
}

/// Data required to durably create an offline transaction.
///
/// The reader-local sequence is intentionally absent. SQLite assigns
/// that value atomically with queue insertion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineTransactionDraft {
    transaction_id: FareTransactionId,
    fare_credential_id: FareCredentialId,
    event_time: EventTime,
    fare_policy_version: FarePolicyVersion,
    provisional_decision: FareDecision,
    transaction_envelope: serde_json::Value,
    created_at_unix_milliseconds: i64,
}

impl OfflineTransactionDraft {
    /// Creates validated offline transaction input.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction_id: FareTransactionId,
        fare_credential_id: FareCredentialId,
        event_time: EventTime,
        fare_policy_version: FarePolicyVersion,
        provisional_decision: FareDecision,
        transaction_envelope: serde_json::Value,
        created_at_unix_milliseconds: i64,
    ) -> Result<Self, ReaderQueueError> {
        if created_at_unix_milliseconds < 0 {
            return Err(ReaderQueueError::NegativePersistenceTime {
                unix_milliseconds: created_at_unix_milliseconds,
            });
        }

        Ok(Self {
            transaction_id,
            fare_credential_id,
            event_time,
            fare_policy_version,
            provisional_decision,
            transaction_envelope,
            created_at_unix_milliseconds,
        })
    }

    /// Returns the stable fare-transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the presented credential identity.
    #[must_use]
    pub const fn fare_credential_id(&self) -> FareCredentialId {
        self.fare_credential_id
    }

    /// Returns the reader-reported event time.
    #[must_use]
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the fare-policy version.
    #[must_use]
    pub const fn fare_policy_version(&self) -> FarePolicyVersion {
        self.fare_policy_version
    }

    /// Returns the provisional offline decision.
    #[must_use]
    pub const fn provisional_decision(&self) -> FareDecision {
        self.provisional_decision
    }

    /// Returns the project-owned transaction envelope.
    #[must_use]
    pub const fn transaction_envelope(&self) -> &serde_json::Value {
        &self.transaction_envelope
    }

    /// Returns when the durable record was created.
    #[must_use]
    pub const fn created_at_unix_milliseconds(&self) -> i64 {
        self.created_at_unix_milliseconds
    }
}

/// One transaction loaded from the durable offline queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedOfflineTransaction {
    transaction_id: FareTransactionId,
    reader_id: ReaderId,
    local_sequence_number: LocalSequenceNumber,
    fare_credential_id: FareCredentialId,
    event_time: EventTime,
    fare_policy_version: FarePolicyVersion,
    provisional_decision: FareDecision,
    transaction_envelope: serde_json::Value,
    queue_state: OfflineQueueState,
    attempt_count: u32,
    next_retry_at_unix_milliseconds: Option<i64>,
    last_failure_category: Option<String>,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
}

impl QueuedOfflineTransaction {
    /// Returns the stable transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader that owns the transaction.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the assigned reader-local sequence.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the presented credential identity.
    #[must_use]
    pub const fn fare_credential_id(&self) -> FareCredentialId {
        self.fare_credential_id
    }

    /// Returns the reader-reported event time.
    #[must_use]
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the policy version used by the reader.
    #[must_use]
    pub const fn fare_policy_version(&self) -> FarePolicyVersion {
        self.fare_policy_version
    }

    /// Returns the provisional decision.
    #[must_use]
    pub const fn provisional_decision(&self) -> FareDecision {
        self.provisional_decision
    }

    /// Returns the serialized project-owned envelope.
    #[must_use]
    pub const fn transaction_envelope(&self) -> &serde_json::Value {
        &self.transaction_envelope
    }

    /// Returns the durable queue state.
    #[must_use]
    pub const fn queue_state(&self) -> OfflineQueueState {
        self.queue_state
    }

    /// Returns the synchronization attempt count.
    #[must_use]
    pub const fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    /// Returns the next permitted retry time.
    #[must_use]
    pub const fn next_retry_at_unix_milliseconds(&self) -> Option<i64> {
        self.next_retry_at_unix_milliseconds
    }

    /// Returns the last sanitized failure category.
    #[must_use]
    pub fn last_failure_category(&self) -> Option<&str> {
        self.last_failure_category.as_deref()
    }

    /// Returns the record creation time.
    #[must_use]
    pub const fn created_at_unix_milliseconds(&self) -> i64 {
        self.created_at_unix_milliseconds
    }

    /// Returns the record update time.
    #[must_use]
    pub const fn updated_at_unix_milliseconds(&self) -> i64 {
        self.updated_at_unix_milliseconds
    }
}

/// Stable failures produced by reader queue operations.
#[derive(Debug, Error)]
pub enum ReaderQueueError {
    /// Persistence timestamps cannot predate the Unix epoch.
    #[error("reader queue persistence time cannot be negative: {unix_milliseconds}")]
    NegativePersistenceTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// The database has not been bound to the expected reader.
    #[error("reader {reader_id} is not bound to this SQLite database or its sequence is exhausted")]
    SequenceUnavailable {
        /// Reader requesting sequence assignment.
        reader_id: ReaderId,
    },

    /// A durable identifier or sequence already exists.
    #[error("reader SQLite queue rejected a duplicate transaction or sequence")]
    WriteConflict {
        /// Original SQLx database failure.
        #[source]
        source: sqlx::Error,
    },

    /// JSON serialization failed.
    #[error("reader queue serialization failed for `{field}`")]
    Serialization {
        /// Stable serialized field name.
        field: &'static str,

        /// Original JSON failure.
        #[source]
        source: serde_json::Error,
    },

    /// SQLite contained invalid durable queue data.
    #[error("reader SQLite queue contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A named queue operation failed.
    #[error("reader SQLite queue operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderQueueError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        let is_unique_violation = matches!(
            &source,
            sqlx::Error::Database(database_error)
                if database_error.kind()
                    == ErrorKind::UniqueViolation
        );

        if is_unique_violation {
            return Self::WriteConflict { source };
        }

        Self::Database { operation, source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }

    const fn serialization(field: &'static str, source: serde_json::Error) -> Self {
        Self::Serialization { field, source }
    }
}

#[derive(sqlx::FromRow)]
struct StoredOfflineTransaction {
    fare_transaction_id: String,
    reader_id: String,
    local_sequence_number: i64,
    fare_credential_id: String,
    event_time_unix_milliseconds: i64,
    fare_policy_version: i64,
    provisional_decision_json: String,
    transaction_envelope_json: String,
    queue_state: String,
    attempt_count: i64,
    next_retry_at_unix_milliseconds: Option<i64>,
    last_failure_category: Option<String>,
    created_at_unix_milliseconds: i64,
    updated_at_unix_milliseconds: i64,
}

/// Atomically assigns a sequence and inserts an offline transaction.
///
/// The reader-state update and queue insert occur in one SQLite
/// transaction. An insertion failure rolls back sequence advancement.
pub async fn enqueue_offline_transaction(
    pool: &SqlitePool,
    reader_id: ReaderId,
    draft: &OfflineTransactionDraft,
) -> Result<QueuedOfflineTransaction, ReaderQueueError> {
    let decision_json = serde_json::to_string(&draft.provisional_decision)
        .map_err(|source| ReaderQueueError::serialization("provisional_decision", source))?;

    let envelope_json = serde_json::to_string(&draft.transaction_envelope)
        .map_err(|source| ReaderQueueError::serialization("transaction_envelope", source))?;

    let mut transaction = pool
        .begin()
        .await
        .map_err(|source| ReaderQueueError::database("begin queue insertion", source))?;

    let assigned_sequence = sqlx::query_scalar::<_, i64>(
        r#"
            UPDATE reader_state
            SET
                next_local_sequence =
                    next_local_sequence + 1,
                updated_at_unix_milliseconds = ?
            WHERE
                singleton = 1
                AND reader_id = ?
                AND next_local_sequence
                    < 9223372036854775807
            RETURNING next_local_sequence - 1
            "#,
    )
    .bind(draft.created_at_unix_milliseconds())
    .bind(reader_id.to_string())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| ReaderQueueError::database("assign local sequence", source))?
    .ok_or(ReaderQueueError::SequenceUnavailable { reader_id })?;

    let sequence_value = u64::try_from(assigned_sequence)
        .map_err(|_| ReaderQueueError::invalid_stored_value("local_sequence_number"))?;

    let local_sequence_number = LocalSequenceNumber::new(sequence_value)
        .map_err(|_| ReaderQueueError::invalid_stored_value("local_sequence_number"))?;

    sqlx::query(
        r#"
        INSERT INTO offline_transactions (
            fare_transaction_id,
            reader_id,
            local_sequence_number,
            fare_credential_id,
            event_time_unix_milliseconds,
            fare_policy_version,
            processing_mode,
            provisional_decision_json,
            transaction_envelope_json,
            queue_state,
            attempt_count,
            next_retry_at_unix_milliseconds,
            last_failure_category,
            created_at_unix_milliseconds,
            updated_at_unix_milliseconds
        )
        VALUES (
            ?, ?, ?, ?, ?, ?, 'offline',
            ?, ?, ?, 0, NULL, NULL, ?, ?
        )
        "#,
    )
    .bind(draft.transaction_id().to_string())
    .bind(reader_id.to_string())
    .bind(assigned_sequence)
    .bind(draft.fare_credential_id().to_string())
    .bind(draft.event_time().unix_milliseconds())
    .bind(
        i64::try_from(draft.fare_policy_version().value())
            .map_err(|_| ReaderQueueError::invalid_stored_value("fare_policy_version"))?,
    )
    .bind(decision_json)
    .bind(envelope_json)
    .bind(OfflineQueueState::Pending.as_str())
    .bind(draft.created_at_unix_milliseconds())
    .bind(draft.created_at_unix_milliseconds())
    .execute(&mut *transaction)
    .await
    .map_err(|source| ReaderQueueError::database("insert offline transaction", source))?;

    transaction
        .commit()
        .await
        .map_err(|source| ReaderQueueError::database("commit queue insertion", source))?;

    Ok(QueuedOfflineTransaction {
        transaction_id: draft.transaction_id(),
        reader_id,
        local_sequence_number,
        fare_credential_id: draft.fare_credential_id(),
        event_time: draft.event_time(),
        fare_policy_version: draft.fare_policy_version(),
        provisional_decision: draft.provisional_decision(),
        transaction_envelope: draft.transaction_envelope().clone(),
        queue_state: OfflineQueueState::Pending,
        attempt_count: 0,
        next_retry_at_unix_milliseconds: None,
        last_failure_category: None,
        created_at_unix_milliseconds: draft.created_at_unix_milliseconds(),
        updated_at_unix_milliseconds: draft.created_at_unix_milliseconds(),
    })
}

/// Loads every durable offline transaction for one reader.
pub async fn load_offline_queue(
    pool: &SqlitePool,
    reader_id: ReaderId,
) -> Result<Vec<QueuedOfflineTransaction>, ReaderQueueError> {
    let records = sqlx::query_as::<_, StoredOfflineTransaction>(
        r#"
            SELECT
                fare_transaction_id,
                reader_id,
                local_sequence_number,
                fare_credential_id,
                event_time_unix_milliseconds,
                fare_policy_version,
                provisional_decision_json,
                transaction_envelope_json,
                queue_state,
                attempt_count,
                next_retry_at_unix_milliseconds,
                last_failure_category,
                created_at_unix_milliseconds,
                updated_at_unix_milliseconds
            FROM offline_transactions
            WHERE reader_id = ?
            ORDER BY local_sequence_number
            "#,
    )
    .bind(reader_id.to_string())
    .fetch_all(pool)
    .await
    .map_err(|source| ReaderQueueError::database("load offline queue", source))?;

    records.into_iter().map(decode_stored_transaction).collect()
}

fn decode_stored_transaction(
    stored: StoredOfflineTransaction,
) -> Result<QueuedOfflineTransaction, ReaderQueueError> {
    let transaction_id = stored
        .fare_transaction_id
        .parse::<FareTransactionId>()
        .map_err(|_| ReaderQueueError::invalid_stored_value("fare_transaction_id"))?;

    let reader_id = stored
        .reader_id
        .parse::<ReaderId>()
        .map_err(|_| ReaderQueueError::invalid_stored_value("reader_id"))?;

    let sequence_value = u64::try_from(stored.local_sequence_number)
        .map_err(|_| ReaderQueueError::invalid_stored_value("local_sequence_number"))?;

    let local_sequence_number = LocalSequenceNumber::new(sequence_value)
        .map_err(|_| ReaderQueueError::invalid_stored_value("local_sequence_number"))?;

    let fare_credential_id = stored
        .fare_credential_id
        .parse::<FareCredentialId>()
        .map_err(|_| ReaderQueueError::invalid_stored_value("fare_credential_id"))?;

    let event_time = EventTime::from_unix_milliseconds(stored.event_time_unix_milliseconds)
        .map_err(|_| ReaderQueueError::invalid_stored_value("event_time_unix_milliseconds"))?;

    let policy_value = u64::try_from(stored.fare_policy_version)
        .map_err(|_| ReaderQueueError::invalid_stored_value("fare_policy_version"))?;

    let fare_policy_version = FarePolicyVersion::new(policy_value)
        .map_err(|_| ReaderQueueError::invalid_stored_value("fare_policy_version"))?;

    let provisional_decision =
        serde_json::from_str::<FareDecision>(&stored.provisional_decision_json)
            .map_err(|source| ReaderQueueError::serialization("provisional_decision", source))?;

    let transaction_envelope =
        serde_json::from_str::<serde_json::Value>(&stored.transaction_envelope_json)
            .map_err(|source| ReaderQueueError::serialization("transaction_envelope", source))?;

    let queue_state = OfflineQueueState::parse(&stored.queue_state)?;

    let attempt_count = u32::try_from(stored.attempt_count)
        .map_err(|_| ReaderQueueError::invalid_stored_value("attempt_count"))?;

    if matches!(
        stored.next_retry_at_unix_milliseconds,
        Some(value) if value < 0
    ) {
        return Err(ReaderQueueError::invalid_stored_value(
            "next_retry_at_unix_milliseconds",
        ));
    }

    if stored.created_at_unix_milliseconds < 0 {
        return Err(ReaderQueueError::invalid_stored_value(
            "created_at_unix_milliseconds",
        ));
    }

    if stored.updated_at_unix_milliseconds < stored.created_at_unix_milliseconds {
        return Err(ReaderQueueError::invalid_stored_value(
            "updated_at_unix_milliseconds",
        ));
    }

    Ok(QueuedOfflineTransaction {
        transaction_id,
        reader_id,
        local_sequence_number,
        fare_credential_id,
        event_time,
        fare_policy_version,
        provisional_decision,
        transaction_envelope,
        queue_state,
        attempt_count,
        next_retry_at_unix_milliseconds: stored.next_retry_at_unix_milliseconds,
        last_failure_category: stored.last_failure_category,
        created_at_unix_milliseconds: stored.created_at_unix_milliseconds,
        updated_at_unix_milliseconds: stored.updated_at_unix_milliseconds,
    })
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
        ReaderDatabaseIdentity, ReaderSqliteConfig, bind_reader_database, connect_reader_sqlite,
        run_reader_sqlite_migrations,
    };

    use super::{
        OfflineQueueState, OfflineTransactionDraft, ReaderQueueError, enqueue_offline_transaction,
        load_offline_queue,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn temporary_database_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-queue-{test_name}-{}.sqlite3",
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
                panic!("valid fare decision failed: {error}")
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
                "kind": "offline_fare_transaction"
            }),
            TEST_TIME + 100,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid queue draft failed: {error}")
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
                panic!("valid SQLite configuration failed: {error}")
            }
        };

        let pool = match connect_reader_sqlite(&config).await {
            Ok(value) => value,
            Err(error) => {
                remove_database_files(&path);

                panic!("SQLite connection failed: {error}")
            }
        };

        if let Err(error) = run_reader_sqlite_migrations(&pool).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("SQLite migrations failed: {error}");
        }

        let identity = match ReaderDatabaseIdentity::new(
            reader_id,
            "development",
            "0.1.0",
            DeviceProtocolVersion::CURRENT,
            TEST_TIME,
        ) {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("reader identity failed: {error}")
            }
        };

        if let Err(error) = bind_reader_database(&pool, &identity).await {
            pool.close().await;
            remove_database_files(&path);

            panic!("reader binding failed: {error}");
        }

        (path, config, pool)
    }

    #[tokio::test]
    async fn insertion_assigns_monotonic_sequences() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("monotonic-sequences", reader_id).await;

        let first = match enqueue_offline_transaction(
            &pool,
            reader_id,
            &draft(FareTransactionId::generate()),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("first queue insertion failed: {error}")
            }
        };

        let second = match enqueue_offline_transaction(
            &pool,
            reader_id,
            &draft(FareTransactionId::generate()),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("second queue insertion failed: {error}")
            }
        };

        assert_eq!(first.local_sequence_number().value(), 1);
        assert_eq!(second.local_sequence_number().value(), 2);
        assert_eq!(first.queue_state(), OfflineQueueState::Pending);

        let next_sequence = match sqlx::query_scalar::<_, i64>(
            r#"
                SELECT next_local_sequence
                FROM reader_state
                WHERE singleton = 1
                "#,
        )
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("sequence query failed: {error}")
            }
        };

        assert_eq!(next_sequence, 3);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn duplicate_insert_rolls_back_sequence() {
        let reader_id = ReaderId::generate();

        let (path, _config, pool) = open_bound_database("duplicate-rollback", reader_id).await;

        let duplicate = draft(FareTransactionId::generate());

        let first = enqueue_offline_transaction(&pool, reader_id, &duplicate).await;

        assert!(first.is_ok());

        let conflict = enqueue_offline_transaction(&pool, reader_id, &duplicate).await;

        assert!(matches!(
            conflict,
            Err(ReaderQueueError::WriteConflict { .. })
        ));

        let following = match enqueue_offline_transaction(
            &pool,
            reader_id,
            &draft(FareTransactionId::generate()),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database_files(&path);

                panic!("following insertion failed: {error}")
            }
        };

        assert_eq!(following.local_sequence_number().value(), 2);

        pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn queued_transaction_survives_reopen() {
        let reader_id = ReaderId::generate();

        let (path, config, first_pool) = open_bound_database("survives-reopen", reader_id).await;

        let expected = match enqueue_offline_transaction(
            &first_pool,
            reader_id,
            &draft(FareTransactionId::generate()),
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                first_pool.close().await;
                remove_database_files(&path);

                panic!("queue insertion failed: {error}")
            }
        };

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

        let loaded = match load_offline_queue(&second_pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                second_pool.close().await;
                remove_database_files(&path);

                panic!("queue reload failed: {error}")
            }
        };

        assert_eq!(loaded, vec![expected]);

        second_pool.close().await;
        remove_database_files(&path);
    }

    #[tokio::test]
    async fn queue_requires_bound_reader() {
        let path = temporary_database_path("requires-binding");

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

            panic!("migration failed: {error}");
        }

        let reader_id = ReaderId::generate();

        let result =
            enqueue_offline_transaction(&pool, reader_id, &draft(FareTransactionId::generate()))
                .await;

        assert!(matches!(
            result,
            Err(
                ReaderQueueError::
                    SequenceUnavailable {
                        reader_id: failed_reader,
                    }
            ) if failed_reader == reader_id
        ));

        pool.close().await;
        remove_database_files(&path);
    }
}
