use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_domain::{FareTransactionId, ReaderId, SynchronizationBatchId};

/// Result of applying one durable synchronization acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SynchronizationAcknowledgementApplication {
    batch_id: SynchronizationBatchId,
    applied_now: bool,
    applied_at_unix_milliseconds: i64,
    acknowledged_entries: u64,
    retryable_failure_entries: u64,
    permanent_failure_entries: u64,
    manual_review_entries: u64,
    last_acknowledged_sequence: u64,
}

impl SynchronizationAcknowledgementApplication {
    /// Returns the stable batch identity.
    #[must_use]
    pub const fn batch_id(self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns whether this call performed the durable application.
    #[must_use]
    pub const fn applied_now(self) -> bool {
        self.applied_now
    }

    /// Returns the original durable application time.
    #[must_use]
    pub const fn applied_at_unix_milliseconds(self) -> i64 {
        self.applied_at_unix_milliseconds
    }

    /// Returns the number of accepted entries.
    #[must_use]
    pub const fn acknowledged_entries(self) -> u64 {
        self.acknowledged_entries
    }

    /// Returns the number of retryable entries.
    #[must_use]
    pub const fn retryable_failure_entries(self) -> u64 {
        self.retryable_failure_entries
    }

    /// Returns the number of final rejected entries.
    #[must_use]
    pub const fn permanent_failure_entries(self) -> u64 {
        self.permanent_failure_entries
    }

    /// Returns the number of entries retained for review.
    #[must_use]
    pub const fn manual_review_entries(self) -> u64 {
        self.manual_review_entries
    }

    /// Returns the contiguous resolved reader sequence.
    #[must_use]
    pub const fn last_acknowledged_sequence(self) -> u64 {
        self.last_acknowledged_sequence
    }
}

/// Stable failures produced while applying acknowledgements.
#[derive(Debug, Error)]
pub enum ReaderAcknowledgementApplicationError {
    /// Application times cannot predate the Unix epoch.
    #[error(
        "synchronization acknowledgement application time cannot be negative: {unix_milliseconds}"
    )]
    NegativeApplicationTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },

    /// No durable acknowledgement exists for the requested batch.
    #[error(
        "synchronization acknowledgement for batch {batch_id} was not found for reader {reader_id}"
    )]
    AcknowledgementNotFound {
        /// Requested batch identity.
        batch_id: SynchronizationBatchId,

        /// Expected reader identity.
        reader_id: ReaderId,
    },

    /// Application cannot occur before acknowledgement receipt.
    #[error(
        "synchronization acknowledgement for batch {batch_id} cannot be applied at {applied_at_unix_milliseconds} because it was received at {received_at_unix_milliseconds}"
    )]
    ApplicationBeforeReceipt {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,

        /// Requested application time.
        applied_at_unix_milliseconds: i64,

        /// Durable acknowledgement receipt time.
        received_at_unix_milliseconds: i64,
    },

    /// An unapplied acknowledgement requires an in-flight batch.
    #[error("synchronization batch {batch_id} is not awaiting acknowledgement application")]
    BatchNotInFlight {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// One acknowledgement entry could not transition its queue record.
    #[error(
        "offline transaction {transaction_id} could not apply its synchronization acknowledgement"
    )]
    EntryStateConflict {
        /// Transaction whose durable state did not match.
        transaction_id: FareTransactionId,
    },

    /// The acknowledgement application marker could not be written.
    #[error("synchronization acknowledgement for batch {batch_id} could not be marked as applied")]
    AcknowledgementUpdateConflict {
        /// Stable batch identity.
        batch_id: SynchronizationBatchId,
    },

    /// The reader database was not bound to the expected identity.
    #[error("reader state was not found for reader {reader_id}")]
    ReaderStateNotFound {
        /// Expected reader identity.
        reader_id: ReaderId,
    },

    /// SQLite contained invalid acknowledgement or queue data.
    #[error("reader acknowledgement application contains an invalid value for `{field}`")]
    InvalidStoredValue {
        /// Stable schema field name.
        field: &'static str,
    },

    /// A named SQLite operation failed.
    #[error("reader SQLite acknowledgement-application operation `{operation}` failed")]
    Database {
        /// Stable operation category.
        operation: &'static str,

        /// Original SQLx failure.
        #[source]
        source: sqlx::Error,
    },
}

