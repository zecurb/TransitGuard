use sqlx::{FromRow, PgPool, Postgres, Transaction, error::ErrorKind, types::Json};
use thiserror::Error;
use transitguard_domain::{
    FarePolicyVersion, FareTransactionId, Money, ReaderId, SynchronizationBatchId,
};
use transitguard_reconciliation::{
    DiscrepancyCase, DiscrepancyCategory, DiscrepancyState, ProposedAdjustment,
    ProposedAdjustmentDirection, ReconciliationEvidence, ReconciliationId, ReconciliationOutcome,
    ReconciliationRecord, ReconciliationStatus, ReconciliationTime,
};
use uuid::Uuid;

use crate::{PersistenceError, PostgresValueCodec, PreparedReconciliationPersistence};

const RECONCILIATION_ENTITY: &str = "reconciliation record";
const DISCREPANCY_ENTITY: &str = "reconciliation discrepancy";
const ADJUSTMENT_ENTITY: &str = "reconciliation proposed adjustment";

const ACQUIRE_TRANSACTION_LOCK_SQL: &str = r#"
SELECT pg_advisory_xact_lock(
    hashtextextended($1, 0)
)
"#;

const FIND_EXISTING_SQL: &str = r#"
SELECT
    reconciliation_id,
    fare_transaction_id,
    source_batch_id,
    reader_id,
    reader_evidence_fingerprint,
    backend_evidence_fingerprint,
    reader_evidence_json,
    backend_evidence_json,
    reader_policy_id,
    reader_policy_version,
    backend_policy_id,
    backend_policy_version,
    outcome,
    status,
    observed_minor_units,
    observed_currency,
    expected_minor_units,
    expected_currency,
    monetary_difference_minor_units,
    monetary_difference_currency,
    reconciled_at_unix_milliseconds
FROM reconciliation_records
WHERE
    fare_transaction_id = $1
    OR reconciliation_id = $2
FOR UPDATE
"#;

const LOAD_BY_TRANSACTION_SQL: &str = r#"
SELECT
    reconciliation_id,
    fare_transaction_id,
    source_batch_id,
    reader_id,
    reader_evidence_fingerprint,
    backend_evidence_fingerprint,
    reader_evidence_json,
    backend_evidence_json,
    reader_policy_id,
    reader_policy_version,
    backend_policy_id,
    backend_policy_version,
    outcome,
    status,
    observed_minor_units,
    observed_currency,
    expected_minor_units,
    expected_currency,
    monetary_difference_minor_units,
    monetary_difference_currency,
    reconciled_at_unix_milliseconds
FROM reconciliation_records
WHERE fare_transaction_id = $1
"#;

const LOAD_BY_ID_SQL: &str = r#"
SELECT
    reconciliation_id,
    fare_transaction_id,
    source_batch_id,
    reader_id,
    reader_evidence_fingerprint,
    backend_evidence_fingerprint,
    reader_evidence_json,
    backend_evidence_json,
    reader_policy_id,
    reader_policy_version,
    backend_policy_id,
    backend_policy_version,
    outcome,
    status,
    observed_minor_units,
    observed_currency,
    expected_minor_units,
    expected_currency,
    monetary_difference_minor_units,
    monetary_difference_currency,
    reconciled_at_unix_milliseconds
FROM reconciliation_records
WHERE reconciliation_id = $1
"#;

const LOAD_SOURCE_TRANSACTION_SQL: &str = r#"
SELECT
    reader_id,
    current_resolution
FROM synchronization_ingest_transactions
WHERE fare_transaction_id = $1
FOR SHARE
"#;

const SOURCE_BATCH_CONTAINS_TRANSACTION_SQL: &str = r#"
SELECT EXISTS (
    SELECT 1
    FROM synchronization_ingest_entries
    WHERE
        batch_id = $1
        AND fare_transaction_id = $2
        AND reader_id = $3
)
"#;

