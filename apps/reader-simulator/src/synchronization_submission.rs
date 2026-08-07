//! Durable reader synchronization submission orchestration.
//!
//! This module connects reader-local SQLite state to the project-owned HTTP
//! synchronization transport. It does not contain real transit credentials,
//! proprietary protocols, or external authentication material.

use std::{future::Future, pin::Pin};

use sqlx::SqlitePool;
use thiserror::Error;
use transitguard_device_protocol::{
    SynchronizationBatchAcknowledgement, SynchronizationBatchRequest,
    SynchronizationFailureCategory,
};
use transitguard_domain::{ReaderId, SynchronizationBatchId};
use transitguard_persistence::{
    ReaderAcknowledgementApplicationError, ReaderProtocolAcknowledgementError,
    ReaderSynchronizationFailureError, ReaderSynchronizationRequestError,
    ReaderSynchronizationStateError, SynchronizationAcknowledgementApplication,
    SynchronizationBatch, apply_synchronization_acknowledgement,
    load_synchronization_batch_request, record_synchronization_manual_review,
    record_synchronization_permanent_failure, record_synchronization_retryable_failure,
    store_protocol_synchronization_acknowledgement,
};
use transitguard_telemetry::SynchronizationTelemetry;

use crate::synchronization_transport::{SynchronizationHttpClient, SynchronizationHttpClientError};

/// Boxed future returned by synchronization transports.
pub type SynchronizationTransportFuture<'a> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    SynchronizationBatchAcknowledgement,
                    SynchronizationTransportFailure,
                >,
            > + Send
            + 'a,
    >,
>;

/// A synchronization transport that submits one validated protocol request.
pub trait SynchronizationTransport: Send + Sync {
    /// Submits one synchronization request.
    fn submit<'a>(
        &'a self,
        request: &'a SynchronizationBatchRequest,
    ) -> SynchronizationTransportFuture<'a>;
}

/// Transport failure retained for diagnostics after durable classification.
#[derive(Debug, Error)]
pub enum SynchronizationTransportFailure {
    /// The concrete HTTP client failed.
    #[error(
        "synchronization HTTP transport failed with category \
         {category:?}"
    )]
    Http {
        /// Stable operational category.
        category: SynchronizationFailureCategory,

        /// Original HTTP transport error.
        #[source]
        source: SynchronizationHttpClientError,
    },

    /// A transport supplied a stable category without an underlying source.
    #[error("synchronization transport failed with category {category:?}")]
    Classified {
        /// Stable operational category.
        category: SynchronizationFailureCategory,
    },
}

impl SynchronizationTransportFailure {
    /// Creates a categorized transport failure without a source error.
    #[must_use]
    pub const fn classified(category: SynchronizationFailureCategory) -> Self {
        Self::Classified { category }
    }

    /// Returns the stable failure category.
    #[must_use]
    pub const fn category(&self) -> SynchronizationFailureCategory {
        match self {
            Self::Http { category, .. } | Self::Classified { category } => *category,
        }
    }
}

impl SynchronizationTransport for SynchronizationHttpClient {
    fn submit<'a>(
        &'a self,
        request: &'a SynchronizationBatchRequest,
    ) -> SynchronizationTransportFuture<'a> {
        Box::pin(async move {
            SynchronizationHttpClient::submit(self, request)
                .await
                .map_err(|source| {
                    let category = source.failure_category();

                    SynchronizationTransportFailure::Http { category, source }
                })
        })
    }
}

/// Durable policy selected for one transport failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizationFailureDisposition {
    /// Preserve the batch and schedule its stable identity for retry.
    Retry,

    /// Stop automated retries because the payload cannot succeed unchanged.
    PermanentFailure,

    /// Retain the batch for operator investigation.
    ManualReview,
}

