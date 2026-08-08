use core::fmt;

use sqlx::{FromRow, PgPool};
use thiserror::Error;
use transitguard_domain::{FareTransactionId, ReaderId, SynchronizationBatchId};
use uuid::{Uuid, Variant, Version};

use crate::PersistenceError;

/// Maximum number of reconciliation work items one operation may claim or
/// recover.
///
/// The bound prevents an accidental worker configuration from locking an
/// unbounded portion of the queue in one database operation.
pub const MAX_RECONCILIATION_WORK_BATCH_SIZE: u16 = 128;

const ENQUEUE_READY_SQL: &str = r#"
WITH ready AS (
    SELECT
        source.fare_transaction_id,
        source.reader_id,
        source.first_seen_batch_id
    FROM synchronization_ingest_transactions AS source
    LEFT JOIN reconciliation_records AS reconciliation
        ON reconciliation.fare_transaction_id
            = source.fare_transaction_id
    LEFT JOIN reconciliation_work_items AS queued
        ON queued.fare_transaction_id
            = source.fare_transaction_id
    WHERE
        source.current_resolution = 'acknowledged'
        AND reconciliation.fare_transaction_id IS NULL
        AND queued.fare_transaction_id IS NULL
    ORDER BY
        source.first_received_at_unix_milliseconds,
        source.fare_transaction_id
    LIMIT $1
)
INSERT INTO reconciliation_work_items (
    fare_transaction_id,
    reader_id,
    source_batch_id,
    state,
    attempt_count,
    available_at_unix_milliseconds
)
SELECT
    fare_transaction_id,
    reader_id,
    first_seen_batch_id,
    'pending',
    0,
    $2
FROM ready
ON CONFLICT (fare_transaction_id) DO NOTHING
"#;

const RECOVER_EXPIRED_SQL: &str = r#"
WITH expired AS (
    SELECT fare_transaction_id
    FROM reconciliation_work_items
    WHERE
        state = 'in_progress'
        AND lease_expires_at_unix_milliseconds <= $1
    ORDER BY
        lease_expires_at_unix_milliseconds,
        fare_transaction_id
    FOR UPDATE SKIP LOCKED
    LIMIT $2
)
UPDATE reconciliation_work_items AS work
SET
    state = 'pending',
    available_at_unix_milliseconds = $1,
    lease_owner_id = NULL,
    claimed_at_unix_milliseconds = NULL,
    lease_expires_at_unix_milliseconds = NULL,
    updated_at = CURRENT_TIMESTAMP
FROM expired
WHERE
    work.fare_transaction_id
        = expired.fare_transaction_id
"#;

const CLAIM_READY_SQL: &str = r#"
WITH candidate AS (
    SELECT fare_transaction_id
    FROM reconciliation_work_items
    WHERE
        state = 'pending'
        AND available_at_unix_milliseconds <= $1
    ORDER BY
        available_at_unix_milliseconds,
        fare_transaction_id
    FOR UPDATE SKIP LOCKED
    LIMIT $2
)
UPDATE reconciliation_work_items AS work
SET
    state = 'in_progress',
    attempt_count = work.attempt_count + 1,
    lease_owner_id = $3,
    claimed_at_unix_milliseconds = $1,
    lease_expires_at_unix_milliseconds = $4,
    updated_at = CURRENT_TIMESTAMP
FROM candidate
WHERE
    work.fare_transaction_id
        = candidate.fare_transaction_id
RETURNING
    work.fare_transaction_id,
    work.reader_id,
    work.source_batch_id,
    work.attempt_count,
    work.claimed_at_unix_milliseconds,
    work.lease_expires_at_unix_milliseconds
"#;

const RENEW_LEASE_SQL: &str = r#"
UPDATE reconciliation_work_items
SET
    lease_expires_at_unix_milliseconds = $4,
    updated_at = CURRENT_TIMESTAMP
WHERE
    fare_transaction_id = $1
    AND state = 'in_progress'
    AND lease_owner_id = $2
    AND lease_expires_at_unix_milliseconds > $3
"#;

const RETRY_SQL: &str = r#"
UPDATE reconciliation_work_items
SET
    state = 'pending',
    available_at_unix_milliseconds = $4,
    lease_owner_id = NULL,
    claimed_at_unix_milliseconds = NULL,
    lease_expires_at_unix_milliseconds = NULL,
    updated_at = CURRENT_TIMESTAMP