impl ReaderAcknowledgementApplicationError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Database { operation, source }
    }

    const fn invalid_stored_value(field: &'static str) -> Self {
        Self::InvalidStoredValue { field }
    }
}

#[derive(sqlx::FromRow)]
struct StoredAcknowledgement {
    received_at_unix_milliseconds: i64,
    applied_at_unix_milliseconds: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct StoredAcknowledgementEntry {
    fare_transaction_id: String,
    local_sequence_number: i64,
    entry_position: i64,
    outcome: String,
    failure_category: Option<String>,
    retry_at_unix_milliseconds: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct StoredQueueState {
    local_sequence_number: i64,
    queue_state: String,
}

#[derive(Debug, Eq, PartialEq)]
enum AppliedResolution {
    Acknowledged,

    RetryableFailure {
        failure_category: String,
        retry_at_unix_milliseconds: i64,
    },

    PermanentFailure {
        failure_category: String,
    },

    ManualReview {
        failure_category: String,
    },
}

impl AppliedResolution {
    fn queue_values(&self) -> (&'static str, Option<&str>, Option<i64>) {
        match self {
            Self::Acknowledged => ("acknowledged", None, None),

            Self::RetryableFailure {
                failure_category,
                retry_at_unix_milliseconds,
            } => (
                "retryable_failure",
                Some(failure_category.as_str()),
                Some(*retry_at_unix_milliseconds),
            ),

            Self::PermanentFailure { failure_category } => {
                ("permanent_failure", Some(failure_category.as_str()), None)
            }

            Self::ManualReview { failure_category } => {
                ("manual_review", Some(failure_category.as_str()), None)
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct DecodedAcknowledgementEntry {
    transaction_id: FareTransactionId,
    local_sequence_number: i64,
    resolution: AppliedResolution,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ResolutionCounts {
    acknowledged: u64,
    retryable_failure: u64,
    permanent_failure: u64,
    manual_review: u64,
}

impl ResolutionCounts {
    fn record(&mut self, resolution: &AppliedResolution) {
        match resolution {
            AppliedResolution::Acknowledged => {
                self.acknowledged += 1;
            }

            AppliedResolution::RetryableFailure { .. } => {
                self.retryable_failure += 1;
            }

            AppliedResolution::PermanentFailure { .. } => {
                self.permanent_failure += 1;
            }

            AppliedResolution::ManualReview { .. } => {
                self.manual_review += 1;
            }
        }
    }
}

/// Atomically applies one previously stored acknowledgement.
///
/// Queue outcomes, batch completion, acknowledgement application, and
/// contiguous reader sequence advancement commit in one SQLite
/// transaction.
///
/// Calling this function again after a successful application returns an
/// idempotent replay result without changing durable state.
pub async fn apply_synchronization_acknowledgement(
    pool: &SqlitePool,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    applied_at_unix_milliseconds: i64,
) -> Result<SynchronizationAcknowledgementApplication, ReaderAcknowledgementApplicationError> {
    if applied_at_unix_milliseconds < 0 {
        return Err(
            ReaderAcknowledgementApplicationError::NegativeApplicationTime {
                unix_milliseconds: applied_at_unix_milliseconds,
            },
        );
    }

    let mut transaction = pool.begin().await.map_err(|source| {
        ReaderAcknowledgementApplicationError::database("begin acknowledgement application", source)
    })?;

    let acknowledgement = sqlx::query_as::<_, StoredAcknowledgement>(
        r#"
            SELECT
                received_at_unix_milliseconds,
                applied_at_unix_milliseconds
            FROM synchronization_acknowledgements
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
        ReaderAcknowledgementApplicationError::database("load acknowledgement", source)
    })?
    .ok_or(
        ReaderAcknowledgementApplicationError::AcknowledgementNotFound {
            batch_id,
            reader_id,
        },
    )?;

    if acknowledgement.received_at_unix_milliseconds < 0 {
        return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
            "received_at_unix_milliseconds",
        ));
    }

    let stored_entries = sqlx::query_as::<_, StoredAcknowledgementEntry>(
        r#"
            SELECT
                fare_transaction_id,
                local_sequence_number,
                entry_position,
                outcome,
                failure_category,
                retry_at_unix_milliseconds
            FROM synchronization_acknowledgement_entries
            WHERE
                batch_id = ?
                AND reader_id = ?
            ORDER BY entry_position
            "#,
    )
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .fetch_all(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementApplicationError::database("load acknowledgement entries", source)
    })?;

    let entries = decode_entries(
        stored_entries,
        acknowledgement.received_at_unix_milliseconds,
    )?;

    if entries.is_empty() {
        return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
            "acknowledgement_entries",
        ));
    }

    let counts = resolution_counts(&entries);

    let stored_last_acknowledged_sequence = sqlx::query_scalar::<_, i64>(
        r#"
            SELECT last_acknowledged_sequence
            FROM reader_state
            WHERE
                singleton = 1
                AND reader_id = ?
            "#,
    )
    .bind(reader_id.to_string())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementApplicationError::database("load reader sequence state", source)
    })?
    .ok_or(ReaderAcknowledgementApplicationError::ReaderStateNotFound { reader_id })?;

    let current_last_acknowledged_sequence = u64::try_from(stored_last_acknowledged_sequence)
        .map_err(|_| {
            ReaderAcknowledgementApplicationError::invalid_stored_value(
                "last_acknowledged_sequence",
            )
        })?;

    if let Some(stored_applied_at_unix_milliseconds) = acknowledgement.applied_at_unix_milliseconds
    {
        if stored_applied_at_unix_milliseconds < 0 {
            return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                "applied_at_unix_milliseconds",
            ));
        }

        transaction.commit().await.map_err(|source| {
            ReaderAcknowledgementApplicationError::database("finish acknowledgement replay", source)
        })?;

        return Ok(application_report(
            batch_id,
            false,
            stored_applied_at_unix_milliseconds,
            counts,
            current_last_acknowledged_sequence,
        ));
    }

    if applied_at_unix_milliseconds < acknowledgement.received_at_unix_milliseconds {
        return Err(
            ReaderAcknowledgementApplicationError::ApplicationBeforeReceipt {
                batch_id,
                applied_at_unix_milliseconds,
                received_at_unix_milliseconds: acknowledgement.received_at_unix_milliseconds,
            },
        );
    }

    let batch_state = sqlx::query_scalar::<_, String>(
        r#"
            SELECT batch_state
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
        ReaderAcknowledgementApplicationError::database("load batch application state", source)
    })?;

    if batch_state.as_deref() != Some("in_flight") {
        return Err(ReaderAcknowledgementApplicationError::BatchNotInFlight { batch_id });
    }

    for entry in &entries {
        let (queue_state, failure_category, retry_at_unix_milliseconds) =
            entry.resolution.queue_values();

        let result = sqlx::query(
            r#"
            UPDATE offline_transactions
            SET
                queue_state = ?,
                next_retry_at_unix_milliseconds = ?,
                last_failure_category = ?,
                updated_at_unix_milliseconds = ?
            WHERE
                fare_transaction_id = ?
                AND reader_id = ?
                AND local_sequence_number = ?
                AND queue_state = 'in_flight'
                AND updated_at_unix_milliseconds <= ?
            "#,
        )
        .bind(queue_state)
        .bind(retry_at_unix_milliseconds)
        .bind(failure_category)
        .bind(applied_at_unix_milliseconds)
        .bind(entry.transaction_id.to_string())
        .bind(reader_id.to_string())
        .bind(entry.local_sequence_number)
        .bind(applied_at_unix_milliseconds)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            ReaderAcknowledgementApplicationError::database("apply acknowledgement entry", source)
        })?;

        if result.rows_affected() != 1 {
            return Err(ReaderAcknowledgementApplicationError::EntryStateConflict {
                transaction_id: entry.transaction_id,
            });
        }
    }

    let batch_update = sqlx::query(
        r#"
        UPDATE synchronization_batches
        SET
            batch_state = 'acknowledged',
            next_retry_at_unix_milliseconds = NULL,
            last_failure_category = NULL,
            updated_at_unix_milliseconds = ?
        WHERE
            batch_id = ?
            AND reader_id = ?
            AND batch_state = 'in_flight'
            AND updated_at_unix_milliseconds <= ?
        "#,
    )
    .bind(applied_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .bind(applied_at_unix_milliseconds)
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementApplicationError::database("complete acknowledged batch", source)
    })?;

    if batch_update.rows_affected() != 1 {
        return Err(ReaderAcknowledgementApplicationError::BatchNotInFlight { batch_id });
    }

    let queue_states = sqlx::query_as::<_, StoredQueueState>(
        r#"
            SELECT
                local_sequence_number,
                queue_state
            FROM offline_transactions
            WHERE
                reader_id = ?
                AND local_sequence_number > ?
            ORDER BY local_sequence_number
            "#,
    )
    .bind(reader_id.to_string())
    .bind(stored_last_acknowledged_sequence)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementApplicationError::database("load contiguous queue state", source)
    })?;

    let advanced_last_acknowledged_sequence =
        contiguous_resolved_sequence(current_last_acknowledged_sequence, queue_states)?;

    if advanced_last_acknowledged_sequence > current_last_acknowledged_sequence {
        let stored_advanced_sequence =
            i64::try_from(advanced_last_acknowledged_sequence).map_err(|_| {
                ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "last_acknowledged_sequence",
                )
            })?;

        let reader_update = sqlx::query(
            r#"
            UPDATE reader_state
            SET
                last_acknowledged_sequence = ?,
                updated_at_unix_milliseconds =
                    MAX(
                        updated_at_unix_milliseconds,
                        ?
                    )
            WHERE
                singleton = 1
                AND reader_id = ?
                AND last_acknowledged_sequence <= ?
                AND next_local_sequence > ?
            "#,
        )
        .bind(stored_advanced_sequence)
        .bind(applied_at_unix_milliseconds)
        .bind(reader_id.to_string())
        .bind(stored_advanced_sequence)
        .bind(stored_advanced_sequence)
        .execute(&mut *transaction)
        .await
        .map_err(|source| {
            ReaderAcknowledgementApplicationError::database("advance acknowledged sequence", source)
        })?;

        if reader_update.rows_affected() != 1 {
            return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                "last_acknowledged_sequence",
            ));
        }
    }

    let acknowledgement_update = sqlx::query(
        r#"
        UPDATE synchronization_acknowledgements
        SET applied_at_unix_milliseconds = ?
        WHERE
            batch_id = ?
            AND reader_id = ?
            AND applied_at_unix_milliseconds IS NULL
        "#,
    )
    .bind(applied_at_unix_milliseconds)
    .bind(batch_id.to_string())
    .bind(reader_id.to_string())
    .execute(&mut *transaction)
    .await
    .map_err(|source| {
        ReaderAcknowledgementApplicationError::database("mark acknowledgement applied", source)
    })?;

    if acknowledgement_update.rows_affected() != 1 {
        return Err(
            ReaderAcknowledgementApplicationError::AcknowledgementUpdateConflict { batch_id },
        );
    }

    transaction.commit().await.map_err(|source| {
        ReaderAcknowledgementApplicationError::database(
            "commit acknowledgement application",
            source,
        )
    })?;

    Ok(application_report(
        batch_id,
        true,
        applied_at_unix_milliseconds,
        counts,
        advanced_last_acknowledged_sequence,
    ))
}