const INSERT_RECONCILIATION_SQL: &str = r#"
INSERT INTO reconciliation_records (
    reconciliation_id,
    fare_transaction_id,
    source_batch_id,
    reader_id,
    reader_evidence_fingerprint,
    backend_evidence_fingerprint,
    reader_evidence_json,
    backend_evidence_json,
    reader_policy_id,
    reader_policy_version,
    backend_policy_id,
    backend_policy_version,
    outcome,
    status,
    observed_minor_units,
    observed_currency,
    expected_minor_units,
    expected_currency,
    monetary_difference_minor_units,
    monetary_difference_currency,
    reconciled_at_unix_milliseconds
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8,
    $9, $10, $11, $12, $13, $14,
    $15, $16, $17, $18, $19, $20, $21
)
"#;

const INSERT_DISCREPANCY_SQL: &str = r#"
INSERT INTO reconciliation_discrepancy_cases (
    discrepancy_case_id,
    reconciliation_id,
    fare_transaction_id,
    reader_id,
    category,
    state,
    created_at_unix_milliseconds
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7
)
"#;

const INSERT_ADJUSTMENT_SQL: &str = r#"
INSERT INTO reconciliation_proposed_adjustments (
    proposed_adjustment_id,
    reconciliation_id,
    fare_transaction_id,
    correction_minor_units,
    currency,
    direction,
    created_at_unix_milliseconds
)
VALUES (
    $1, $2, $3, $4, $5, $6, $7
)
"#;

/// Result of attempting to persist one authoritative reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationPersistenceDisposition {
    /// Reconciliation and dependent state were created by this call.
    Stored,

    /// Identical immutable reconciliation state already existed.
    Replayed,
}

/// Reconstructed durable reconciliation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredReconciliation {
    record: ReconciliationRecord,
    reader_evidence: ReconciliationEvidence,
    backend_evidence: ReconciliationEvidence,
}

impl StoredReconciliation {
    /// Returns the reconstructed authoritative record.
    #[must_use]
    pub const fn record(self) -> ReconciliationRecord {
        self.record
    }

    /// Returns the immutable reader evidence.
    #[must_use]
    pub const fn reader_evidence(self) -> ReconciliationEvidence {
        self.reader_evidence
    }

    /// Returns the immutable backend evidence.
    #[must_use]
    pub const fn backend_evidence(self) -> ReconciliationEvidence {
        self.backend_evidence
    }
}

/// Stable failures from PostgreSQL reconciliation persistence.
#[derive(Debug, Error)]
pub enum ReconciliationRepositoryError {
    /// Synchronized source transaction does not exist.
    #[error("reconciliation source transaction {transaction_id} is not stored")]
    SourceTransactionNotFound {
        /// Missing source transaction.
        transaction_id: FareTransactionId,
    },

    /// Source transaction has not reached the acknowledged state.
    #[error("reconciliation source transaction {transaction_id} is not acknowledged")]
    SourceTransactionNotReady {
        /// Source transaction.
        transaction_id: FareTransactionId,
    },

    /// Reader identity disagrees with synchronized source provenance.
    #[error("reconciliation reader conflicts with source transaction {transaction_id}")]
    SourceReaderConflict {
        /// Source transaction.
        transaction_id: FareTransactionId,
    },

    /// Claimed synchronization batch does not contain the transaction.
    #[error("reconciliation source batch {batch_id} does not contain transaction {transaction_id}")]
    SourceBatchConflict {
        /// Claimed batch.
        batch_id: SynchronizationBatchId,

        /// Source transaction.
        transaction_id: FareTransactionId,
    },

    /// Reconciliation identity or transaction identity was reused with
    /// different immutable evidence.
    #[error("reconciliation identity conflicts with stored transaction {transaction_id}")]
    IdentityConflict {
        /// Transaction whose durable identity conflicts.
        transaction_id: FareTransactionId,
    },

