use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use thiserror::Error;
use transitguard_device_protocol::{
    SynchronizationBatchAcknowledgement, SynchronizationEntryOutcome,
    SynchronizationFailureCategory,
};
use transitguard_domain::{FareTransactionId, ReaderId, SynchronizationBatchId};
use uuid::Uuid;

use crate::{PersistenceError, PreparedSynchronizationIngest, PreparedSynchronizationIngestEntry};

const BATCH_ENTITY: &str = "synchronization ingest batch";
const TRANSACTION_ENTITY: &str = "synchronization ingest transaction";
const ENTRY_ENTITY: &str = "synchronization ingest entry";

const VALIDATE_READER_SQL: &str = r#"
SELECT status IN ('active', 'offline')
FROM reader_equipment
WHERE id = $1
FOR SHARE
"#;

const FIND_BATCH_SQL: &str = r#"
SELECT
    reader_id,
    request_fingerprint
FROM synchronization_ingest_batches
WHERE batch_id = $1
"#;

const LOAD_ACKNOWLEDGEMENT_SQL: &str = r#"
SELECT canonical_acknowledgement_json
FROM synchronization_ingest_batches
WHERE batch_id = $1
"#;

const INSERT_BATCH_SQL: &str = r#"
INSERT INTO synchronization_ingest_batches (
    batch_id,
    reader_id,
    protocol_version,
    environment_id,
    reader_software_version,
    first_local_sequence_number,
    last_local_sequence_number,
    submitted_at_unix_milliseconds,
    received_at_unix_milliseconds,
    entry_count,
    request_fingerprint,
    canonical_request_json,
    acknowledgement_fingerprint,
    canonical_acknowledgement_json
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7,
    $8, $9, $10, $11, $12, $13, $14
)
"#;

const FIND_TRANSACTION_SQL: &str = r#"
SELECT
    fare_transaction_id,
    reader_id,
    local_sequence_number,
    transaction_fingerprint
FROM synchronization_ingest_transactions
WHERE
    fare_transaction_id = $1
    OR (
        reader_id = $2
        AND local_sequence_number = $3
    )
FOR UPDATE
"#;

const INSERT_TRANSACTION_SQL: &str = r#"
INSERT INTO synchronization_ingest_transactions (
    fare_transaction_id,
    reader_id,
    local_sequence_number,
    transaction_fingerprint,
    canonical_transaction_envelope_json,
    first_seen_batch_id,
    current_resolution,
    first_received_at_unix_milliseconds,
    last_resolved_at_unix_milliseconds
)
VALUES (
    $1, $2, $3, $4, $5,
    $6, $7, $8, $9
)
"#;

const UPDATE_TRANSACTION_SQL: &str = r#"
UPDATE synchronization_ingest_transactions
SET
    current_resolution = CASE
        WHEN $3 >= last_resolved_at_unix_milliseconds
        THEN $2
        ELSE current_resolution
    END,
    last_resolved_at_unix_milliseconds =
        GREATEST(
            last_resolved_at_unix_milliseconds,
            $3
        ),
    updated_at = CURRENT_TIMESTAMP
WHERE fare_transaction_id = $1
"#;

const INSERT_ENTRY_SQL: &str = r#"
INSERT INTO synchronization_ingest_entries (
    batch_id,
    reader_id,
    entry_position,
    fare_transaction_id,
    local_sequence_number,
    outcome,
    failure_category,
    next_retry_at_unix_milliseconds,
    resolved_at_unix_milliseconds
)
VALUES (
    $1, $2, $3, $4, $5,
    $6, $7, $8, $9
)
"#;

/// Result of writing one synchronization batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizationIngestDisposition {
    /// The batch and its entries were written during this call.
    Stored,

    /// An identical durable batch was already present.
    Replayed,
}

/// Stable PostgreSQL synchronization-ingest failures.
#[derive(Debug, Error)]
pub enum SynchronizationIngestPersistenceError {
    /// The submitted reader does not exist.
    #[error("synchronization reader {reader_id} is not registered")]
    ReaderNotRegistered {
        /// Reader identity submitted by the batch.
        reader_id: ReaderId,
    },

    /// The submitted reader cannot currently authenticate.
    #[error("synchronization reader {reader_id} is not operational")]
    ReaderNotOperational {
        /// Reader identity submitted by the batch.
        reader_id: ReaderId,
    },