fn decode_entries(
    stored_entries: Vec<StoredAcknowledgementEntry>,
    received_at_unix_milliseconds: i64,
) -> Result<Vec<DecodedAcknowledgementEntry>, ReaderAcknowledgementApplicationError> {
    let mut entries = Vec::with_capacity(stored_entries.len());

    for (expected_position, stored_entry) in stored_entries.into_iter().enumerate() {
        let actual_position = usize::try_from(stored_entry.entry_position).map_err(|_| {
            ReaderAcknowledgementApplicationError::invalid_stored_value("entry_position")
        })?;

        if actual_position != expected_position {
            return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                "entry_position",
            ));
        }

        let transaction_id = stored_entry
            .fare_transaction_id
            .parse::<FareTransactionId>()
            .map_err(|_| {
                ReaderAcknowledgementApplicationError::invalid_stored_value("fare_transaction_id")
            })?;

        let sequence = u64::try_from(stored_entry.local_sequence_number).map_err(|_| {
            ReaderAcknowledgementApplicationError::invalid_stored_value("local_sequence_number")
        })?;

        if sequence == 0 {
            return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                "local_sequence_number",
            ));
        }

        let resolution = decode_resolution(
            stored_entry.outcome.as_str(),
            stored_entry.failure_category,
            stored_entry.retry_at_unix_milliseconds,
            received_at_unix_milliseconds,
        )?;

        entries.push(DecodedAcknowledgementEntry {
            transaction_id,
            local_sequence_number: stored_entry.local_sequence_number,
            resolution,
        });
    }

    Ok(entries)
}