    /// Stored reconciliation data could not be safely reconstructed.
    #[error("stored reconciliation contains an invalid value for `{field}`")]
    InvalidStoredRecord {
        /// Stable schema field.
        field: &'static str,
    },

    /// Underlying persistence boundary failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl ReconciliationRepositoryError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::database(operation, source))
    }

    fn write(operation: &'static str, entity: &'static str, source: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::write(operation, entity, source))
    }

    const fn invalid(field: &'static str) -> Self {
        Self::InvalidStoredRecord { field }
    }
}

/// PostgreSQL repository for authoritative financial reconciliation.
#[derive(Clone, Debug)]
pub struct PostgresReconciliationRepository {
    pool: PgPool,
}

impl PostgresReconciliationRepository {
    /// Creates a PostgreSQL reconciliation repository.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying PostgreSQL pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Atomically stores a complete prepared reconciliation.
    ///
    /// The source synchronized transaction is verified before any
    /// reconciliation row is inserted. The authoritative record, discrepancy
    /// case, and proposed adjustment then commit in one PostgreSQL
    /// transaction.
    ///
    /// An exact replay succeeds without creating duplicate rows. Reusing a
    /// reconciliation or fare-transaction identity with different immutable
    /// evidence is rejected.
    pub async fn store(
        &self,
        prepared: &PreparedReconciliationPersistence,
    ) -> Result<ReconciliationPersistenceDisposition, ReconciliationRepositoryError> {
        let record = prepared.record();

        let mut transaction = self.pool.begin().await.map_err(|source| {
            ReconciliationRepositoryError::database("begin reconciliation persistence", source)
        })?;

        acquire_transaction_lock(&mut transaction, record).await?;

        let existing = find_existing(&mut transaction, record).await?;

        match existing.as_slice() {
            [] => {}

            [row] => {
                let stored = decode_row(row)?;

                if stored.record == record
                    && stored.reader_evidence == prepared.reader_evidence()
                    && stored.backend_evidence == prepared.backend_evidence()
                {
                    transaction.commit().await.map_err(|source| {
                        ReconciliationRepositoryError::database(
                            "finish reconciliation replay",
                            source,
                        )
                    })?;

                    return Ok(ReconciliationPersistenceDisposition::Replayed);
                }

                return Err(ReconciliationRepositoryError::IdentityConflict {
                    transaction_id: record.transaction_id(),
                });
            }

            _ => {
                return Err(ReconciliationRepositoryError::IdentityConflict {
                    transaction_id: record.transaction_id(),
                });
            }
        }

        validate_source_transaction(&mut transaction, record).await?;

        insert_reconciliation(&mut transaction, prepared).await?;

        if let Some(discrepancy) = prepared.discrepancy_case() {
            insert_discrepancy(&mut transaction, discrepancy).await?;
        }

        if let Some(adjustment) = prepared.proposed_adjustment() {
            insert_adjustment(&mut transaction, adjustment).await?;
        }

        transaction.commit().await.map_err(|source| {
            ReconciliationRepositoryError::database("commit reconciliation persistence", source)
        })?;

        Ok(ReconciliationPersistenceDisposition::Stored)
    }

    /// Loads an authoritative reconciliation by transaction identity.
    pub async fn load_by_transaction_id(
        &self,
        transaction_id: FareTransactionId,
    ) -> Result<Option<StoredReconciliation>, ReconciliationRepositoryError> {
        let row = sqlx::query_as::<_, ReconciliationRow>(LOAD_BY_TRANSACTION_SQL)
            .bind(transaction_id.into_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationRepositoryError::database(
                    "load reconciliation by transaction",
                    source,
                )
            })?;

        row.as_ref().map(decode_row).transpose()
    }

    /// Loads an authoritative reconciliation by reconciliation identity.
    pub async fn load_by_id(
        &self,
        reconciliation_id: ReconciliationId,
    ) -> Result<Option<StoredReconciliation>, ReconciliationRepositoryError> {
        let row = sqlx::query_as::<_, ReconciliationRow>(LOAD_BY_ID_SQL)
            .bind(*reconciliation_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationRepositoryError::database("load reconciliation by identity", source)
            })?;

        row.as_ref().map(decode_row).transpose()
    }
}