    /// A batch identity was reused with different content.
    #[error("synchronization batch {batch_id} conflicts with stored content")]
    BatchIdentityConflict {
        /// Conflicting batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// A transaction identity or reader sequence conflicted.
    #[error("synchronization transaction {transaction_id} conflicts with stored identity")]
    TransactionIdentityConflict {
        /// Conflicting transaction identity.
        transaction_id: FareTransactionId,
    },

    /// The underlying persistence boundary failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl SynchronizationIngestPersistenceError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::database(operation, source))
    }

    fn write(operation: &'static str, entity: &'static str, source: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::write(operation, entity, source))
    }

    const fn numeric(field: &'static str) -> Self {
        Self::Persistence(PersistenceError::NumericValueOutOfRange { field })
    }
}

/// PostgreSQL repository for reader synchronization ingest.
#[derive(Clone, Debug)]
pub struct PostgresSynchronizationIngestRepository {
    pool: PgPool,
}

impl PostgresSynchronizationIngestRepository {
    /// Creates a synchronization-ingest repository.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying PostgreSQL connection pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Loads the original committed acknowledgement for a batch.
    pub async fn load_acknowledgement(
        &self,
        batch_id: SynchronizationBatchId,
    ) -> Result<Option<SynchronizationBatchAcknowledgement>, SynchronizationIngestPersistenceError>
    {
        let stored = sqlx::query_scalar::<_, Json<serde_json::Value>>(LOAD_ACKNOWLEDGEMENT_SQL)
            .bind(batch_id.into_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| {
                SynchronizationIngestPersistenceError::database(
                    "load synchronization acknowledgement",
                    source,
                )
            })?;

        match stored {
            Some(Json(value)) => {
                let acknowledgement = serde_json::from_value(value).map_err(|source| {
                    SynchronizationIngestPersistenceError::Persistence(
                        PersistenceError::serialization(
                            "decode synchronization acknowledgement",
                            source,
                        ),
                    )
                })?;

                Ok(Some(acknowledgement))
            }

            None => Ok(None),
        }
    }

    /// Atomically stores one prepared synchronization batch.
    ///
    /// An exact durable replay succeeds without creating duplicate
    /// rows. Reusing a batch, transaction, or reader sequence with
    /// conflicting content is rejected.
    pub async fn store(
        &self,
        ingest: &PreparedSynchronizationIngest,
    ) -> Result<SynchronizationIngestDisposition, SynchronizationIngestPersistenceError> {
        let mut transaction = self.pool.begin().await.map_err(|source| {
            SynchronizationIngestPersistenceError::database("begin synchronization ingest", source)
        })?;

        validate_reader(&mut transaction, ingest).await?;

        let existing_batch = find_existing_batch(&mut transaction, ingest).await?;

        if let Some(existing_batch) = existing_batch {
            let disposition = validate_existing_batch(existing_batch, ingest)?;

            transaction.commit().await.map_err(|source| {
                SynchronizationIngestPersistenceError::database(
                    "finish synchronization replay",
                    source,
                )
            })?;

            return Ok(disposition);
        }

        insert_batch(&mut transaction, ingest).await?;

        for entry in ingest.entries() {
            store_transaction(&mut transaction, ingest, entry).await?;

            insert_entry(&mut transaction, ingest, entry).await?;
        }

        transaction.commit().await.map_err(|source| {
            SynchronizationIngestPersistenceError::database("commit synchronization ingest", source)
        })?;

        Ok(SynchronizationIngestDisposition::Stored)
    }
}

#[derive(Debug, FromRow)]
struct ExistingBatchRow {
    reader_id: Uuid,
    request_fingerprint: String,
}

#[derive(Debug, FromRow)]
struct ExistingTransactionRow {
    fare_transaction_id: Uuid,
    reader_id: Uuid,
    local_sequence_number: i64,
    transaction_fingerprint: String,
}

async fn validate_reader(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
) -> Result<(), SynchronizationIngestPersistenceError> {
    let operational = sqlx::query_scalar::<_, bool>(VALIDATE_READER_SQL)
        .bind(ingest.reader_id().into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::database(
                "validate synchronization reader",
                source,
            )
        })?;

    match operational {
        Some(true) => Ok(()),

        Some(false) => Err(
            SynchronizationIngestPersistenceError::ReaderNotOperational {
                reader_id: ingest.reader_id(),
            },
        ),

        None => Err(SynchronizationIngestPersistenceError::ReaderNotRegistered {
            reader_id: ingest.reader_id(),
        }),
    }
}