WHERE
    fare_transaction_id = $1
    AND state = 'in_progress'
    AND lease_owner_id = $2
    AND lease_expires_at_unix_milliseconds > $3
"#;

const COMPLETE_SQL: &str = r#"
UPDATE reconciliation_work_items
SET
    state = 'completed',
    lease_owner_id = NULL,
    claimed_at_unix_milliseconds = NULL,
    lease_expires_at_unix_milliseconds = NULL,
    completed_at_unix_milliseconds = $3,
    updated_at = CURRENT_TIMESTAMP
WHERE
    fare_transaction_id = $1
    AND state = 'in_progress'
    AND lease_owner_id = $2
    AND lease_expires_at_unix_milliseconds > $3
"#;

/// Errors produced while validating a reconciliation-worker identity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReconciliationWorkerIdError {
    /// Worker identities cannot use the all-zero UUID.
    #[error("reconciliation worker identifier cannot use the nil UUID")]
    Nil,

    /// Worker identities require the RFC UUID variant.
    #[error("reconciliation worker identifier must use the RFC 9562 UUID variant")]
    InvalidVariant,

    /// Worker identities use time-sortable UUID version 7.
    #[error("reconciliation worker identifier must use UUID version 7")]
    UnsupportedVersion,
}

/// Opaque project-owned identity of one reconciliation worker process.
///
/// The identifier contains no hostname, account name, credential, or other
/// operator information.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReconciliationWorkerId(Uuid);

impl ReconciliationWorkerId {
    /// Generates a new project-owned worker identity.
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    fn validate(value: Uuid) -> Result<Self, ReconciliationWorkerIdError> {
        if value.is_nil() {
            return Err(ReconciliationWorkerIdError::Nil);
        }

        if value.get_variant() != Variant::RFC4122 {
            return Err(ReconciliationWorkerIdError::InvalidVariant);
        }

        if value.get_version() != Some(Version::SortRand) {
            return Err(ReconciliationWorkerIdError::UnsupportedVersion);
        }

        Ok(Self(value))
    }
}

impl TryFrom<Uuid> for ReconciliationWorkerId {
    type Error = ReconciliationWorkerIdError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        Self::validate(value)
    }
}

impl fmt::Display for ReconciliationWorkerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One durable work item currently owned by a reconciliation worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaimedReconciliationWork {
    transaction_id: FareTransactionId,
    reader_id: ReaderId,
    source_batch_id: SynchronizationBatchId,
    attempt_count: u32,
    claimed_at_unix_milliseconds: i64,
    lease_expires_at_unix_milliseconds: i64,
}

impl ClaimedReconciliationWork {
    /// Returns the synchronized transaction to reconcile.
    #[must_use]
    pub const fn transaction_id(self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader that produced the source transaction.
    #[must_use]
    pub const fn reader_id(self) -> ReaderId {
        self.reader_id
    }

    /// Returns the synchronization batch that first supplied the transaction.
    #[must_use]
    pub const fn source_batch_id(self) -> SynchronizationBatchId {
        self.source_batch_id
    }

    /// Returns the durable processing attempt number.
    #[must_use]
    pub const fn attempt_count(self) -> u32 {
        self.attempt_count
    }

    /// Returns when the current lease was acquired.
    #[must_use]
    pub const fn claimed_at_unix_milliseconds(self) -> i64 {
        self.claimed_at_unix_milliseconds
    }

    /// Returns when the current lease expires.
    #[must_use]
    pub const fn lease_expires_at_unix_milliseconds(self) -> i64 {
        self.lease_expires_at_unix_milliseconds
    }
}

/// Stable failures exposed by the reconciliation work-queue boundary.
#[derive(Debug, Error)]
pub enum ReconciliationWorkQueueError {
    /// Queue operations require a positive bounded batch size.
    #[error("reconciliation work batch size must be greater than zero")]
    ZeroBatchSize,

    /// Requested batch exceeded the database-locking safety bound.
    #[error("reconciliation work batch size {requested} exceeds maximum {maximum}")]
    BatchSizeTooLarge {
        /// Requested number of work items.
        requested: u16,

        /// Maximum permitted number.
        maximum: u16,
    },

    /// A caller supplied a timestamp before the Unix epoch.
    #[error("reconciliation work timestamp `{field}` cannot be negative")]
    NegativeTimestamp {
        /// Stable input field.
        field: &'static str,
    },