#[derive(Debug, FromRow)]
struct ReconciliationRow {
    reconciliation_id: Uuid,
    fare_transaction_id: Uuid,
    source_batch_id: Option<Uuid>,
    reader_id: Uuid,
    reader_evidence_fingerprint: String,
    backend_evidence_fingerprint: String,
    reader_evidence_json: Json<serde_json::Value>,
    backend_evidence_json: Json<serde_json::Value>,
    reader_policy_id: Uuid,
    reader_policy_version: i64,
    backend_policy_id: Uuid,
    backend_policy_version: i64,
    outcome: String,
    status: String,
    observed_minor_units: Option<i64>,
    observed_currency: Option<String>,
    expected_minor_units: Option<i64>,
    expected_currency: Option<String>,
    monetary_difference_minor_units: Option<i64>,
    monetary_difference_currency: Option<String>,
    reconciled_at_unix_milliseconds: i64,
}

#[derive(Debug, FromRow)]
struct SourceTransactionRow {
    reader_id: Uuid,
    current_resolution: String,
}

async fn acquire_transaction_lock(
    transaction: &mut Transaction<'_, Postgres>,
    record: ReconciliationRecord,
) -> Result<(), ReconciliationRepositoryError> {
    sqlx::query(ACQUIRE_TRANSACTION_LOCK_SQL)
        .bind(record.transaction_id().into_uuid().to_string())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            ReconciliationRepositoryError::database(
                "lock reconciliation transaction identity",
                source,
            )
        })?;

    Ok(())
}

async fn find_existing(
    transaction: &mut Transaction<'_, Postgres>,
    record: ReconciliationRecord,
) -> Result<Vec<ReconciliationRow>, ReconciliationRepositoryError> {
    sqlx::query_as::<_, ReconciliationRow>(FIND_EXISTING_SQL)
        .bind(record.transaction_id().into_uuid())
        .bind(*record.id().as_uuid())
        .fetch_all(&mut **transaction)
        .await
        .map_err(|source| {
            ReconciliationRepositoryError::database("find existing reconciliation", source)
        })
}

async fn validate_source_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    record: ReconciliationRecord,
) -> Result<(), ReconciliationRepositoryError> {
    let source = sqlx::query_as::<_, SourceTransactionRow>(LOAD_SOURCE_TRANSACTION_SQL)
        .bind(record.transaction_id().into_uuid())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|source| {
            ReconciliationRepositoryError::database(
                "load reconciliation source transaction",
                source,
            )
        })?;

    let source = source.ok_or(ReconciliationRepositoryError::SourceTransactionNotFound {
        transaction_id: record.transaction_id(),
    })?;

    if source.reader_id != record.reader_id().into_uuid() {
        return Err(ReconciliationRepositoryError::SourceReaderConflict {
            transaction_id: record.transaction_id(),
        });
    }

    if source.current_resolution != "acknowledged" {
        return Err(ReconciliationRepositoryError::SourceTransactionNotReady {
            transaction_id: record.transaction_id(),
        });
    }

    if let Some(batch_id) = record.source_batch_id() {
        let contains = sqlx::query_scalar::<_, bool>(SOURCE_BATCH_CONTAINS_TRANSACTION_SQL)
            .bind(batch_id.into_uuid())
            .bind(record.transaction_id().into_uuid())
            .bind(record.reader_id().into_uuid())
            .fetch_one(&mut **transaction)
            .await
            .map_err(|source| {
                ReconciliationRepositoryError::database(
                    "validate reconciliation source batch",
                    source,
                )
            })?;

        if !contains {
            return Err(ReconciliationRepositoryError::SourceBatchConflict {
                batch_id,
                transaction_id: record.transaction_id(),
            });
        }
    }

    Ok(())
}