async fn find_existing_batch(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
) -> Result<Option<ExistingBatchRow>, SynchronizationIngestPersistenceError> {
    sqlx::query_as::<_, ExistingBatchRow>(FIND_BATCH_SQL)
        .bind(ingest.batch_id().into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::database(
                "find synchronization ingest batch",
                source,
            )
        })
}

fn validate_existing_batch(
    existing: ExistingBatchRow,
    ingest: &PreparedSynchronizationIngest,
) -> Result<SynchronizationIngestDisposition, SynchronizationIngestPersistenceError> {
    let matches = existing.reader_id == ingest.reader_id().into_uuid()
        && existing.request_fingerprint == ingest.request_fingerprint().to_string();

    if matches {
        Ok(SynchronizationIngestDisposition::Replayed)
    } else {
        Err(
            SynchronizationIngestPersistenceError::BatchIdentityConflict {
                batch_id: ingest.batch_id(),
            },
        )
    }
}

async fn insert_batch(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
) -> Result<(), SynchronizationIngestPersistenceError> {
    let first_sequence = sequence_to_i64(
        ingest.first_local_sequence_number().value(),
        "synchronization_ingest_batches.first_local_sequence_number",
    )?;

    let last_sequence = sequence_to_i64(
        ingest.last_local_sequence_number().value(),
        "synchronization_ingest_batches.last_local_sequence_number",
    )?;

    let entry_count = i32::try_from(ingest.entry_count()).map_err(|_| {
        SynchronizationIngestPersistenceError::numeric("synchronization_ingest_batches.entry_count")
    })?;

    sqlx::query(INSERT_BATCH_SQL)
        .bind(ingest.batch_id().into_uuid())
        .bind(ingest.reader_id().into_uuid())
        .bind(i32::from(ingest.protocol_version().value()))
        .bind(ingest.environment_id().as_str())
        .bind(ingest.reader_software_version().as_str())
        .bind(first_sequence)
        .bind(last_sequence)
        .bind(ingest.submitted_at_unix_milliseconds())
        .bind(ingest.received_at_unix_milliseconds())
        .bind(entry_count)
        .bind(ingest.request_fingerprint().to_string())
        .bind(Json(ingest.canonical_request_json().clone()))
        .bind(ingest.acknowledgement_fingerprint().to_string())
        .bind(Json(ingest.canonical_acknowledgement_json().clone()))
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::write(
                "insert synchronization ingest batch",
                BATCH_ENTITY,
                source,
            )
        })?;

    Ok(())
}

async fn store_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
    entry: &PreparedSynchronizationIngestEntry,
) -> Result<(), SynchronizationIngestPersistenceError> {
    let local_sequence = sequence_to_i64(
        entry.local_sequence_number().value(),
        "synchronization_ingest_transactions.local_sequence_number",
    )?;

    let existing = sqlx::query_as::<_, ExistingTransactionRow>(FIND_TRANSACTION_SQL)
        .bind(entry.transaction_id().into_uuid())
        .bind(ingest.reader_id().into_uuid())
        .bind(local_sequence)
        .fetch_all(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::database(
                "find synchronization ingest transaction",
                source,
            )
        })?;

    match existing.as_slice() {
        [] => insert_transaction(transaction, ingest, entry, local_sequence).await,

        [stored] if transaction_identity_matches(stored, ingest, entry, local_sequence) => {
            update_transaction(transaction, entry).await
        }

        _ => Err(
            SynchronizationIngestPersistenceError::TransactionIdentityConflict {
                transaction_id: entry.transaction_id(),
            },
        ),
    }
}

fn transaction_identity_matches(
    stored: &ExistingTransactionRow,
    ingest: &PreparedSynchronizationIngest,
    entry: &PreparedSynchronizationIngestEntry,
    local_sequence: i64,
) -> bool {
    stored.fare_transaction_id == entry.transaction_id().into_uuid()
        && stored.reader_id == ingest.reader_id().into_uuid()
        && stored.local_sequence_number == local_sequence
        && stored.transaction_fingerprint == entry.transaction_fingerprint().to_string()
}

async fn insert_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
    entry: &PreparedSynchronizationIngestEntry,
    local_sequence: i64,
) -> Result<(), SynchronizationIngestPersistenceError> {
    sqlx::query(INSERT_TRANSACTION_SQL)
        .bind(entry.transaction_id().into_uuid())
        .bind(ingest.reader_id().into_uuid())
        .bind(local_sequence)
        .bind(entry.transaction_fingerprint().to_string())
        .bind(Json(entry.canonical_transaction_envelope_json().clone()))
        .bind(ingest.batch_id().into_uuid())
        .bind(outcome_name(entry.outcome()))
        .bind(ingest.received_at_unix_milliseconds())
        .bind(entry.resolved_at_unix_milliseconds())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::write(
                "insert synchronization ingest transaction",
                TRANSACTION_ENTITY,
                source,
            )
        })?;

    Ok(())
}

