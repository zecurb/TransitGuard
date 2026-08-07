//! Structured, payload-free TransitGuard operational telemetry.
//!
//! Telemetry records only bounded counters and stable project-owned
//! classifications. It never records transaction envelopes, credentials,
//! secrets, database errors, network response bodies, or stack traces.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use transitguard_device_protocol::{
    SynchronizationBatchAcknowledgement, SynchronizationEntryOutcome,
    SynchronizationFailureCategory,
};

const ACKNOWLEDGED_OUTCOME_INDEX: usize = 0;
const RETRYABLE_FAILURE_OUTCOME_INDEX: usize = 1;
const PERMANENT_FAILURE_OUTCOME_INDEX: usize = 2;
const MANUAL_REVIEW_OUTCOME_INDEX: usize = 3;
const OUTCOME_COUNT: usize = 4;

const NETWORK_TIMEOUT_FAILURE_INDEX: usize = 0;
const CONNECTION_FAILURE_INDEX: usize = 1;
const RESPONSE_DECODE_FAILURE_INDEX: usize = 2;
const PAYLOAD_TOO_LARGE_FAILURE_INDEX: usize = 3;
const UNSUPPORTED_PROTOCOL_FAILURE_INDEX: usize = 4;
const READER_NOT_REGISTERED_FAILURE_INDEX: usize = 5;
const READER_NOT_OPERATIONAL_FAILURE_INDEX: usize = 6;
const ENVIRONMENT_MISMATCH_FAILURE_INDEX: usize = 7;
const BATCH_IDENTITY_CONFLICT_FAILURE_INDEX: usize = 8;
const BATCH_RANGE_MISMATCH_FAILURE_INDEX: usize = 9;
const ENTRY_ORDER_MISMATCH_FAILURE_INDEX: usize = 10;
const TRANSACTION_IDENTITY_CONFLICT_FAILURE_INDEX: usize = 11;
const BACKEND_TEMPORARILY_UNAVAILABLE_FAILURE_INDEX: usize = 12;
const BACKEND_VALIDATION_FAILURE_INDEX: usize = 13;
const MANUAL_REVIEW_REQUIRED_FAILURE_INDEX: usize = 14;
const FAILURE_COUNT: usize = 15;

/// Immutable per-entry synchronization outcome counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SynchronizationOutcomeCounts {
    /// Entries accepted by the backend.
    pub acknowledged: u64,

    /// Entries retained for a later retry.
    pub retryable_failure: u64,

    /// Entries rejected permanently.
    pub permanent_failure: u64,

    /// Entries requiring operator review.
    pub manual_review: u64,
}

/// Immutable stable synchronization failure-category counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SynchronizationFailureCounts {
    /// Requests that exceeded their configured timeout.
    pub network_timeout: u64,

    /// Requests that could not establish a connection.
    pub connection_failure: u64,

    /// Responses that could not be decoded safely.
    pub response_decode_failure: u64,

    /// Requests or responses that exceeded a protocol limit.
    pub payload_too_large: u64,

    /// Requests using an unsupported protocol version.
    pub unsupported_protocol: u64,

    /// Requests submitted for an unknown reader.
    pub reader_not_registered: u64,

    /// Requests submitted for an inoperable reader.
    pub reader_not_operational: u64,

    /// Requests bound to the wrong environment.
    pub environment_mismatch: u64,

    /// Batch identities reused with different content.
    pub batch_identity_conflict: u64,

    /// Requests with inconsistent sequence ranges.
    pub batch_range_mismatch: u64,

    /// Requests with inconsistent entry ordering.
    pub entry_order_mismatch: u64,

    /// Transactions conflicting with durable backend identity.
    pub transaction_identity_conflict: u64,

    /// Requests blocked by a temporary backend dependency failure.
    pub backend_temporarily_unavailable: u64,

    /// Requests rejected by backend validation.
    pub backend_validation_failure: u64,

    /// Requests requiring operator review.
    pub manual_review_required: u64,
}

/// Immutable synchronization health snapshot suitable for structured output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SynchronizationTelemetrySnapshot {
    /// Synchronization requests that began processing.
    pub requests_total: u64,

    /// Complete acknowledgements received or produced.
    pub acknowledgements_total: u64,

    /// Per-entry outcomes recorded from acknowledgements.
    pub entries_total: u64,

    /// Requests that completed with a stable failure category.
    pub request_failures_total: u64,

    /// Per-entry outcome totals.
    pub outcomes: SynchronizationOutcomeCounts,

    /// Stable request-failure totals.
    pub failures: SynchronizationFailureCounts,
}

#[derive(Debug)]
struct SynchronizationTelemetryInner {
    requests_total: AtomicU64,
    acknowledgements_total: AtomicU64,
    entries_total: AtomicU64,
    request_failures_total: AtomicU64,
    outcome_counts: [AtomicU64; OUTCOME_COUNT],
    failure_counts: [AtomicU64; FAILURE_COUNT],
}