async fn insert_reconciliation(
    transaction: &mut Transaction<'_, Postgres>,
    prepared: &PreparedReconciliationPersistence,
) -> Result<(), ReconciliationRepositoryError> {
    let record = prepared.record();

    let reader_json = serde_json::to_value(prepared.reader_evidence()).map_err(|source| {
        ReconciliationRepositoryError::Persistence(PersistenceError::serialization(
            "serialize reconciliation reader evidence",
            source,
        ))
    })?;

    let backend_json = serde_json::to_value(prepared.backend_evidence()).map_err(|source| {
        ReconciliationRepositoryError::Persistence(PersistenceError::serialization(
            "serialize reconciliation backend evidence",
            source,
        ))
    })?;

    let reader_policy_version = policy_version_to_i64(record.reader_policy_version())?;

    let backend_policy_version = policy_version_to_i64(record.backend_policy_version())?;

    let observed = money_columns(record.observed_amount());
    let expected = money_columns(record.expected_amount());
    let difference = money_columns(record.monetary_difference());

    let result = sqlx::query(INSERT_RECONCILIATION_SQL)
        .bind(*record.id().as_uuid())
        .bind(record.transaction_id().into_uuid())
        .bind(
            record
                .source_batch_id()
                .map(SynchronizationBatchId::into_uuid),
        )
        .bind(record.reader_id().into_uuid())
        .bind(record.reader_evidence_fingerprint().to_string())
        .bind(record.backend_evidence_fingerprint().to_string())
        .bind(Json(reader_json))
        .bind(Json(backend_json))
        .bind(record.reader_policy_id().into_uuid())
        .bind(reader_policy_version)
        .bind(record.backend_policy_id().into_uuid())
        .bind(backend_policy_version)
        .bind(outcome_name(record.outcome()))
        .bind(status_name(record.status()))
        .bind(observed.0)
        .bind(observed.1)
        .bind(expected.0)
        .bind(expected.1)
        .bind(difference.0)
        .bind(difference.1)
        .bind(record.reconciled_at().unix_milliseconds())
        .execute(&mut **transaction)
        .await;

    match result {
        Ok(_) => Ok(()),

        Err(source)
            if matches!(
                source.as_database_error().map(|error| error.kind()),
                Some(ErrorKind::UniqueViolation)
            ) =>
        {
            Err(ReconciliationRepositoryError::IdentityConflict {
                transaction_id: record.transaction_id(),
            })
        }

        Err(source) => Err(ReconciliationRepositoryError::write(
            "insert reconciliation record",
            RECONCILIATION_ENTITY,
            source,
        )),
    }
}

async fn insert_discrepancy(
    transaction: &mut Transaction<'_, Postgres>,
    discrepancy: &DiscrepancyCase,
) -> Result<(), ReconciliationRepositoryError> {
    sqlx::query(INSERT_DISCREPANCY_SQL)
        .bind(*discrepancy.id().reconciliation_id().as_uuid())
        .bind(*discrepancy.reconciliation_id().as_uuid())
        .bind(discrepancy.transaction_id().into_uuid())
        .bind(discrepancy.reader_id().into_uuid())
        .bind(discrepancy_category_name(discrepancy.category()))
        .bind(discrepancy_state_name(discrepancy.state()))
        .bind(discrepancy.created_at().unix_milliseconds())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            ReconciliationRepositoryError::write(
                "insert reconciliation discrepancy",
                DISCREPANCY_ENTITY,
                source,
            )
        })?;

    Ok(())
}