async fn update_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    entry: &PreparedSynchronizationIngestEntry,
) -> Result<(), SynchronizationIngestPersistenceError> {
    let result = sqlx::query(UPDATE_TRANSACTION_SQL)
        .bind(entry.transaction_id().into_uuid())
        .bind(outcome_name(entry.outcome()))
        .bind(entry.resolved_at_unix_milliseconds())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::write(
                "update synchronization ingest transaction",
                TRANSACTION_ENTITY,
                source,
            )
        })?;

    if result.rows_affected() != 1 {
        return Err(
            SynchronizationIngestPersistenceError::TransactionIdentityConflict {
                transaction_id: entry.transaction_id(),
            },
        );
    }

    Ok(())
}

async fn insert_entry(
    transaction: &mut Transaction<'_, Postgres>,
    ingest: &PreparedSynchronizationIngest,
    entry: &PreparedSynchronizationIngestEntry,
) -> Result<(), SynchronizationIngestPersistenceError> {
    let entry_position = i32::try_from(entry.entry_position()).map_err(|_| {
        SynchronizationIngestPersistenceError::numeric(
            "synchronization_ingest_entries.entry_position",
        )
    })?;

    let local_sequence = sequence_to_i64(
        entry.local_sequence_number().value(),
        "synchronization_ingest_entries.local_sequence_number",
    )?;

    let failure_category = entry
        .failure_category()
        .map(SynchronizationFailureCategory::as_str);

    sqlx::query(INSERT_ENTRY_SQL)
        .bind(ingest.batch_id().into_uuid())
        .bind(ingest.reader_id().into_uuid())
        .bind(entry_position)
        .bind(entry.transaction_id().into_uuid())
        .bind(local_sequence)
        .bind(outcome_name(entry.outcome()))
        .bind(failure_category)
        .bind(entry.next_retry_at_unix_milliseconds())
        .bind(entry.resolved_at_unix_milliseconds())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            SynchronizationIngestPersistenceError::write(
                "insert synchronization ingest entry",
                ENTRY_ENTITY,
                source,
            )
        })?;

    Ok(())
}

const fn outcome_name(outcome: SynchronizationEntryOutcome) -> &'static str {
    match outcome {
        SynchronizationEntryOutcome::Acknowledged => "acknowledged",

        SynchronizationEntryOutcome::RetryableFailure => "retryable_failure",

        SynchronizationEntryOutcome::PermanentFailure => "permanent_failure",

        SynchronizationEntryOutcome::ManualReview => "manual_review",
    }
}

fn sequence_to_i64(
    value: u64,
    field: &'static str,
) -> Result<i64, SynchronizationIngestPersistenceError> {
    i64::try_from(value).map_err(|_| SynchronizationIngestPersistenceError::numeric(field))
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::SynchronizationEntryOutcome;

    use super::{SynchronizationIngestPersistenceError, outcome_name, sequence_to_i64};

    #[test]
    fn outcomes_use_schema_values() {
        assert_eq!(
            outcome_name(SynchronizationEntryOutcome::Acknowledged),
            "acknowledged"
        );

        assert_eq!(
            outcome_name(SynchronizationEntryOutcome::RetryableFailure),
            "retryable_failure"
        );

        assert_eq!(
            outcome_name(SynchronizationEntryOutcome::PermanentFailure),
            "permanent_failure"
        );

        assert_eq!(
            outcome_name(SynchronizationEntryOutcome::ManualReview),
            "manual_review"
        );
    }

    #[test]
    fn sequence_conversion_accepts_supported_values() {
        let converted = sequence_to_i64(42, "test_sequence");

        assert!(matches!(converted, Ok(42)));
    }

    #[test]
    fn sequence_conversion_rejects_unsupported_values() {
        let converted = sequence_to_i64(u64::MAX, "test_sequence");

        assert!(matches!(
            converted,
            Err(SynchronizationIngestPersistenceError::Persistence(
                crate::PersistenceError::NumericValueOutOfRange {
                    field: "test_sequence"
                }
            ))
        ));
    }
}