    /// Lease duration must be positive.
    #[error("reconciliation work lease duration must be greater than zero")]
    InvalidLeaseDuration,

    /// Computing the lease expiration exceeded the timestamp representation.
    #[error("reconciliation work lease expiration overflowed")]
    LeaseExpirationOverflow,

    /// Retry availability cannot precede the operation time.
    #[error("reconciliation retry availability cannot precede the current operation time")]
    RetryBeforeCurrentTime,

    /// The caller no longer owns a live lease for the work item.
    #[error("reconciliation work lease was lost for transaction {transaction_id}")]
    LeaseLost {
        /// Transaction whose lease is no longer owned.
        transaction_id: FareTransactionId,
    },

    /// PostgreSQL returned a row that violates the queue model.
    #[error("stored reconciliation work contains an invalid value for `{field}`")]
    InvalidStoredWork {
        /// Stable schema field.
        field: &'static str,
    },

    /// Underlying persistence operation failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl ReconciliationWorkQueueError {
    fn database(operation: &'static str, source: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::database(operation, source))
    }

    const fn invalid(field: &'static str) -> Self {
        Self::InvalidStoredWork { field }
    }
}

/// PostgreSQL-backed durable reconciliation work queue.
#[derive(Clone, Debug)]
pub struct PostgresReconciliationWorkQueue {
    pool: PgPool,
}

impl PostgresReconciliationWorkQueue {
    /// Creates a reconciliation work queue.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying PostgreSQL pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Adds acknowledged synchronized transactions that do not yet have
    /// reconciliation state.
    ///
    /// Bootstrap is bounded and idempotent. Existing work items and already
    /// reconciled transactions are not duplicated.
    pub async fn enqueue_ready(
        &self,
        now_unix_milliseconds: i64,
        limit: u16,
    ) -> Result<u64, ReconciliationWorkQueueError> {
        validate_timestamp(now_unix_milliseconds, "now_unix_milliseconds")?;

        let limit = validate_limit(limit)?;

        let result = sqlx::query(ENQUEUE_READY_SQL)
            .bind(limit)
            .bind(now_unix_milliseconds)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database("enqueue reconciliation work", source)
            })?;