/// Determines the durable policy for one stable transport category.
#[must_use]
pub const fn synchronization_failure_disposition(
    category: SynchronizationFailureCategory,
) -> SynchronizationFailureDisposition {
    match category {
        SynchronizationFailureCategory::NetworkTimeout
        | SynchronizationFailureCategory::ConnectionFailure
        | SynchronizationFailureCategory::ResponseDecodeFailure
        | SynchronizationFailureCategory::BackendTemporarilyUnavailable => {
            SynchronizationFailureDisposition::Retry
        }

        SynchronizationFailureCategory::PayloadTooLarge
        | SynchronizationFailureCategory::UnsupportedProtocol
        | SynchronizationFailureCategory::BackendValidationFailure => {
            SynchronizationFailureDisposition::PermanentFailure
        }

        SynchronizationFailureCategory::ReaderNotRegistered
        | SynchronizationFailureCategory::ReaderNotOperational
        | SynchronizationFailureCategory::EnvironmentMismatch
        | SynchronizationFailureCategory::BatchIdentityConflict
        | SynchronizationFailureCategory::BatchRangeMismatch
        | SynchronizationFailureCategory::EntryOrderMismatch
        | SynchronizationFailureCategory::TransactionIdentityConflict
        | SynchronizationFailureCategory::ManualReviewRequired => {
            SynchronizationFailureDisposition::ManualReview
        }
    }
}

/// Durable result of submitting one in-flight synchronization batch.
#[derive(Debug)]
pub enum SynchronizationSubmissionResult {
    /// The backend acknowledgement was stored and applied to SQLite.
    Applied {
        /// Whether the durable acknowledgement already existed.
        acknowledgement_replayed: bool,

        /// Atomic queue and batch application result.
        application: SynchronizationAcknowledgementApplication,
    },

    /// The same stable batch is scheduled for another attempt.
    RetryScheduled {
        /// Original categorized transport failure.
        failure: SynchronizationTransportFailure,

        /// Updated durable synchronization batch.
        batch: Box<SynchronizationBatch>,
    },

    /// The batch and its queue entries received a terminal failure.
    PermanentFailure {
        /// Original categorized transport failure.
        failure: SynchronizationTransportFailure,

        /// Updated durable synchronization batch.
        batch: Box<SynchronizationBatch>,
    },

    /// The batch and its queue entries require operator review.
    ManualReview {
        /// Original categorized transport failure.
        failure: SynchronizationTransportFailure,

        /// Updated durable synchronization batch.
        batch: Box<SynchronizationBatch>,
    },
}

