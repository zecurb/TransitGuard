//! TransitGuard backend worker orchestration.
//!
//! This crate coordinates bounded durable reconciliation work. It deliberately
//! keeps fare-policy evaluation outside the queue coordinator so deterministic
//! reconciliation processing can be tested independently from lease handling.

use std::{error::Error, future::Future};

use thiserror::Error;
use transitguard_domain::FareTransactionId;
use transitguard_persistence::{
    ClaimedReconciliationWork, MAX_RECONCILIATION_WORK_BATCH_SIZE, PostgresReconciliationWorkQueue,
    ReconciliationWorkQueueError, ReconciliationWorkerId,
};

/// Validated configuration for one reconciliation worker cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationWorkerConfig {
    enqueue_limit: u16,
    recovery_limit: u16,
    claim_limit: u16,
    lease_duration_milliseconds: i64,
}

/// Errors produced while validating reconciliation-worker configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReconciliationWorkerConfigError {
    /// Queue operation limits must be positive.
    #[error("reconciliation worker `{field}` must be greater than zero")]
    ZeroLimit {
        /// Stable configuration field.
        field: &'static str,
    },

    /// Queue operation limits cannot exceed the persistence safety bound.
    #[error("reconciliation worker `{field}` value {requested} exceeds maximum {maximum}")]
    LimitTooLarge {
        /// Stable configuration field.
        field: &'static str,

        /// Requested limit.
        requested: u16,

        /// Maximum queue-operation limit.
        maximum: u16,
    },

    /// A worker lease must have a positive duration.
    #[error("reconciliation worker lease duration must be greater than zero")]
    InvalidLeaseDuration,
}

impl ReconciliationWorkerConfig {
    /// Creates bounded reconciliation-worker configuration.
    pub fn new(
        enqueue_limit: u16,
        recovery_limit: u16,
        claim_limit: u16,
        lease_duration_milliseconds: i64,
    ) -> Result<Self, ReconciliationWorkerConfigError> {
        validate_limit(enqueue_limit, "enqueue_limit")?;

        validate_limit(recovery_limit, "recovery_limit")?;

        validate_limit(claim_limit, "claim_limit")?;

        if lease_duration_milliseconds <= 0 {
            return Err(ReconciliationWorkerConfigError::InvalidLeaseDuration);
        }

        Ok(Self {
            enqueue_limit,
            recovery_limit,
            claim_limit,
            lease_duration_milliseconds,
        })
    }

    /// Returns the maximum number of synchronized transactions bootstrapped
    /// into the durable queue in one cycle.
    #[must_use]
    pub const fn enqueue_limit(self) -> u16 {
        self.enqueue_limit
    }

    /// Returns the maximum number of expired leases recovered in one cycle.
    #[must_use]
    pub const fn recovery_limit(self) -> u16 {
        self.recovery_limit
    }

    /// Returns the maximum number of work items claimed in one cycle.
    #[must_use]
    pub const fn claim_limit(self) -> u16 {
        self.claim_limit
    }

    /// Returns the duration assigned to each newly claimed lease.
    #[must_use]
    pub const fn lease_duration_milliseconds(self) -> i64 {
        self.lease_duration_milliseconds
    }
}

/// A successful business-processing decision for one claimed transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationProcessDisposition {
    /// Processing completed and the durable work item may be finalized.
    Complete {
        /// Time at which processing completed.
        completed_at_unix_milliseconds: i64,
    },

    /// Processing encountered a classified retryable condition.
    Retry {
        /// Time at which the retry decision was made.
        observed_at_unix_milliseconds: i64,

        /// Earliest time at which the work may be claimed again.
        available_at_unix_milliseconds: i64,
    },
}

/// Summary of one bounded reconciliation-worker cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationWorkerCycleReport {
    enqueued: u64,
    recovered: u64,
    claimed: u64,
    completed: u64,
    retried: u64,
}

impl ReconciliationWorkerCycleReport {
    /// Returns how many newly synchronized transactions entered the queue.
    #[must_use]
    pub const fn enqueued(self) -> u64 {
        self.enqueued
    }

    /// Returns how many expired leases were recovered.
    #[must_use]
    pub const fn recovered(self) -> u64 {
        self.recovered
    }

    /// Returns how many work items were claimed.
    #[must_use]
    pub const fn claimed(self) -> u64 {
        self.claimed
    }

    /// Returns how many claimed items completed successfully.
    #[must_use]
    pub const fn completed(self) -> u64 {
        self.completed
    }

    /// Returns how many claimed items were deliberately scheduled for retry.
    #[must_use]
    pub const fn retried(self) -> u64 {
        self.retried
    }
}