        Ok(result.rows_affected())
    }

    /// Returns expired claims to the pending state.
    ///
    /// Recovery is bounded and uses `SKIP LOCKED`, allowing multiple worker
    /// processes to perform restart recovery concurrently without selecting
    /// the same rows.
    pub async fn recover_expired(
        &self,
        now_unix_milliseconds: i64,
        limit: u16,
    ) -> Result<u64, ReconciliationWorkQueueError> {
        validate_timestamp(now_unix_milliseconds, "now_unix_milliseconds")?;

        let limit = validate_limit(limit)?;

        let result = sqlx::query(RECOVER_EXPIRED_SQL)
            .bind(now_unix_milliseconds)
            .bind(limit)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database(
                    "recover expired reconciliation work",
                    source,
                )
            })?;

        Ok(result.rows_affected())
    }

    /// Claims a bounded batch of currently available work.
    ///
    /// PostgreSQL row locking with `FOR UPDATE SKIP LOCKED` prevents
    /// concurrent workers from claiming the same pending item.
    pub async fn claim_ready(
        &self,
        worker_id: ReconciliationWorkerId,
        now_unix_milliseconds: i64,
        lease_duration_milliseconds: i64,
        limit: u16,
    ) -> Result<Vec<ClaimedReconciliationWork>, ReconciliationWorkQueueError> {
        validate_timestamp(now_unix_milliseconds, "now_unix_milliseconds")?;

        let lease_expires_at =
            lease_expiration(now_unix_milliseconds, lease_duration_milliseconds)?;

        let limit = validate_limit(limit)?;

        let rows = sqlx::query_as::<_, WorkRow>(CLAIM_READY_SQL)
            .bind(now_unix_milliseconds)
            .bind(limit)
            .bind(*worker_id.as_uuid())
            .bind(lease_expires_at)
            .fetch_all(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database("claim reconciliation work", source)
            })?;

        rows.iter().map(decode_work_row).collect()
    }

    /// Renews an actively owned lease.
    ///
    /// A lease that already expired, completed, retried, or moved to another
    /// worker cannot be resurrected by a stale worker.
    pub async fn renew_lease(
        &self,
        transaction_id: FareTransactionId,
        worker_id: ReconciliationWorkerId,
        now_unix_milliseconds: i64,
        lease_duration_milliseconds: i64,
    ) -> Result<(), ReconciliationWorkQueueError> {
        validate_timestamp(now_unix_milliseconds, "now_unix_milliseconds")?;

        let lease_expires_at =
            lease_expiration(now_unix_milliseconds, lease_duration_milliseconds)?;

        let result = sqlx::query(RENEW_LEASE_SQL)
            .bind(transaction_id.into_uuid())
            .bind(*worker_id.as_uuid())
            .bind(now_unix_milliseconds)
            .bind(lease_expires_at)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database("renew reconciliation work lease", source)
            })?;

        require_owned_lease(result.rows_affected(), transaction_id)
    }

    /// Releases actively owned work for a later retry.
    ///
    /// Attempt count is preserved. The next successful claim increments the
    /// durable attempt number.
    pub async fn retry(
        &self,
        transaction_id: FareTransactionId,
        worker_id: ReconciliationWorkerId,
        now_unix_milliseconds: i64,
        available_at_unix_milliseconds: i64,
    ) -> Result<(), ReconciliationWorkQueueError> {
        validate_timestamp(now_unix_milliseconds, "now_unix_milliseconds")?;

        validate_timestamp(
            available_at_unix_milliseconds,
            "available_at_unix_milliseconds",
        )?;

        if available_at_unix_milliseconds < now_unix_milliseconds {
            return Err(ReconciliationWorkQueueError::RetryBeforeCurrentTime);
        }

        let result = sqlx::query(RETRY_SQL)
            .bind(transaction_id.into_uuid())
            .bind(*worker_id.as_uuid())
            .bind(now_unix_milliseconds)
            .bind(available_at_unix_milliseconds)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database("retry reconciliation work", source)
            })?;

        require_owned_lease(result.rows_affected(), transaction_id)
    }

    /// Marks actively owned work completed.
    ///
    /// Completion requires the same worker identity and a still-live lease,
    /// preventing late work from overwriting a newer claim.
    pub async fn complete(
        &self,
        transaction_id: FareTransactionId,
        worker_id: ReconciliationWorkerId,
        completed_at_unix_milliseconds: i64,
    ) -> Result<(), ReconciliationWorkQueueError> {
        validate_timestamp(
            completed_at_unix_milliseconds,
            "completed_at_unix_milliseconds",
        )?;

        let result = sqlx::query(COMPLETE_SQL)
            .bind(transaction_id.into_uuid())
            .bind(*worker_id.as_uuid())
            .bind(completed_at_unix_milliseconds)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                ReconciliationWorkQueueError::database("complete reconciliation work", source)
            })?;

        require_owned_lease(result.rows_affected(), transaction_id)
    }
}

#[derive(Debug, FromRow)]
struct WorkRow {
    fare_transaction_id: Uuid,
    reader_id: Uuid,
    source_batch_id: Uuid,
    attempt_count: i32,
    claimed_at_unix_milliseconds: Option<i64>,
    lease_expires_at_unix_milliseconds: Option<i64>,
}

fn decode_work_row(
    row: &WorkRow,
) -> Result<ClaimedReconciliationWork, ReconciliationWorkQueueError> {
    let transaction_id = FareTransactionId::try_from(row.fare_transaction_id).map_err(|_| {
        ReconciliationWorkQueueError::invalid("reconciliation_work_items.fare_transaction_id")
    })?;

    let reader_id = ReaderId::try_from(row.reader_id).map_err(|_| {
        ReconciliationWorkQueueError::invalid("reconciliation_work_items.reader_id")
    })?;

    let source_batch_id = SynchronizationBatchId::try_from(row.source_batch_id).map_err(|_| {
        ReconciliationWorkQueueError::invalid("reconciliation_work_items.source_batch_id")
    })?;

    let attempt_count = u32::try_from(row.attempt_count).map_err(|_| {
        ReconciliationWorkQueueError::invalid("reconciliation_work_items.attempt_count")
    })?;

    if attempt_count == 0 {
        return Err(ReconciliationWorkQueueError::invalid(
            "reconciliation_work_items.attempt_count",
        ));
    }

    let claimed_at_unix_milliseconds = row.claimed_at_unix_milliseconds.ok_or_else(|| {
        ReconciliationWorkQueueError::invalid(
            "reconciliation_work_items.claimed_at_unix_milliseconds",
        )
    })?;

    let lease_expires_at_unix_milliseconds =
        row.lease_expires_at_unix_milliseconds.ok_or_else(|| {
            ReconciliationWorkQueueError::invalid(
                "reconciliation_work_items.lease_expires_at_unix_milliseconds",
            )
        })?;

    if claimed_at_unix_milliseconds < 0
        || lease_expires_at_unix_milliseconds <= claimed_at_unix_milliseconds
    {
        return Err(ReconciliationWorkQueueError::invalid(
            "reconciliation_work_items.lease",
        ));
    }

    Ok(ClaimedReconciliationWork {
        transaction_id,
        reader_id,
        source_batch_id,
        attempt_count,
        claimed_at_unix_milliseconds,
        lease_expires_at_unix_milliseconds,
    })
}