fn decode_resolution(
    outcome: &str,
    failure_category: Option<String>,
    retry_at_unix_milliseconds: Option<i64>,
    received_at_unix_milliseconds: i64,
) -> Result<AppliedResolution, ReaderAcknowledgementApplicationError> {
    match outcome {
        "acknowledged" => {
            if failure_category.is_some() || retry_at_unix_milliseconds.is_some() {
                return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "acknowledgement_outcome",
                ));
            }

            Ok(AppliedResolution::Acknowledged)
        }

        "retryable_failure" => {
            let failure_category = validated_failure_category(failure_category)?;

            let retry_at_unix_milliseconds = retry_at_unix_milliseconds.ok_or_else(|| {
                ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "retry_at_unix_milliseconds",
                )
            })?;

            if retry_at_unix_milliseconds < received_at_unix_milliseconds {
                return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "retry_at_unix_milliseconds",
                ));
            }

            Ok(AppliedResolution::RetryableFailure {
                failure_category,
                retry_at_unix_milliseconds,
            })
        }

        "permanent_failure" => {
            if retry_at_unix_milliseconds.is_some() {
                return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "retry_at_unix_milliseconds",
                ));
            }

            Ok(AppliedResolution::PermanentFailure {
                failure_category: validated_failure_category(failure_category)?,
            })
        }

        "manual_review" => {
            if retry_at_unix_milliseconds.is_some() {
                return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                    "retry_at_unix_milliseconds",
                ));
            }

            Ok(AppliedResolution::ManualReview {
                failure_category: validated_failure_category(failure_category)?,
            })
        }

        _ => Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
            "outcome",
        )),
    }
}