impl Default for SynchronizationTelemetryInner {
    fn default() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            acknowledgements_total: AtomicU64::new(0),
            entries_total: AtomicU64::new(0),
            request_failures_total: AtomicU64::new(0),
            outcome_counts: std::array::from_fn(|_| AtomicU64::new(0)),
            failure_counts: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

/// Cloneable, thread-safe synchronization telemetry recorder.
///
/// Clones share the same atomic counters, allowing API state and background
/// tasks to report one consistent process-local snapshot.
#[derive(Clone, Debug, Default)]
pub struct SynchronizationTelemetry {
    inner: Arc<SynchronizationTelemetryInner>,
}

impl SynchronizationTelemetry {
    /// Creates an empty synchronization telemetry recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the start of one synchronization request.
    pub fn record_request_started(&self) {
        increment(&self.inner.requests_total);
    }

    /// Records one complete acknowledgement and all per-entry outcomes.
    pub fn record_acknowledgement(&self, acknowledgement: &SynchronizationBatchAcknowledgement) {
        increment(&self.inner.acknowledgements_total);

        for entry in acknowledgement.entries() {
            self.record_entry_outcome(entry.outcome());
        }
    }

    /// Records one synchronization entry outcome.
    pub fn record_entry_outcome(&self, outcome: SynchronizationEntryOutcome) {
        increment(&self.inner.entries_total);

        increment(&self.inner.outcome_counts[outcome_index(outcome)]);
    }

    /// Records one sanitized request failure.
    pub fn record_request_failure(&self, category: SynchronizationFailureCategory) {
        increment(&self.inner.request_failures_total);

        increment(&self.inner.failure_counts[failure_index(category)]);
    }

    /// Returns a consistent process-local counter snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SynchronizationTelemetrySnapshot {
        SynchronizationTelemetrySnapshot {
            requests_total: load(&self.inner.requests_total),
            acknowledgements_total: load(&self.inner.acknowledgements_total),
            entries_total: load(&self.inner.entries_total),
            request_failures_total: load(&self.inner.request_failures_total),
            outcomes: SynchronizationOutcomeCounts {
                acknowledged: self.load_outcome(ACKNOWLEDGED_OUTCOME_INDEX),
                retryable_failure: self.load_outcome(RETRYABLE_FAILURE_OUTCOME_INDEX),
                permanent_failure: self.load_outcome(PERMANENT_FAILURE_OUTCOME_INDEX),
                manual_review: self.load_outcome(MANUAL_REVIEW_OUTCOME_INDEX),
            },
            failures: SynchronizationFailureCounts {
                network_timeout: self.load_failure(NETWORK_TIMEOUT_FAILURE_INDEX),
                connection_failure: self.load_failure(CONNECTION_FAILURE_INDEX),
                response_decode_failure: self.load_failure(RESPONSE_DECODE_FAILURE_INDEX),
                payload_too_large: self.load_failure(PAYLOAD_TOO_LARGE_FAILURE_INDEX),
                unsupported_protocol: self.load_failure(UNSUPPORTED_PROTOCOL_FAILURE_INDEX),
                reader_not_registered: self.load_failure(READER_NOT_REGISTERED_FAILURE_INDEX),
                reader_not_operational: self.load_failure(READER_NOT_OPERATIONAL_FAILURE_INDEX),
                environment_mismatch: self.load_failure(ENVIRONMENT_MISMATCH_FAILURE_INDEX),
                batch_identity_conflict: self.load_failure(BATCH_IDENTITY_CONFLICT_FAILURE_INDEX),
                batch_range_mismatch: self.load_failure(BATCH_RANGE_MISMATCH_FAILURE_INDEX),
                entry_order_mismatch: self.load_failure(ENTRY_ORDER_MISMATCH_FAILURE_INDEX),
                transaction_identity_conflict: self
                    .load_failure(TRANSACTION_IDENTITY_CONFLICT_FAILURE_INDEX),
                backend_temporarily_unavailable: self
                    .load_failure(BACKEND_TEMPORARILY_UNAVAILABLE_FAILURE_INDEX),
                backend_validation_failure: self.load_failure(BACKEND_VALIDATION_FAILURE_INDEX),
                manual_review_required: self.load_failure(MANUAL_REVIEW_REQUIRED_FAILURE_INDEX),
            },
        }
    }

    fn load_outcome(&self, index: usize) -> u64 {
        load(&self.inner.outcome_counts[index])
    }

    fn load_failure(&self, index: usize) -> u64 {
        load(&self.inner.failure_counts[index])
    }
}

fn increment(counter: &AtomicU64) {
    let _ = counter.fetch_add(1, Ordering::Relaxed);
}

