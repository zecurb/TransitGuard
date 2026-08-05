use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_device_protocol::{
    CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
    ReaderSoftwareVersion, SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
    SynchronizationProtocolError, SynchronizationRequestEntry,
};
use transitguard_domain::{
    FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};

/// Failures produced while reconstructing a durable synchronization request.
#[derive(Debug, Error)]
pub enum ReaderSynchronizationRequestError {
    /// The requested batch does not exist for this reader.
    #[error("synchronization batch {batch_id} was not found for reader {reader_id}")]
    BatchNotFound {
        /// Requested durable batch identity.
        batch_id: SynchronizationBatchId,

        /// Reader expected to own the batch.
        reader_id: ReaderId,
    },

    /// Only a durably submitted batch can be transported.
    #[error("synchronization batch {batch_id} is not awaiting a backend acknowledgement")]
    BatchNotInFlight {
        /// Batch in an invalid lifecycle state.
        batch_id: SynchronizationBatchId,
    },

    /// A submitted batch must preserve its original submission time.
    #[error("synchronization batch {batch_id} has no durable submission time")]
    MissingSubmissionTime {
        /// Batch missing submission metadata.
        batch_id: SynchronizationBatchId,
    },

    /// Reader metadata has not been initialized.
    #[error("reader state was not found for reader {reader_id}")]
    ReaderStateNotFound {
        /// Expected durable reader identity.
        reader_id: ReaderId,
    },

    /// The reader database belongs to a different reader.
    #[error("stored reader {actual} does not match requested reader {expected}")]
    ReaderIdentityMismatch {
        /// Reader requested by the caller.
        expected: ReaderId,

        /// Reader recorded in SQLite.
        actual: ReaderId,
    },

    /// Reader and batch protocol versions must remain identical.
    #[error("synchronization batch {batch_id} protocol does not match reader state")]
    ProtocolMismatch {
        /// Batch containing inconsistent protocol metadata.
        batch_id: SynchronizationBatchId,
    },

    /// SQLite contains malformed request metadata.
    #[error("reader synchronization request contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable invalid field name.
        field: &'static str,
    },

    /// Durable values failed protocol validation.
    #[error(transparent)]
    Protocol(#[from] SynchronizationProtocolError),

    /// A named SQLite request-assembly operation failed.
    #[error("reader SQLite synchronization-request operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderSynchronizationRequestError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

#[derive(sqlx::FromRow)]
struct StoredReaderState {
    reader_id: String,
    environment_id: String,
    software_version: String,
    protocol_version: i64,
}

#[derive(sqlx::FromRow)]
struct StoredBatch {
    protocol_version: i64,
    first_local_sequence_number: i64,
    last_local_sequence_number: i64,
    batch_state: String,
    submitted_at_unix_milliseconds: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct StoredEntry {
    fare_transaction_id: String,
    local_sequence_number: i64,
    entry_position: i64,
    transaction_envelope_json: String,
}

/// Reconstructs the exact protocol request for an in-flight durable batch.
///
/// The original submission timestamp, batch identity, entry ordering, and
/// canonical transaction envelopes are loaded from one SQLite transaction.
/// Repeated calls for the same retry therefore produce the same request
/// fingerprint.
pub async fn load_synchronization_batch_request(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
) -> Result<SynchronizationBatchRequest, ReaderSynchronizationRequestError> {
    let mut transaction = pool.begin().await.map_err(|source| {
        ReaderSynchronizationRequestError::database("begin request assembly", source)
    })?;

    let batch = sqlx::query_as::<_, StoredBatch>(
        r#"
        SELECT
            protocol_version,
            first_local_sequence_number,
            last_local_sequence_number,
            batch_state,
            submitted_at_unix_milliseconds
        FROM synchronization_batches
        WHERE
            batch_id = ?
            AND reader_id = ?
        "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationRequestError::database("load synchronization batch", source)
    })?
    .ok_or(ReaderSynchronizationRequestError::BatchNotFound {
        batch_id,
        reader_id,
    })?;

    if batch.batch_state != "in_flight" {
        return Err(ReaderSynchronizationRequestError::BatchNotInFlight { batch_id });
    }

    let submitted_at_unix_milliseconds = batch
        .submitted_at_unix_milliseconds
        .ok_or(ReaderSynchronizationRequestError::MissingSubmissionTime { batch_id })?;

    if submitted_at_unix_milliseconds < 0 {
        return Err(ReaderSynchronizationRequestError::invalid_stored_value(
            "submitted_at_unix_milliseconds",
        ));
    }

    let reader = sqlx::query_as::<_, StoredReaderState>(
        r#"
        SELECT
            reader_id,
            environment_id,
            software_version,
            protocol_version
        FROM reader_state
        WHERE singleton = 1
        "#,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| ReaderSynchronizationRequestError::database("load reader state", source))?
    .ok_or(ReaderSynchronizationRequestError::ReaderStateNotFound { reader_id })?;

    let stored_reader_id = reader
        .reader_id
        .parse::<ReaderId>()
        .map_err(|_| ReaderSynchronizationRequestError::invalid_stored_value("reader_id"))?;

    if stored_reader_id != reader_id {
        return Err(ReaderSynchronizationRequestError::ReaderIdentityMismatch {
            expected: reader_id,
            actual: stored_reader_id,
        });
    }

    let reader_protocol = decode_protocol_version(reader.protocol_version)?;

    let batch_protocol = decode_protocol_version(batch.protocol_version)?;

    if reader_protocol != batch_protocol {
        return Err(ReaderSynchronizationRequestError::ProtocolMismatch { batch_id });
    }

    let first_local_sequence_number = decode_sequence(
        batch.first_local_sequence_number,
        "first_local_sequence_number",
    )?;

    let last_local_sequence_number = decode_sequence(
        batch.last_local_sequence_number,
        "last_local_sequence_number",
    )?;

    let stored_entries = sqlx::query_as::<_, StoredEntry>(
        r#"
        SELECT
            entry.fare_transaction_id,
            entry.local_sequence_number,
            entry.entry_position,
            queued.transaction_envelope_json
        FROM synchronization_entries AS entry
        INNER JOIN offline_transactions AS queued
            ON queued.fare_transaction_id =
                entry.fare_transaction_id
            AND queued.reader_id = entry.reader_id
            AND queued.local_sequence_number =
                entry.local_sequence_number
        WHERE
            entry.batch_id = ?
            AND entry.reader_id = ?
        ORDER BY entry.entry_position
        "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_all(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderSynchronizationRequestError::database("load synchronization request entries", source)
    })?;

    if stored_entries.is_empty() {
        return Err(ReaderSynchronizationRequestError::invalid_stored_value(
            "synchronization_entries",
        ));
    }

    let mut entries = Vec::with_capacity(stored_entries.len());

    for (expected_position, stored) in stored_entries.into_iter().enumerate() {
        let actual_position = usize::try_from(stored.entry_position).map_err(|_| {
            ReaderSynchronizationRequestError::invalid_stored_value("entry_position")
        })?;

        if actual_position != expected_position {
            return Err(ReaderSynchronizationRequestError::invalid_stored_value(
                "entry_position",
            ));
        }

        let transaction_id = stored
            .fare_transaction_id
            .parse::<FareTransactionId>()
            .map_err(|_| {
                ReaderSynchronizationRequestError::invalid_stored_value("fare_transaction_id")
            })?;

        let local_sequence_number =
            decode_sequence(stored.local_sequence_number, "local_sequence_number")?;

        let transaction_envelope =
            CanonicalTransactionEnvelope::from_json(&stored.transaction_envelope_json)?;

        entries.push(SynchronizationRequestEntry::new(
            transaction_id,
            local_sequence_number,
            transaction_envelope,
        ));
    }

    let request = SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
        protocol_version: batch_protocol,
        environment_id: ProtocolEnvironmentId::new(reader.environment_id)?,
        reader_id,
        reader_software_version: ReaderSoftwareVersion::new(reader.software_version)?,
        batch_id,
        first_local_sequence_number,
        last_local_sequence_number,
        submitted_at_unix_milliseconds,
        entries,
    })?;

    transaction.commit().await.map_err(|source| {
        ReaderSynchronizationRequestError::database("commit request assembly", source)
    })?;

    Ok(request)
}