/// Failures produced by one worker cycle.
#[derive(Debug, Error)]
pub enum ReconciliationWorkerCycleError<E>
where
    E: Error + 'static,
{
    /// Durable queue operation failed.
    #[error(transparent)]
    Queue(#[from] ReconciliationWorkQueueError),

    /// The business processor failed unexpectedly.
    ///
    /// The lease is intentionally left intact. Normal expired-lease recovery
    /// makes the work restart-safe without pretending an unknown processor
    /// failure was a classified retry.
    #[error("reconciliation processor failed for transaction {transaction_id}")]
    Processor {
        /// Transaction whose processing failed.
        transaction_id: FareTransactionId,

        /// Underlying processor failure.
        #[source]
        source: E,
    },
}

/// Runs one bounded reconciliation-worker cycle.
///
/// The cycle performs four ordered operations:
///
/// 1. bootstrap newly acknowledged synchronization transactions;
/// 2. recover expired worker leases;
/// 3. claim a bounded work batch;
/// 4. hand each claim to the supplied reconciliation processor.
///
/// Processor failures do not mark the work completed and do not manufacture a
/// retry classification. The live lease remains durable until normal lease
/// recovery makes the transaction available after process failure or restart.
pub async fn run_reconciliation_worker_cycle<Process, ProcessFuture, ProcessError>(
    queue: &PostgresReconciliationWorkQueue,
    worker_id: ReconciliationWorkerId,
    cycle_started_at_unix_milliseconds: i64,
    config: ReconciliationWorkerConfig,
    mut process: Process,
) -> Result<ReconciliationWorkerCycleReport, ReconciliationWorkerCycleError<ProcessError>>
where
    Process: FnMut(ClaimedReconciliationWork) -> ProcessFuture,
    ProcessFuture: Future<Output = Result<ReconciliationProcessDisposition, ProcessError>>,
    ProcessError: Error + Send + Sync + 'static,
{
    let enqueued = queue
        .enqueue_ready(cycle_started_at_unix_milliseconds, config.enqueue_limit())
        .await?;

    let recovered = queue
        .recover_expired(cycle_started_at_unix_milliseconds, config.recovery_limit())
        .await?;

    let claims = queue
        .claim_ready(
            worker_id,
            cycle_started_at_unix_milliseconds,
            config.lease_duration_milliseconds(),
            config.claim_limit(),
        )
        .await?;

    let claimed = u64::try_from(claims.len()).map_err(|_| {
        ReconciliationWorkQueueError::BatchSizeTooLarge {
            requested: config.claim_limit(),
            maximum: MAX_RECONCILIATION_WORK_BATCH_SIZE,
        }
    })?;

    let mut completed = 0_u64;
    let mut retried = 0_u64;

    for claim in claims {
        let transaction_id = claim.transaction_id();

        let disposition =
            process(claim)
                .await
                .map_err(|source| ReconciliationWorkerCycleError::Processor {
                    transaction_id,
                    source,
                })?;

        match disposition {
            ReconciliationProcessDisposition::Complete {
                completed_at_unix_milliseconds,
            } => {
                queue
                    .complete(transaction_id, worker_id, completed_at_unix_milliseconds)
                    .await?;

                completed += 1;
            }

            ReconciliationProcessDisposition::Retry {
                observed_at_unix_milliseconds,
                available_at_unix_milliseconds,
            } => {
                queue
                    .retry(
                        transaction_id,
                        worker_id,
                        observed_at_unix_milliseconds,
                        available_at_unix_milliseconds,
                    )
                    .await?;

                retried += 1;
            }
        }
    }

    Ok(ReconciliationWorkerCycleReport {
        enqueued,
        recovered,
        claimed,
        completed,
        retried,
    })
}

fn validate_limit(value: u16, field: &'static str) -> Result<(), ReconciliationWorkerConfigError> {
    if value == 0 {
        return Err(ReconciliationWorkerConfigError::ZeroLimit { field });
    }

    if value > MAX_RECONCILIATION_WORK_BATCH_SIZE {
        return Err(ReconciliationWorkerConfigError::LimitTooLarge {
            field,
            requested: value,
            maximum: MAX_RECONCILIATION_WORK_BATCH_SIZE,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use transitguard_persistence::MAX_RECONCILIATION_WORK_BATCH_SIZE;

    use super::{
        ReconciliationProcessDisposition, ReconciliationWorkerConfig,
        ReconciliationWorkerConfigError,
    };

    #[test]
    fn worker_configuration_accepts_bounded_values() {
        let result = ReconciliationWorkerConfig::new(32, 32, 16, 30_000);

        let config = match result {
            Ok(config) => config,

            Err(error) => {
                panic!("valid worker configuration failed: {error}")
            }
        };

        assert_eq!(config.enqueue_limit(), 32);
        assert_eq!(config.recovery_limit(), 32);
        assert_eq!(config.claim_limit(), 16);

        assert_eq!(config.lease_duration_milliseconds(), 30_000);
    }

    #[test]
    fn worker_configuration_rejects_zero_limit() {
        let result = ReconciliationWorkerConfig::new(0, 32, 16, 30_000);

        assert!(matches!(
            result,
            Err(ReconciliationWorkerConfigError::ZeroLimit {
                field: "enqueue_limit"
            })
        ));
    }

    #[test]
    fn worker_configuration_enforces_queue_bound() {
        let result =
            ReconciliationWorkerConfig::new(32, 32, MAX_RECONCILIATION_WORK_BATCH_SIZE + 1, 30_000);

        assert!(matches!(
            result,
            Err(ReconciliationWorkerConfigError::LimitTooLarge {
                field: "claim_limit",
                ..
            })
        ));
    }

    #[test]
    fn worker_configuration_rejects_invalid_lease() {
        let result = ReconciliationWorkerConfig::new(32, 32, 16, 0);

        assert!(matches!(
            result,
            Err(ReconciliationWorkerConfigError::InvalidLeaseDuration)
        ));
    }

    #[test]
    fn processing_dispositions_preserve_times() {
        let complete = ReconciliationProcessDisposition::Complete {
            completed_at_unix_milliseconds: 10,
        };

        assert!(matches!(
            complete,
            ReconciliationProcessDisposition::Complete {
                completed_at_unix_milliseconds: 10
            }
        ));

        let retry = ReconciliationProcessDisposition::Retry {
            observed_at_unix_milliseconds: 20,
            available_at_unix_milliseconds: 30,
        };

        assert!(matches!(
            retry,
            ReconciliationProcessDisposition::Retry {
                observed_at_unix_milliseconds: 20,
                available_at_unix_milliseconds: 30
            }
        ));
    }
}