fn validate_limit(limit: u16) -> Result<i64, ReconciliationWorkQueueError> {
    if limit == 0 {
        return Err(ReconciliationWorkQueueError::ZeroBatchSize);
    }

    if limit > MAX_RECONCILIATION_WORK_BATCH_SIZE {
        return Err(ReconciliationWorkQueueError::BatchSizeTooLarge {
            requested: limit,
            maximum: MAX_RECONCILIATION_WORK_BATCH_SIZE,
        });
    }

    Ok(i64::from(limit))
}

const fn validate_timestamp(
    value: i64,
    field: &'static str,
) -> Result<(), ReconciliationWorkQueueError> {
    if value < 0 {
        return Err(ReconciliationWorkQueueError::NegativeTimestamp { field });
    }

    Ok(())
}

fn lease_expiration(
    now_unix_milliseconds: i64,
    lease_duration_milliseconds: i64,
) -> Result<i64, ReconciliationWorkQueueError> {
    if lease_duration_milliseconds <= 0 {
        return Err(ReconciliationWorkQueueError::InvalidLeaseDuration);
    }

    now_unix_milliseconds
        .checked_add(lease_duration_milliseconds)
        .ok_or(ReconciliationWorkQueueError::LeaseExpirationOverflow)
}

fn require_owned_lease(
    rows_affected: u64,
    transaction_id: FareTransactionId,
) -> Result<(), ReconciliationWorkQueueError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(ReconciliationWorkQueueError::LeaseLost { transaction_id })
    }
}

#[cfg(test)]
mod tests {
    use uuid::{Uuid, Variant, Version};

    use super::{
        MAX_RECONCILIATION_WORK_BATCH_SIZE, ReconciliationWorkQueueError, ReconciliationWorkerId,
        ReconciliationWorkerIdError, lease_expiration, validate_limit,
    };

    #[test]
    fn generated_worker_identity_is_uuid_v7() {
        let worker_id = ReconciliationWorkerId::generate();

        assert!(!worker_id.as_uuid().is_nil());

        assert_eq!(worker_id.as_uuid().get_variant(), Variant::RFC4122);

        assert_eq!(worker_id.as_uuid().get_version(), Some(Version::SortRand));
    }

    #[test]
    fn non_v7_worker_identity_is_rejected() {
        let value = match Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000") {
            Ok(value) => value,

            Err(error) => {
                panic!("test UUID failed to parse: {error}")
            }
        };

        let result = ReconciliationWorkerId::try_from(value);

        assert_eq!(result, Err(ReconciliationWorkerIdError::UnsupportedVersion));
    }

    #[test]
    fn work_batch_bound_is_enforced() {
        assert!(matches!(
            validate_limit(0),
            Err(ReconciliationWorkQueueError::ZeroBatchSize)
        ));

        assert!(validate_limit(MAX_RECONCILIATION_WORK_BATCH_SIZE).is_ok());

        assert!(matches!(
            validate_limit(MAX_RECONCILIATION_WORK_BATCH_SIZE + 1),
            Err(ReconciliationWorkQueueError::BatchSizeTooLarge { .. })
        ));
    }

    #[test]
    fn lease_duration_must_be_positive() {
        assert!(matches!(
            lease_expiration(100, 0),
            Err(ReconciliationWorkQueueError::InvalidLeaseDuration)
        ));

        assert!(matches!(lease_expiration(100, 50), Ok(150)));
    }

    #[test]
    fn lease_expiration_overflow_is_rejected() {
        assert!(matches!(
            lease_expiration(i64::MAX, 1),
            Err(ReconciliationWorkQueueError::LeaseExpirationOverflow)
        ));
    }
}