fn load(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

const fn outcome_index(outcome: SynchronizationEntryOutcome) -> usize {
    match outcome {
        SynchronizationEntryOutcome::Acknowledged => ACKNOWLEDGED_OUTCOME_INDEX,

        SynchronizationEntryOutcome::RetryableFailure => RETRYABLE_FAILURE_OUTCOME_INDEX,

        SynchronizationEntryOutcome::PermanentFailure => PERMANENT_FAILURE_OUTCOME_INDEX,

        SynchronizationEntryOutcome::ManualReview => MANUAL_REVIEW_OUTCOME_INDEX,
    }
}

const fn failure_index(category: SynchronizationFailureCategory) -> usize {
    match category {
        SynchronizationFailureCategory::NetworkTimeout => NETWORK_TIMEOUT_FAILURE_INDEX,

        SynchronizationFailureCategory::ConnectionFailure => CONNECTION_FAILURE_INDEX,

        SynchronizationFailureCategory::ResponseDecodeFailure => RESPONSE_DECODE_FAILURE_INDEX,

        SynchronizationFailureCategory::PayloadTooLarge => PAYLOAD_TOO_LARGE_FAILURE_INDEX,

        SynchronizationFailureCategory::UnsupportedProtocol => UNSUPPORTED_PROTOCOL_FAILURE_INDEX,

        SynchronizationFailureCategory::ReaderNotRegistered => READER_NOT_REGISTERED_FAILURE_INDEX,

        SynchronizationFailureCategory::ReaderNotOperational => {
            READER_NOT_OPERATIONAL_FAILURE_INDEX
        }

        SynchronizationFailureCategory::EnvironmentMismatch => ENVIRONMENT_MISMATCH_FAILURE_INDEX,

        SynchronizationFailureCategory::BatchIdentityConflict => {
            BATCH_IDENTITY_CONFLICT_FAILURE_INDEX
        }

        SynchronizationFailureCategory::BatchRangeMismatch => BATCH_RANGE_MISMATCH_FAILURE_INDEX,

        SynchronizationFailureCategory::EntryOrderMismatch => ENTRY_ORDER_MISMATCH_FAILURE_INDEX,

        SynchronizationFailureCategory::TransactionIdentityConflict => {
            TRANSACTION_IDENTITY_CONFLICT_FAILURE_INDEX
        }

        SynchronizationFailureCategory::BackendTemporarilyUnavailable => {
            BACKEND_TEMPORARILY_UNAVAILABLE_FAILURE_INDEX
        }

        SynchronizationFailureCategory::BackendValidationFailure => {
            BACKEND_VALIDATION_FAILURE_INDEX
        }

        SynchronizationFailureCategory::ManualReviewRequired => {
            MANUAL_REVIEW_REQUIRED_FAILURE_INDEX
        }
    }
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{
        SynchronizationEntryOutcome, SynchronizationFailureCategory,
    };

    use super::{
        SynchronizationFailureCounts, SynchronizationOutcomeCounts, SynchronizationTelemetry,
        SynchronizationTelemetrySnapshot,
    };

    #[test]
    fn synchronization_counts_are_structured() {
        let telemetry = SynchronizationTelemetry::new();

        telemetry.record_request_started();
        telemetry.record_request_started();

        telemetry.record_entry_outcome(SynchronizationEntryOutcome::Acknowledged);
        telemetry.record_entry_outcome(SynchronizationEntryOutcome::RetryableFailure);
        telemetry.record_entry_outcome(SynchronizationEntryOutcome::PermanentFailure);
        telemetry.record_entry_outcome(SynchronizationEntryOutcome::ManualReview);

        telemetry.record_request_failure(SynchronizationFailureCategory::NetworkTimeout);
        telemetry
            .record_request_failure(SynchronizationFailureCategory::BackendTemporarilyUnavailable);
        telemetry.record_request_failure(SynchronizationFailureCategory::BatchIdentityConflict);

        assert_eq!(
            telemetry.snapshot(),
            SynchronizationTelemetrySnapshot {
                requests_total: 2,
                acknowledgements_total: 0,
                entries_total: 4,
                request_failures_total: 3,
                outcomes: SynchronizationOutcomeCounts {
                    acknowledged: 1,
                    retryable_failure: 1,
                    permanent_failure: 1,
                    manual_review: 1,
                },
                failures: SynchronizationFailureCounts {
                    network_timeout: 1,
                    batch_identity_conflict: 1,
                    backend_temporarily_unavailable: 1,
                    ..SynchronizationFailureCounts::default()
                },
            }
        );
    }

    #[test]
    fn telemetry_clones_share_process_counters() {
        let first = SynchronizationTelemetry::new();
        let second = first.clone();

        first.record_request_started();

        second.record_request_failure(SynchronizationFailureCategory::ConnectionFailure);

        let snapshot = first.snapshot();

        assert_eq!(snapshot.requests_total, 1);
        assert_eq!(snapshot.request_failures_total, 1);
        assert_eq!(snapshot.failures.connection_failure, 1);
    }
}