fn decode_protocol_version(
    value: i64,
) -> Result<DeviceProtocolVersion, ReaderSynchronizationRequestError> {
    let value = u16::try_from(value)
        .map_err(|_| ReaderSynchronizationRequestError::invalid_stored_value("protocol_version"))?;

    DeviceProtocolVersion::new(value)
        .map_err(|_| ReaderSynchronizationRequestError::invalid_stored_value("protocol_version"))
}

fn decode_sequence(
    value: i64,
    field: &'static str,
) -> Result<LocalSequenceNumber, ReaderSynchronizationRequestError> {
    let value = u64::try_from(value)
        .map_err(|_| ReaderSynchronizationRequestError::invalid_stored_value(field))?;

    LocalSequenceNumber::new(value)
        .map_err(|_| ReaderSynchronizationRequestError::invalid_stored_value(field))
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
        OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig, bind_reader_database,
        connect_reader_sqlite, create_synchronization_batch, enqueue_offline_transaction,
        mark_synchronization_batch_in_flight, record_synchronization_retryable_failure,
        run_reader_sqlite_migrations,
    };

    use super::load_synchronization_batch_request;

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-sync-request-{name}-{}.sqlite3",
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

    #[tokio::test]
    async fn retries_reconstruct_the_same_protocol_request() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("stable-retry", reader_id).await;

        for _ in 0..2 {
            if let Err(error) = enqueue_offline_transaction(&pool, reader_id, &draft()).await {
                pool.close().await;
                remove_database(&path);
                panic!("queue insertion failed: {error}");
            }
        }

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
                panic!("batch creation failed: {error}");
            }
        };

        if let Err(error) = mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 300,
        )
        .await
        {
            pool.close().await;
            remove_database(&path);
            panic!("first submission failed: {error}");
        }

        let first =
            match load_synchronization_batch_request(&pool, reader_id, batch.batch_id()).await {
                Ok(value) => value,
                Err(error) => {
                    pool.close().await;
                    remove_database(&path);
                    panic!("first request failed: {error}");
                }
            };

        assert_eq!(first.submitted_at_unix_milliseconds(), TEST_TIME + 300);
        assert_eq!(first.entries().len(), 2);
        assert_eq!(first.environment_id().as_str(), "development");
        assert_eq!(first.reader_software_version().as_str(), "0.1.0");

        if let Err(error) = record_synchronization_retryable_failure(
            &pool,
            reader_id,
            batch.batch_id(),
            "network_timeout",
            TEST_TIME + 400,
            TEST_TIME + 500,
        )
        .await
        {
            pool.close().await;
            remove_database(&path);
            panic!("retry failure recording failed: {error}");
        }

        if let Err(error) = mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 500,
        )
        .await
        {
            pool.close().await;
            remove_database(&path);
            panic!("second submission failed: {error}");
        }

        let second =
            match load_synchronization_batch_request(&pool, reader_id, batch.batch_id()).await {
                Ok(value) => value,
                Err(error) => {
                    pool.close().await;
                    remove_database(&path);
                    panic!("second request failed: {error}");
                }
            };

        assert_eq!(first, second);
        assert_eq!(first.fingerprint(), second.fingerprint());

        pool.close().await;
        remove_database(&path);
    }
}