async fn insert_adjustment(
    transaction: &mut Transaction<'_, Postgres>,
    adjustment: ProposedAdjustment,
) -> Result<(), ReconciliationRepositoryError> {
    let amount = adjustment.correction_amount();

    sqlx::query(INSERT_ADJUSTMENT_SQL)
        .bind(*adjustment.id().reconciliation_id().as_uuid())
        .bind(*adjustment.reconciliation_id().as_uuid())
        .bind(adjustment.transaction_id().into_uuid())
        .bind(amount.minor_units())
        .bind(PostgresValueCodec::encode_currency(amount.currency()))
        .bind(adjustment_direction_name(adjustment.direction()))
        .bind(adjustment.created_at().unix_milliseconds())
        .execute(&mut **transaction)
        .await
        .map_err(|source| {
            ReconciliationRepositoryError::write(
                "insert reconciliation proposed adjustment",
                ADJUSTMENT_ENTITY,
                source,
            )
        })?;

    Ok(())
}

fn decode_row(
    row: &ReconciliationRow,
) -> Result<StoredReconciliation, ReconciliationRepositoryError> {
    let reconciliation_id = ReconciliationId::try_from(row.reconciliation_id).map_err(|_| {
        ReconciliationRepositoryError::invalid("reconciliation_records.reconciliation_id")
    })?;

    let transaction_id = FareTransactionId::try_from(row.fare_transaction_id).map_err(|_| {
        ReconciliationRepositoryError::invalid("reconciliation_records.fare_transaction_id")
    })?;

    let reader_id = ReaderId::try_from(row.reader_id)
        .map_err(|_| ReconciliationRepositoryError::invalid("reconciliation_records.reader_id"))?;

    let source_batch_id = row
        .source_batch_id
        .map(SynchronizationBatchId::try_from)
        .transpose()
        .map_err(|_| {
            ReconciliationRepositoryError::invalid("reconciliation_records.source_batch_id")
        })?;

    let reader_evidence =
        serde_json::from_value::<ReconciliationEvidence>(row.reader_evidence_json.0.clone())
            .map_err(|_| {
                ReconciliationRepositoryError::invalid(
                    "reconciliation_records.reader_evidence_json",
                )
            })?;

    let backend_evidence =
        serde_json::from_value::<ReconciliationEvidence>(row.backend_evidence_json.0.clone())
            .map_err(|_| {
                ReconciliationRepositoryError::invalid(
                    "reconciliation_records.backend_evidence_json",
                )
            })?;

    let reconciled_at = ReconciliationTime::from_unix_milliseconds(
        row.reconciled_at_unix_milliseconds,
    )
    .map_err(|_| {
        ReconciliationRepositoryError::invalid(
            "reconciliation_records.reconciled_at_unix_milliseconds",
        )
    })?;

    let record = ReconciliationRecord::create(
        reconciliation_id,
        transaction_id,
        source_batch_id,
        reader_id,
        reader_evidence,
        backend_evidence,
        reconciled_at,
    )
    .map_err(|_| ReconciliationRepositoryError::invalid("reconciliation_records.evidence"))?;

    validate_denormalized_columns(row, record)?;

    Ok(StoredReconciliation {
        record,
        reader_evidence,
        backend_evidence,
    })
}

fn validate_denormalized_columns(
    row: &ReconciliationRow,
    record: ReconciliationRecord,
) -> Result<(), ReconciliationRepositoryError> {
    if row.reader_evidence_fingerprint != record.reader_evidence_fingerprint().to_string() {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.reader_evidence_fingerprint",
        ));
    }

    if row.backend_evidence_fingerprint != record.backend_evidence_fingerprint().to_string() {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.backend_evidence_fingerprint",
        ));
    }

    if row.reader_policy_id != record.reader_policy_id().into_uuid() {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.reader_policy_id",
        ));
    }

    if row.backend_policy_id != record.backend_policy_id().into_uuid() {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.backend_policy_id",
        ));
    }

    if row.reader_policy_version != policy_version_to_i64(record.reader_policy_version())? {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.reader_policy_version",
        ));
    }

    if row.backend_policy_version != policy_version_to_i64(record.backend_policy_version())? {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.backend_policy_version",
        ));
    }

    if row.outcome != outcome_name(record.outcome()) {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.outcome",
        ));
    }

    if row.status != status_name(record.status()) {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.status",
        ));
    }

    if !stored_money_matches(
        record.observed_amount(),
        row.observed_minor_units,
        row.observed_currency.as_deref(),
    ) {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.observed_amount",
        ));
    }

    if !stored_money_matches(
        record.expected_amount(),
        row.expected_minor_units,
        row.expected_currency.as_deref(),
    ) {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.expected_amount",
        ));
    }

    if !stored_money_matches(
        record.monetary_difference(),
        row.monetary_difference_minor_units,
        row.monetary_difference_currency.as_deref(),
    ) {
        return Err(ReconciliationRepositoryError::invalid(
            "reconciliation_records.monetary_difference",
        ));
    }

    Ok(())
}