fn validated_failure_category(
    failure_category: Option<String>,
) -> Result<String, ReaderAcknowledgementApplicationError> {
    let failure_category = failure_category.ok_or_else(|| {
        ReaderAcknowledgementApplicationError::invalid_stored_value("failure_category")
    })?;

    let normalized = failure_category.trim();

    if normalized.is_empty() {
        return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
            "failure_category",
        ));
    }

    Ok(normalized.to_owned())
}

fn resolution_counts(entries: &[DecodedAcknowledgementEntry]) -> ResolutionCounts {
    let mut counts = ResolutionCounts::default();

    for entry in entries {
        counts.record(&entry.resolution);
    }

    counts
}

fn contiguous_resolved_sequence(
    current_last_acknowledged_sequence: u64,
    queue_states: Vec<StoredQueueState>,
) -> Result<u64, ReaderAcknowledgementApplicationError> {
    let mut last_acknowledged_sequence = current_last_acknowledged_sequence;

    let mut expected_sequence = current_last_acknowledged_sequence
        .checked_add(1)
        .ok_or_else(|| {
            ReaderAcknowledgementApplicationError::invalid_stored_value(
                "last_acknowledged_sequence",
            )
        })?;

    for queue_state in queue_states {
        let sequence = u64::try_from(queue_state.local_sequence_number).map_err(|_| {
            ReaderAcknowledgementApplicationError::invalid_stored_value("local_sequence_number")
        })?;

        if sequence == 0 {
            return Err(ReaderAcknowledgementApplicationError::invalid_stored_value(
                "local_sequence_number",
            ));
        }

        if sequence != expected_sequence {
            break;
        }

        if !is_resolved_queue_state(queue_state.queue_state.as_str()) {
            break;
        }

        last_acknowledged_sequence = sequence;

        expected_sequence = sequence.checked_add(1).ok_or_else(|| {
            ReaderAcknowledgementApplicationError::invalid_stored_value(
                "last_acknowledged_sequence",
            )
        })?;
    }

    Ok(last_acknowledged_sequence)
}

fn is_resolved_queue_state(queue_state: &str) -> bool {
    matches!(queue_state, "acknowledged" | "permanent_failure")
}