/// Failures produced while coordinating durable synchronization.
#[derive(Debug, Error)]
pub enum SynchronizationSubmissionError {
    /// Durable protocol-request reconstruction failed.
    #[error(transparent)]
    Request(#[from] ReaderSynchronizationRequestError),

    /// Protocol acknowledgement translation or storage failed.
    #[error(transparent)]
    Acknowledgement(#[from] ReaderProtocolAcknowledgementError),

    /// Durable acknowledgement application failed.
    #[error(transparent)]
    Application(#[from] ReaderAcknowledgementApplicationError),

    /// Retry-state persistence failed.
    #[error(transparent)]
    RetryState(#[from] ReaderSynchronizationStateError),

    /// Final-state persistence failed.
    #[error(transparent)]
    FinalState(#[from] ReaderSynchronizationFailureError),
}

/// Submits one batch that is already durably marked as in flight.
///
/// Success stores the protocol acknowledgement before atomically applying its
/// outcomes. Transport failures are converted to retry, permanent-failure, or
/// manual-review states according to their stable protocol category.
pub async fn submit_in_flight_synchronization_batch<T: SynchronizationTransport>(
    pool: &SqlitePool,
    transport: &T,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    completed_at_unix_milliseconds: i64,
    retry_at_unix_milliseconds: i64,
) -> Result<SynchronizationSubmissionResult, SynchronizationSubmissionError> {
    let telemetry = SynchronizationTelemetry::new();

    submit_in_flight_synchronization_batch_with_telemetry(
        pool,
        transport,
        &telemetry,
        reader_id,
        batch_id,
        completed_at_unix_milliseconds,
        retry_at_unix_milliseconds,
    )
    .await
}

/// Submits one in-flight synchronization batch while recording sanitized
/// process-local telemetry.
///
/// The recorder receives request, acknowledgement, entry-outcome, and stable
/// transport-failure counters. It never receives transaction envelopes,
/// credentials, response bodies, database errors, or stack traces.
pub async fn submit_in_flight_synchronization_batch_with_telemetry<T: SynchronizationTransport>(
    pool: &SqlitePool,
    transport: &T,
    telemetry: &SynchronizationTelemetry,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    completed_at_unix_milliseconds: i64,
    retry_at_unix_milliseconds: i64,
) -> Result<SynchronizationSubmissionResult, SynchronizationSubmissionError> {
    let request = load_synchronization_batch_request(pool, reader_id, batch_id).await?;

    telemetry.record_request_started();

    match transport.submit(&request).await {
        Ok(acknowledgement) => {
            let stored =
                store_protocol_synchronization_acknowledgement(pool, &acknowledgement).await?;

            telemetry.record_acknowledgement(&acknowledgement);

            let applied_at_unix_milliseconds =
                completed_at_unix_milliseconds.max(acknowledgement.received_at_unix_milliseconds());

            let application = apply_synchronization_acknowledgement(
                pool,
                reader_id,
                batch_id,
                applied_at_unix_milliseconds,
            )
            .await?;

            Ok(SynchronizationSubmissionResult::Applied {
                acknowledgement_replayed: stored.replayed(),
                application,
            })
        }

        Err(failure) => {
            let category = failure.category();
            let failure_category = category.as_str();

            telemetry.record_request_failure(category);

            match synchronization_failure_disposition(category) {
                SynchronizationFailureDisposition::Retry => {
                    let batch = record_synchronization_retryable_failure(
                        pool,
                        reader_id,
                        batch_id,
                        failure_category,
                        completed_at_unix_milliseconds,
                        retry_at_unix_milliseconds,
                    )
                    .await?;

                    Ok(SynchronizationSubmissionResult::RetryScheduled {
                        failure,
                        batch: Box::new(batch),
                    })
                }

                SynchronizationFailureDisposition::PermanentFailure => {
                    let batch = record_synchronization_permanent_failure(
                        pool,
                        reader_id,
                        batch_id,
                        failure_category,
                        completed_at_unix_milliseconds,
                    )
                    .await?;

                    Ok(SynchronizationSubmissionResult::PermanentFailure {
                        failure,
                        batch: Box::new(batch),
                    })
                }

                SynchronizationFailureDisposition::ManualReview => {
                    let batch = record_synchronization_manual_review(
                        pool,
                        reader_id,
                        batch_id,
                        failure_category,
                        completed_at_unix_milliseconds,
                    )
                    .await?;

                    Ok(SynchronizationSubmissionResult::ManualReview {
                        failure,
                        batch: Box::new(batch),
                    })
                }
            }
        }
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
    use transitguard_device_protocol::{
        DeviceProtocolVersion, SynchronizationAcknowledgementEntry,
        SynchronizationBatchAcknowledgement, SynchronizationBatchAcknowledgementDefinition,
        SynchronizationBatchRequest, SynchronizationEntryOutcome, SynchronizationFailureCategory,
    };
    use transitguard_domain::{
        Currency, EventTime, FareApprovalReason, FareCredentialId, FareDecision, FarePolicyVersion,
        FareTransactionId, Money, ReaderId, SynchronizationBatchId,
    };
    use transitguard_persistence::{
        OfflineQueueState, OfflineTransactionDraft, ReaderDatabaseIdentity, ReaderSqliteConfig,
        SynchronizationBatch, SynchronizationBatchState, bind_reader_database,
        connect_reader_sqlite, create_synchronization_batch, enqueue_offline_transaction,
        load_offline_queue, load_synchronization_batch, mark_synchronization_batch_in_flight,
        run_reader_sqlite_migrations,
    };
    use transitguard_telemetry::SynchronizationTelemetry;

    use super::{
        SynchronizationSubmissionResult, SynchronizationTransport, SynchronizationTransportFailure,
        SynchronizationTransportFuture, submit_in_flight_synchronization_batch,
        submit_in_flight_synchronization_batch_with_telemetry,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    #[derive(Clone, Copy)]
    enum FakeResponse {
        Acknowledge,
        Fail(SynchronizationFailureCategory),
    }

    struct FakeTransport {
        response: FakeResponse,
        received_at_unix_milliseconds: i64,
    }

    impl FakeTransport {
        const fn acknowledging(received_at_unix_milliseconds: i64) -> Self {
            Self {
                response: FakeResponse::Acknowledge,
                received_at_unix_milliseconds,
            }
        }

        const fn failing(category: SynchronizationFailureCategory) -> Self {
            Self {
                response: FakeResponse::Fail(category),
                received_at_unix_milliseconds: 0,
            }
        }
    }

    impl SynchronizationTransport for FakeTransport {
        fn submit<'a>(
            &'a self,
            request: &'a SynchronizationBatchRequest,
        ) -> SynchronizationTransportFuture<'a> {
            Box::pin(async move {
                match self.response {
                    FakeResponse::Fail(category) => {
                        Err(SynchronizationTransportFailure::classified(category))
                    }

                    FakeResponse::Acknowledge => {
                        let entries = request
                            .entries()
                            .iter()
                            .map(|entry| {
                                SynchronizationAcknowledgementEntry::new(
                                    entry.transaction_id(),
                                    entry.local_sequence_number(),
                                    SynchronizationEntryOutcome::Acknowledged,
                                    None,
                                    None,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| {
                                SynchronizationTransportFailure::classified(
                                    SynchronizationFailureCategory::ResponseDecodeFailure,
                                )
                            })?;

                        SynchronizationBatchAcknowledgement::new(
                            SynchronizationBatchAcknowledgementDefinition {
                                protocol_version: request.protocol_version(),
                                environment_id: request.environment_id().clone(),
                                reader_id: request.reader_id(),
                                batch_id: request.batch_id(),
                                first_local_sequence_number: request.first_local_sequence_number(),
                                last_local_sequence_number: request.last_local_sequence_number(),
                                received_at_unix_milliseconds: self.received_at_unix_milliseconds,
                                replayed: false,
                                entries,
                            },
                        )
                        .map_err(|_| {
                            SynchronizationTransportFailure::classified(
                                SynchronizationFailureCategory::ResponseDecodeFailure,
                            )
                        })
                    }
                }
            })
        }
    }

    fn database_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "transitguard-submission-{name}-{}.sqlite3",
            SynchronizationBatchId::generate()
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

    async fn submitted_batch(pool: &SqlitePool, reader_id: ReaderId) -> SynchronizationBatch {
        for _ in 0..2 {
            if let Err(error) = enqueue_offline_transaction(pool, reader_id, &draft()).await {
                panic!("queue insertion failed: {error}");
            }
        }

        let batch = match create_synchronization_batch(
            pool,
            reader_id,
            DeviceProtocolVersion::CURRENT,
            TEST_TIME + 200,
            2,
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
            batch.batch_id(),
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

    #[tokio::test]
    async fn successful_submission_applies_acknowledgement() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("success", reader_id).await;

        let submitted = submitted_batch(&pool, reader_id).await;

        let transport = FakeTransport::acknowledging(TEST_TIME + 350);

        let telemetry = SynchronizationTelemetry::new();

        let result = submit_in_flight_synchronization_batch_with_telemetry(
            &pool,
            &transport,
            &telemetry,
            reader_id,
            submitted.batch_id(),
            TEST_TIME + 400,
            TEST_TIME + 600,
        )
        .await;

        let application = match result {
            Ok(SynchronizationSubmissionResult::Applied {
                acknowledgement_replayed,
                application,
            }) => {
                assert!(!acknowledgement_replayed);
                application
            }

            Ok(other) => {
                pool.close().await;
                remove_database(&path);
                panic!("unexpected synchronization result: {other:?}")
            }

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("submission failed: {error}")
            }
        };

        assert!(application.applied_now());
        assert_eq!(application.acknowledged_entries(), 2);
        assert_eq!(application.retryable_failure_entries(), 0);
        assert_eq!(application.permanent_failure_entries(), 0);
        assert_eq!(application.manual_review_entries(), 0);
        assert_eq!(application.applied_at_unix_milliseconds(), TEST_TIME + 400);
        assert_eq!(application.last_acknowledged_sequence(), 2);

        let telemetry_snapshot = telemetry.snapshot();

        assert_eq!(telemetry_snapshot.requests_total, 1);
        assert_eq!(telemetry_snapshot.acknowledgements_total, 1);
        assert_eq!(telemetry_snapshot.entries_total, 2);
        assert_eq!(telemetry_snapshot.outcomes.acknowledged, 2);
        assert_eq!(telemetry_snapshot.request_failures_total, 0);

        let batch = match load_synchronization_batch(&pool, reader_id, submitted.batch_id()).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("batch reload failed: {error}")
            }
        };

        assert_eq!(batch.state(), SynchronizationBatchState::Acknowledged);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue reload failed: {error}")
            }
        };

        assert!(
            queue
                .iter()
                .all(|entry| { entry.queue_state() == OfflineQueueState::Acknowledged })
        );

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn temporary_transport_failure_schedules_retry() {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database("retry", reader_id).await;

        let submitted = submitted_batch(&pool, reader_id).await;

        let transport = FakeTransport::failing(SynchronizationFailureCategory::NetworkTimeout);

        let telemetry = SynchronizationTelemetry::new();

        let result = submit_in_flight_synchronization_batch_with_telemetry(
            &pool,
            &transport,
            &telemetry,
            reader_id,
            submitted.batch_id(),
            TEST_TIME + 400,
            TEST_TIME + 600,
        )
        .await;

        let batch = match result {
            Ok(SynchronizationSubmissionResult::RetryScheduled { failure, batch }) => {
                assert_eq!(
                    failure.category(),
                    SynchronizationFailureCategory::NetworkTimeout
                );
                batch
            }

            Ok(other) => {
                pool.close().await;
                remove_database(&path);
                panic!("unexpected synchronization result: {other:?}")
            }

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("retry scheduling failed: {error}")
            }
        };

        assert_eq!(batch.state(), SynchronizationBatchState::RetryableFailure);
        assert_eq!(
            batch.next_retry_at_unix_milliseconds(),
            Some(TEST_TIME + 600)
        );
        assert_eq!(batch.last_failure_category(), Some("network_timeout"));

        let telemetry_snapshot = telemetry.snapshot();

        assert_eq!(telemetry_snapshot.requests_total, 1);
        assert_eq!(telemetry_snapshot.acknowledgements_total, 0);
        assert_eq!(telemetry_snapshot.entries_total, 0);
        assert_eq!(telemetry_snapshot.request_failures_total, 1);
        assert_eq!(telemetry_snapshot.failures.network_timeout, 1);

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue reload failed: {error}")
            }
        };

        assert!(
            queue
                .iter()
                .all(|entry| { entry.queue_state() == OfflineQueueState::InFlight })
        );

        pool.close().await;
        remove_database(&path);
    }

    async fn assert_final_policy(
        test_name: &str,
        category: SynchronizationFailureCategory,
        expected_batch_state: SynchronizationBatchState,
        expected_queue_state: OfflineQueueState,
    ) {
        let reader_id = ReaderId::generate();
        let (path, pool) = open_database(test_name, reader_id).await;

        let submitted = submitted_batch(&pool, reader_id).await;

        let transport = FakeTransport::failing(category);

        let result = submit_in_flight_synchronization_batch(
            &pool,
            &transport,
            reader_id,
            submitted.batch_id(),
            TEST_TIME + 400,
            TEST_TIME + 600,
        )
        .await;

        let batch = match result {
            Ok(SynchronizationSubmissionResult::PermanentFailure { failure, batch })
            | Ok(SynchronizationSubmissionResult::ManualReview { failure, batch }) => {
                assert_eq!(failure.category(), category);
                batch
            }

            Ok(other) => {
                pool.close().await;
                remove_database(&path);
                panic!("unexpected synchronization result: {other:?}")
            }

            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("final policy failed: {error}")
            }
        };

        assert_eq!(batch.state(), expected_batch_state);
        assert_eq!(batch.last_failure_category(), Some(category.as_str()));

        let queue = match load_offline_queue(&pool, reader_id).await {
            Ok(value) => value,
            Err(error) => {
                pool.close().await;
                remove_database(&path);
                panic!("queue reload failed: {error}")
            }
        };

        assert!(
            queue
                .iter()
                .all(|entry| { entry.queue_state() == expected_queue_state })
        );

        pool.close().await;
        remove_database(&path);
    }

    #[tokio::test]
    async fn final_transport_failures_follow_durable_policy() {
        assert_final_policy(
            "permanent",
            SynchronizationFailureCategory::PayloadTooLarge,
            SynchronizationBatchState::PermanentFailure,
            OfflineQueueState::PermanentFailure,
        )
        .await;

        assert_final_policy(
            "manual-review",
            SynchronizationFailureCategory::BatchIdentityConflict,
            SynchronizationBatchState::ManualReview,
            OfflineQueueState::ManualReview,
        )
        .await;
    }
}