fn money_columns(money: Option<Money>) -> (Option<i64>, Option<&'static str>) {
    match money {
        Some(value) => (
            Some(value.minor_units()),
            Some(PostgresValueCodec::encode_currency(value.currency())),
        ),

        None => (None, None),
    }
}

fn stored_money_matches(
    money: Option<Money>,
    minor_units: Option<i64>,
    currency: Option<&str>,
) -> bool {
    match money {
        Some(value) => {
            minor_units == Some(value.minor_units())
                && currency == Some(PostgresValueCodec::encode_currency(value.currency()))
        }

        None => minor_units.is_none() && currency.is_none(),
    }
}

fn policy_version_to_i64(version: FarePolicyVersion) -> Result<i64, ReconciliationRepositoryError> {
    i64::try_from(version.value()).map_err(|_| {
        ReconciliationRepositoryError::Persistence(PersistenceError::NumericValueOutOfRange {
            field: "reconciliation_records.policy_version",
        })
    })
}

const fn outcome_name(outcome: ReconciliationOutcome) -> &'static str {
    match outcome {
        ReconciliationOutcome::Matched => "matched",

        ReconciliationOutcome::FareAmountMismatch => "fare_amount_mismatch",

        ReconciliationOutcome::PolicyVersionMismatch => "policy_version_mismatch",

        ReconciliationOutcome::EligibilityMismatch => "eligibility_mismatch",

        ReconciliationOutcome::ProductMismatch => "product_mismatch",

        ReconciliationOutcome::TransferMismatch => "transfer_mismatch",

        ReconciliationOutcome::FareCapMismatch => "fare_cap_mismatch",

        ReconciliationOutcome::DuplicateTransaction => "duplicate_transaction",

        ReconciliationOutcome::MissingBackendContext => "missing_backend_context",

        ReconciliationOutcome::InvalidEvidence => "invalid_evidence",

        ReconciliationOutcome::ManualReviewRequired => "manual_review_required",
    }
}

const fn status_name(status: ReconciliationStatus) -> &'static str {
    match status {
        ReconciliationStatus::Matched => "matched",
        ReconciliationStatus::Discrepancy => "discrepancy",
        ReconciliationStatus::ManualReview => "manual_review",
    }
}

const fn discrepancy_category_name(category: DiscrepancyCategory) -> &'static str {
    match category {
        DiscrepancyCategory::FareAmountMismatch => "fare_amount_mismatch",

        DiscrepancyCategory::PolicyVersionMismatch => "policy_version_mismatch",

        DiscrepancyCategory::EligibilityMismatch => "eligibility_mismatch",

        DiscrepancyCategory::ProductMismatch => "product_mismatch",

        DiscrepancyCategory::TransferMismatch => "transfer_mismatch",

        DiscrepancyCategory::FareCapMismatch => "fare_cap_mismatch",

        DiscrepancyCategory::DuplicateTransaction => "duplicate_transaction",

        DiscrepancyCategory::MissingBackendContext => "missing_backend_context",

        DiscrepancyCategory::InvalidEvidence => "invalid_evidence",

        DiscrepancyCategory::ManualReviewRequired => "manual_review_required",
    }
}