fn application_report(
    batch_id: SynchronizationBatchId,
    applied_now: bool,
    applied_at_unix_milliseconds: i64,
    counts: ResolutionCounts,
    last_acknowledged_sequence: u64,
) -> SynchronizationAcknowledgementApplication {
    SynchronizationAcknowledgementApplication {
        batch_id,
        applied_now,
        applied_at_unix_milliseconds,
        acknowledged_entries: counts.acknowledged,
        retryable_failure_entries: counts.retryable_failure,
        permanent_failure_entries: counts.permanent_failure,
        manual_review_entries: counts.manual_review,
        last_acknowledged_sequence,
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
        OfflineQueueState, OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig,
        SynchronizationAcknowledgement, SynchronizationAcknowledgementEntry, SynchronizationBatch,
        SynchronizationBatchState, SynchronizationEntryResolution, bind_reader_database,
        connect_reader_sqlite, create_synchronization_batch, enqueue_offline_transaction,
        load_offline_queue, load_synchronization_batch, mark_synchronization_batch_in_flight,
        run_reader_sqlite_migrations, store_synchronization_acknowledgement,
    };

    use super::{ReaderAcknowledgementApplicationError, apply_synchronization_acknowledgement};

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-ack-application-{name}-{}.sqlite3",
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
        created_at_unix_milliseconds: i64,
        attempted_at_unix_milliseconds: i64,
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
            created_at_unix_milliseconds,
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
            attempted_at_unix_milliseconds,
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
        received_at_unix_milliseconds: i64,
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
            received_at_unix_milliseconds,
            entries,
        ) {
            Ok(value) => value,

            Err(error) => {
                panic!("acknowledgement creation failed: {error}")
            }
        }
    }

    async fn store_acknowledgement(
        pool: &SqlitePool,
        acknowledgement: &SynchronizationAcknowledgement,
    ) {
        if let Err(error) = store_synchronization_acknowledgement(pool, acknowledgement).await {
            panic!("acknowledgement storage failed: {error}");
        }
    }

    #[tokio::test]
    async fn partial_outcomes_apply_atomically() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("partial-outcomes", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 4, TEST_TIME + 200, TEST_TIME + 300).await;

        let value = acknowledgement(
            &batch,
            TEST_TIME + 400,
            vec![
                SynchronizationEntryResolution::Acknowledged,
                SynchronizationEntryResolution::RetryableFailure {
                    failure_category: String::from("backend_timeout"),
                    retry_at_unix_milliseconds: TEST_TIME + 1_000,
                },
                SynchronizationEntryResolution::PermanentFailure {
                    failure_category: String::from("invalid_envelope"),
                },
                SynchronizationEntryResolution::ManualReview {
                    failure_category: String::from("sequence_investigation"),
                },
            ],
        );

        store_acknowledgement(&pool, &value).await;

        let report = match apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 500,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("acknowledgement application failed: {error}")
            }
        };

        assert!(report.applied_now());
        assert_eq!(report.acknowledged_entries(), 1);
        assert_eq!(report.retryable_failure_entries(), 1);
        assert_eq!(report.permanent_failure_entries(), 1);
        assert_eq!(report.manual_review_entries(), 1);
        assert_eq!(report.last_acknowledged_sequence(), 1);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue.len(), 4);

        assert_eq!(queue[0].queue_state(), OfflineQueueState::Acknowledged);

        assert_eq!(queue[1].queue_state(), OfflineQueueState::RetryableFailure);

        assert_eq!(
            queue[1].next_retry_at_unix_milliseconds(),
            Some(TEST_TIME + 1_000)
        );

        assert_eq!(queue[1].last_failure_category(), Some("backend_timeout"));

        assert_eq!(queue[2].queue_state(), OfflineQueueState::PermanentFailure);

        assert_eq!(queue[2].last_failure_category(), Some("invalid_envelope"));

        assert_eq!(queue[3].queue_state(), OfflineQueueState::ManualReview);

        assert_eq!(
            queue[3].last_failure_category(),
            Some("sequence_investigation")
        );

        let loaded_batch =
            match load_synchronization_batch(&pool, reader_id, batch.batch_id()).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("batch load failed: {error}")
                }
            };

        assert_eq!(
            loaded_batch.state(),
            SynchronizationBatchState::Acknowledged
        );

        let applied_at = match sqlx::query_scalar::<_, Option<i64>>(
            r#"
                SELECT applied_at_unix_milliseconds
                FROM synchronization_acknowledgements
                WHERE
                    batch_id = ?
                    AND reader_id = ?
                "#,
        )
        .bind(batch.batch_id().to_string())
        .bind(reader_id.to_string())
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("application marker load failed: {error}")
            }
        };

        assert_eq!(applied_at, Some(TEST_TIME + 500));

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn acknowledgement_application_replay_is_idempotent() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("idempotent", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1, TEST_TIME + 200, TEST_TIME + 300).await;

        let value = acknowledgement(
            &batch,
            TEST_TIME + 400,
            vec![SynchronizationEntryResolution::Acknowledged],
        );

        store_acknowledgement(&pool, &value).await;

        let first = apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 500,
        )
        .await;

        assert!(matches!(
            first,
            Ok(report) if report.applied_now()
        ));

        let replay = match apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 600,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("application replay failed: {error}")
            }
        };

        assert!(!replay.applied_now());

        assert_eq!(replay.applied_at_unix_milliseconds(), TEST_TIME + 500);

        assert_eq!(replay.last_acknowledged_sequence(), 1);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue.len(), 1);

        assert_eq!(queue[0].queue_state(), OfflineQueueState::Acknowledged);

        assert_eq!(queue[0].attempt_count(), 1);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn application_before_receipt_rolls_back() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("before-receipt", reader_id).await;

        let batch = submitted_batch(&pool, reader_id, 1, TEST_TIME + 200, TEST_TIME + 300).await;

        let value = acknowledgement(
            &batch,
            TEST_TIME + 600,
            vec![SynchronizationEntryResolution::Acknowledged],
        );

        store_acknowledgement(&pool, &value).await;

        let result = apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            batch.batch_id(),
            TEST_TIME + 500,
        )
        .await;

        assert!(matches!(
            result,
            Err(
                ReaderAcknowledgementApplicationError::
                    ApplicationBeforeReceipt {
                        batch_id,
                        ..
                    }
            ) if batch_id == batch.batch_id()
        ));

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("queue load failed: {error}")
            }
        };

        assert_eq!(queue[0].queue_state(), OfflineQueueState::InFlight);

        let loaded_batch =
            match load_synchronization_batch(&pool, reader_id, batch.batch_id()).await {
                Ok(value) => value,

                Err(error) => {
                    pool.close().await;
                    remove_database(&path);

                    panic!("batch load failed: {error}")
                }
            };

        assert_eq!(loaded_batch.state(), SynchronizationBatchState::InFlight);

        let applied_at = match sqlx::query_scalar::<_, Option<i64>>(
            r#"
                SELECT applied_at_unix_milliseconds
                FROM synchronization_acknowledgements
                WHERE
                    batch_id = ?
                    AND reader_id = ?
                "#,
        )
        .bind(batch.batch_id().to_string())
        .bind(reader_id.to_string())
        .fetch_one(&pool)
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("application marker load failed: {error}")
            }
        };

        assert_eq!(applied_at, None);

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn later_retry_resolution_advances_contiguous_sequence() {
        let reader_id = ReaderId::generate();

        let (path, pool) = open_database("contiguous-sequence", reader_id).await;

        let first_batch =
            submitted_batch(&pool, reader_id, 2, TEST_TIME + 200, TEST_TIME + 300).await;

        let first_transaction = first_batch.entries()[0].transaction_id();

        let first_acknowledgement = acknowledgement(
            &first_batch,
            TEST_TIME + 400,
            vec![
                SynchronizationEntryResolution::RetryableFailure {
                    failure_category: String::from("backend_timeout"),
                    retry_at_unix_milliseconds: TEST_TIME + 700,
                },
                SynchronizationEntryResolution::Acknowledged,
            ],
        );

        store_acknowledgement(&pool, &first_acknowledgement).await;

        let first_report = match apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            first_batch.batch_id(),
            TEST_TIME + 500,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("first application failed: {error}")
            }
        };

        assert_eq!(first_report.last_acknowledged_sequence(), 0);

        let retry_prepared = match create_synchronization_batch(
            &pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 700,
            10,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("retry batch creation failed: {error}")
            }
        };

        assert_eq!(retry_prepared.entries().len(), 1);

        assert_eq!(
            retry_prepared.entries()[0].transaction_id(),
            first_transaction
        );

        let retry_batch = match mark_synchronization_batch_in_flight(
            &pool,
            reader_id,
            retry_prepared.batch_id(),
            TEST_TIME + 800,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("retry batch submission failed: {error}")
            }
        };

        let retry_acknowledgement = acknowledgement(
            &retry_batch,
            TEST_TIME + 900,
            vec![SynchronizationEntryResolution::Acknowledged],
        );

        store_acknowledgement(&pool, &retry_acknowledgement).await;

        let retry_report = match apply_synchronization_acknowledgement(
            &pool,
            reader_id,
            retry_batch.batch_id(),
            TEST_TIME + 1_000,
        )
        .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                remove_database(&path);

                panic!("retry application failed: {error}")
            }
        };

        assert_eq!(retry_report.last_acknowledged_sequence(), 2);

        pool.close().await;
        remove_database(&path);
    }
}