const fn discrepancy_state_name(state: DiscrepancyState) -> &'static str {
    match state {
        DiscrepancyState::Open => "open",
        DiscrepancyState::ManualReview => "manual_review",
        DiscrepancyState::Resolved => "resolved",
        DiscrepancyState::Dismissed => "dismissed",
    }
}

const fn adjustment_direction_name(direction: ProposedAdjustmentDirection) -> &'static str {
    match direction {
        ProposedAdjustmentDirection::IncreaseRecordedFare => "increase_recorded_fare",

        ProposedAdjustmentDirection::DecreaseRecordedFare => "decrease_recorded_fare",
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{Currency, Money};
    use transitguard_reconciliation::{
        DiscrepancyCategory, DiscrepancyState, ProposedAdjustmentDirection, ReconciliationOutcome,
        ReconciliationStatus,
    };

    use super::{
        adjustment_direction_name, discrepancy_category_name, discrepancy_state_name,
        money_columns, outcome_name, status_name, stored_money_matches,
    };

    #[test]
    fn reconciliation_outcomes_match_database_contract() {
        let values = [
            (ReconciliationOutcome::Matched, "matched"),
            (
                ReconciliationOutcome::FareAmountMismatch,
                "fare_amount_mismatch",
            ),
            (
                ReconciliationOutcome::PolicyVersionMismatch,
                "policy_version_mismatch",
            ),
            (
                ReconciliationOutcome::EligibilityMismatch,
                "eligibility_mismatch",
            ),
            (ReconciliationOutcome::ProductMismatch, "product_mismatch"),
            (ReconciliationOutcome::TransferMismatch, "transfer_mismatch"),
            (ReconciliationOutcome::FareCapMismatch, "fare_cap_mismatch"),
            (
                ReconciliationOutcome::DuplicateTransaction,
                "duplicate_transaction",
            ),
            (
                ReconciliationOutcome::MissingBackendContext,
                "missing_backend_context",
            ),
            (ReconciliationOutcome::InvalidEvidence, "invalid_evidence"),
            (
                ReconciliationOutcome::ManualReviewRequired,
                "manual_review_required",
            ),
        ];

        for (value, expected) in values {
            assert_eq!(outcome_name(value), expected);
        }
    }

    #[test]
    fn lifecycle_values_match_database_contract() {
        assert_eq!(status_name(ReconciliationStatus::Matched), "matched");

        assert_eq!(
            status_name(ReconciliationStatus::Discrepancy),
            "discrepancy"
        );

        assert_eq!(
            status_name(ReconciliationStatus::ManualReview),
            "manual_review"
        );

        assert_eq!(discrepancy_state_name(DiscrepancyState::Open), "open");

        assert_eq!(
            discrepancy_state_name(DiscrepancyState::ManualReview),
            "manual_review"
        );
    }

    #[test]
    fn dependent_record_values_match_database_contract() {
        assert_eq!(
            discrepancy_category_name(DiscrepancyCategory::FareAmountMismatch),
            "fare_amount_mismatch"
        );

        assert_eq!(
            adjustment_direction_name(ProposedAdjustmentDirection::IncreaseRecordedFare),
            "increase_recorded_fare"
        );

        assert_eq!(
            adjustment_direction_name(ProposedAdjustmentDirection::DecreaseRecordedFare),
            "decrease_recorded_fare"
        );
    }

    #[test]
    fn optional_money_columns_are_consistent() {
        let money = Money::from_minor_units(250, Currency::Usd);

        let columns = money_columns(Some(money));

        assert_eq!(columns.0, Some(250));
        assert_eq!(columns.1, Some("USD"));

        assert!(stored_money_matches(Some(money), Some(250), Some("USD")));

        assert!(!stored_money_matches(Some(money), Some(300), Some("USD")));

        assert!(stored_money_matches(None, None, None));
    }
}
