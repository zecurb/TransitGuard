use serde_json::Value;
use thiserror::Error;
use transitguard_device_protocol::{
    DeviceProtocolVersion, ProtocolEnvironmentId, ReaderSoftwareVersion,
    SynchronizationBatchAcknowledgement, SynchronizationBatchRequest, SynchronizationEntryOutcome,
    SynchronizationFailureCategory, SynchronizationPayloadFingerprint,
    SynchronizationProtocolError, SynchronizationRequestFingerprint,
};
use transitguard_domain::{
    FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};

/// Errors produced while preparing durable backend ingest records.
#[derive(Debug, Error)]
pub enum SynchronizationIngestRecordError {
    /// The acknowledgement did not match its submitted request.
    #[error("synchronization acknowledgement does not match the request: {0}")]
    AcknowledgementMismatch(#[from] SynchronizationProtocolError),

    /// A validated protocol value could not be converted to JSON.
    #[error("failed to serialize synchronization {payload}: {source}")]
    Serialization {
        /// Stable name of the payload being serialized.
        payload: &'static str,

        /// Underlying JSON serialization error.
        #[source]
        source: serde_json::Error,
    },
}

/// Persistence-ready representation of one synchronization batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSynchronizationIngest {
    protocol_version: DeviceProtocolVersion,
    environment_id: ProtocolEnvironmentId,
    reader_id: ReaderId,
    reader_software_version: ReaderSoftwareVersion,
    batch_id: SynchronizationBatchId,
    first_local_sequence_number: LocalSequenceNumber,
    last_local_sequence_number: LocalSequenceNumber,
    submitted_at_unix_milliseconds: i64,
    received_at_unix_milliseconds: i64,
    request_fingerprint: SynchronizationRequestFingerprint,
    canonical_request_json: Value,
    acknowledgement_fingerprint: SynchronizationPayloadFingerprint,
    canonical_acknowledgement_json: Value,
    entries: Vec<PreparedSynchronizationIngestEntry>,
}

impl PreparedSynchronizationIngest {
    /// Validates and converts one request and acknowledgement pair.
    pub fn prepare(
        request: &SynchronizationBatchRequest,
        acknowledgement: &SynchronizationBatchAcknowledgement,
    ) -> Result<Self, SynchronizationIngestRecordError> {
        acknowledgement.validate_against_request(request)?;

        let canonical_request_json = serde_json::to_value(request).map_err(|source| {
            SynchronizationIngestRecordError::Serialization {
                payload: "request",
                source,
            }
        })?;

        let canonical_acknowledgement_json =
            serde_json::to_value(acknowledgement).map_err(|source| {
                SynchronizationIngestRecordError::Serialization {
                    payload: "acknowledgement",
                    source,
                }
            })?;

        let mut entries = Vec::with_capacity(request.entries().len());

        for (entry_position, (request_entry, acknowledgement_entry)) in request
            .entries()
            .iter()
            .zip(acknowledgement.entries().iter())
            .enumerate()
        {
            let canonical_transaction_envelope_json = serde_json::from_str(
                request_entry.transaction_envelope().as_str(),
            )
            .map_err(|source| SynchronizationIngestRecordError::Serialization {
                payload: "transaction envelope",
                source,
            })?;

            entries.push(PreparedSynchronizationIngestEntry {
                entry_position,
                transaction_id: request_entry.transaction_id(),
                local_sequence_number: request_entry.local_sequence_number(),
                transaction_fingerprint:
                    transitguard_device_protocol::SynchronizationPayloadFingerprint::for_transaction_envelope(
                        request_entry.transaction_envelope(),
                    ),
                canonical_transaction_envelope_json,
                outcome: acknowledgement_entry.outcome(),
                failure_category: acknowledgement_entry.failure_category(),
                next_retry_at_unix_milliseconds: acknowledgement_entry
                    .next_retry_at_unix_milliseconds(),
                resolved_at_unix_milliseconds: acknowledgement.received_at_unix_milliseconds(),
            });
        }

        Ok(Self {
            protocol_version: request.protocol_version(),
            environment_id: request.environment_id().clone(),
            reader_id: request.reader_id(),
            reader_software_version: request.reader_software_version().clone(),
            batch_id: request.batch_id(),
            first_local_sequence_number: request.first_local_sequence_number(),
            last_local_sequence_number: request.last_local_sequence_number(),
            submitted_at_unix_milliseconds: request.submitted_at_unix_milliseconds(),
            received_at_unix_milliseconds: acknowledgement.received_at_unix_milliseconds(),
            request_fingerprint: request.fingerprint(),
            canonical_request_json,
            acknowledgement_fingerprint:
                transitguard_device_protocol::SynchronizationPayloadFingerprint::for_acknowledgement(
                    acknowledgement,
                ),
            canonical_acknowledgement_json,
            entries,
        })
    }

    /// Returns the project-owned protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the backend environment identity.
    #[must_use]
    pub const fn environment_id(&self) -> &ProtocolEnvironmentId {
        &self.environment_id
    }

    /// Returns the durable reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the reader software version.
    #[must_use]
    pub const fn reader_software_version(&self) -> &ReaderSoftwareVersion {
        &self.reader_software_version
    }

    /// Returns the synchronization batch identity.
    #[must_use]
    pub const fn batch_id(&self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns the declared first local sequence.
    #[must_use]
    pub const fn first_local_sequence_number(&self) -> LocalSequenceNumber {
        self.first_local_sequence_number
    }

    /// Returns the declared final local sequence.
    #[must_use]
    pub const fn last_local_sequence_number(&self) -> LocalSequenceNumber {
        self.last_local_sequence_number
    }

    /// Returns the reader submission time.
    #[must_use]
    pub const fn submitted_at_unix_milliseconds(&self) -> i64 {
        self.submitted_at_unix_milliseconds
    }

    /// Returns the backend receipt time.
    #[must_use]
    pub const fn received_at_unix_milliseconds(&self) -> i64 {
        self.received_at_unix_milliseconds
    }

    /// Returns the number of ordered entries.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the deterministic request fingerprint.
    #[must_use]
    pub const fn request_fingerprint(&self) -> SynchronizationRequestFingerprint {
        self.request_fingerprint
    }

    /// Returns the validated request JSON object.
    #[must_use]
    pub const fn canonical_request_json(&self) -> &Value {
        &self.canonical_request_json
    }

    /// Returns the deterministic acknowledgement fingerprint.
    #[must_use]
    pub const fn acknowledgement_fingerprint(&self) -> SynchronizationPayloadFingerprint {
        self.acknowledgement_fingerprint
    }

    /// Returns the validated acknowledgement JSON object.
    #[must_use]
    pub const fn canonical_acknowledgement_json(&self) -> &Value {
        &self.canonical_acknowledgement_json
    }

    /// Returns the ordered persistence-ready entries.
    #[must_use]
    pub fn entries(&self) -> &[PreparedSynchronizationIngestEntry] {
        &self.entries
    }
}

/// Persistence-ready representation of one batch entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSynchronizationIngestEntry {
    entry_position: usize,
    transaction_id: FareTransactionId,
    local_sequence_number: LocalSequenceNumber,
    transaction_fingerprint: SynchronizationPayloadFingerprint,
    canonical_transaction_envelope_json: Value,
    outcome: SynchronizationEntryOutcome,
    failure_category: Option<SynchronizationFailureCategory>,
    next_retry_at_unix_milliseconds: Option<i64>,
    resolved_at_unix_milliseconds: i64,
}

impl PreparedSynchronizationIngestEntry {
    /// Returns the zero-based position in the submitted batch.
    #[must_use]
    pub const fn entry_position(&self) -> usize {
        self.entry_position
    }

    /// Returns the stable fare transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader-local sequence number.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the canonical transaction fingerprint.
    #[must_use]
    pub const fn transaction_fingerprint(&self) -> SynchronizationPayloadFingerprint {
        self.transaction_fingerprint
    }

    /// Returns the transaction envelope JSON object.
    #[must_use]
    pub const fn canonical_transaction_envelope_json(&self) -> &Value {
        &self.canonical_transaction_envelope_json
    }

    /// Returns the backend resolution.
    #[must_use]
    pub const fn outcome(&self) -> SynchronizationEntryOutcome {
        self.outcome
    }

    /// Returns the sanitized backend failure category.
    #[must_use]
    pub const fn failure_category(&self) -> Option<SynchronizationFailureCategory> {
        self.failure_category
    }

    /// Returns the earliest permitted retry time.
    #[must_use]
    pub const fn next_retry_at_unix_milliseconds(&self) -> Option<i64> {
        self.next_retry_at_unix_milliseconds
    }

    /// Returns the backend resolution time.
    #[must_use]
    pub const fn resolved_at_unix_milliseconds(&self) -> i64 {
        self.resolved_at_unix_milliseconds
    }
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{
        CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
        ReaderSoftwareVersion, SynchronizationAcknowledgementEntry,
        SynchronizationBatchAcknowledgement, SynchronizationBatchAcknowledgementDefinition,
        SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
        SynchronizationEntryOutcome, SynchronizationFailureCategory, SynchronizationRequestEntry,
    };
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use super::{PreparedSynchronizationIngest, SynchronizationIngestRecordError};

    const SUBMITTED_AT: i64 = 1_700_000_000_000;
    const RECEIVED_AT: i64 = 1_700_000_000_500;

    fn sequence(value: u64) -> LocalSequenceNumber {
        match LocalSequenceNumber::new(value) {
            Ok(sequence) => sequence,

            Err(error) => {
                panic!("valid sequence failed: {error}")
            }
        }
    }

    fn environment() -> ProtocolEnvironmentId {
        match ProtocolEnvironmentId::new("development") {
            Ok(environment) => environment,

            Err(error) => {
                panic!("valid environment failed: {error}")
            }
        }
    }

    fn software_version() -> ReaderSoftwareVersion {
        match ReaderSoftwareVersion::new("0.1.0") {
            Ok(version) => version,

            Err(error) => {
                panic!("valid software version failed: {error}")
            }
        }
    }

    fn envelope(local_sequence_number: u64) -> CanonicalTransactionEnvelope {
        let json =
            format!("{{\"schema_version\":1,\"local_sequence_number\":{local_sequence_number}}}");

        match CanonicalTransactionEnvelope::from_json(&json) {
            Ok(envelope) => envelope,

            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        }
    }

    fn request(
        reader_id: ReaderId,
        batch_id: SynchronizationBatchId,
        transaction_ids: [FareTransactionId; 2],
    ) -> SynchronizationBatchRequest {
        let entries = vec![
            SynchronizationRequestEntry::new(transaction_ids[0], sequence(10), envelope(10)),
            SynchronizationRequestEntry::new(transaction_ids[1], sequence(12), envelope(12)),
        ];

        match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            environment_id: environment(),
            reader_id,
            reader_software_version: software_version(),
            batch_id,
            first_local_sequence_number: sequence(10),
            last_local_sequence_number: sequence(12),
            submitted_at_unix_milliseconds: SUBMITTED_AT,
            entries,
        }) {
            Ok(request) => request,

            Err(error) => {
                panic!("valid request failed: {error}")
            }
        }
    }

    fn acknowledgement(
        request: &SynchronizationBatchRequest,
        batch_id: SynchronizationBatchId,
    ) -> SynchronizationBatchAcknowledgement {
        let first_entry = match SynchronizationAcknowledgementEntry::new(
            request.entries()[0].transaction_id(),
            request.entries()[0].local_sequence_number(),
            SynchronizationEntryOutcome::Acknowledged,
            None,
            None,
        ) {
            Ok(entry) => entry,

            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        };

        let second_entry = match SynchronizationAcknowledgementEntry::new(
            request.entries()[1].transaction_id(),
            request.entries()[1].local_sequence_number(),
            SynchronizationEntryOutcome::RetryableFailure,
            Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable),
            Some(RECEIVED_AT + 1_000),
        ) {
            Ok(entry) => entry,

            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        };

        match SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: request.protocol_version(),
                environment_id: request.environment_id().clone(),
                reader_id: request.reader_id(),
                batch_id,
                first_local_sequence_number: request.first_local_sequence_number(),
                last_local_sequence_number: request.last_local_sequence_number(),
                received_at_unix_milliseconds: RECEIVED_AT,
                replayed: false,
                entries: vec![first_entry, second_entry],
            },
        ) {
            Ok(acknowledgement) => acknowledgement,

            Err(error) => {
                panic!("valid acknowledgement failed: {error}")
            }
        }
    }

    #[test]
    fn prepared_ingest_preserves_batch_identity() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();

        let request = request(
            reader_id,
            batch_id,
            [FareTransactionId::generate(), FareTransactionId::generate()],
        );

        let acknowledgement = acknowledgement(&request, batch_id);

        let prepared = match PreparedSynchronizationIngest::prepare(&request, &acknowledgement) {
            Ok(prepared) => prepared,

            Err(error) => {
                panic!("ingest preparation failed: {error}")
            }
        };

        assert_eq!(prepared.reader_id(), reader_id);
        assert_eq!(prepared.batch_id(), batch_id);
        assert_eq!(prepared.entry_count(), 2);
        assert_eq!(prepared.first_local_sequence_number().value(), 10);
        assert_eq!(prepared.last_local_sequence_number().value(), 12);
        assert_eq!(prepared.submitted_at_unix_milliseconds(), SUBMITTED_AT);
        assert_eq!(prepared.received_at_unix_milliseconds(), RECEIVED_AT);
    }

    #[test]
    fn prepared_ingest_contains_json_objects() {
        let batch_id = SynchronizationBatchId::generate();

        let request = request(
            ReaderId::generate(),
            batch_id,
            [FareTransactionId::generate(), FareTransactionId::generate()],
        );

        let acknowledgement = acknowledgement(&request, batch_id);

        let prepared = match PreparedSynchronizationIngest::prepare(&request, &acknowledgement) {
            Ok(prepared) => prepared,

            Err(error) => {
                panic!("ingest preparation failed: {error}")
            }
        };

        assert!(prepared.canonical_request_json().is_object());
        assert!(prepared.canonical_acknowledgement_json().is_object());

        assert!(
            prepared.entries()[0]
                .canonical_transaction_envelope_json()
                .is_object()
        );

        assert!(
            prepared.entries()[1]
                .canonical_transaction_envelope_json()
                .is_object()
        );
    }

    #[test]
    fn prepared_ingest_preserves_mixed_outcomes() {
        let batch_id = SynchronizationBatchId::generate();

        let request = request(
            ReaderId::generate(),
            batch_id,
            [FareTransactionId::generate(), FareTransactionId::generate()],
        );

        let acknowledgement = acknowledgement(&request, batch_id);

        let prepared = match PreparedSynchronizationIngest::prepare(&request, &acknowledgement) {
            Ok(prepared) => prepared,

            Err(error) => {
                panic!("ingest preparation failed: {error}")
            }
        };

        assert_eq!(prepared.entries()[0].entry_position(), 0);

        assert_eq!(
            prepared.entries()[0].outcome(),
            SynchronizationEntryOutcome::Acknowledged
        );

        assert_eq!(prepared.entries()[1].entry_position(), 1);

        assert_eq!(
            prepared.entries()[1].outcome(),
            SynchronizationEntryOutcome::RetryableFailure
        );

        assert_eq!(
            prepared.entries()[1].failure_category(),
            Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable)
        );

        assert_eq!(
            prepared.entries()[1].next_retry_at_unix_milliseconds(),
            Some(RECEIVED_AT + 1_000)
        );
    }

    #[test]
    fn mismatched_batch_acknowledgement_is_rejected() {
        let batch_id = SynchronizationBatchId::generate();

        let request = request(
            ReaderId::generate(),
            batch_id,
            [FareTransactionId::generate(), FareTransactionId::generate()],
        );

        let acknowledgement = acknowledgement(&request, SynchronizationBatchId::generate());

        let result = PreparedSynchronizationIngest::prepare(&request, &acknowledgement);

        assert!(matches!(
            result,
            Err(SynchronizationIngestRecordError::AcknowledgementMismatch(_))
        ));
    }

    #[test]
    fn prepared_fingerprints_match_protocol_values() {
        let batch_id = SynchronizationBatchId::generate();

        let request = request(
            ReaderId::generate(),
            batch_id,
            [FareTransactionId::generate(), FareTransactionId::generate()],
        );

        let acknowledgement = acknowledgement(&request, batch_id);

        let prepared = match PreparedSynchronizationIngest::prepare(&request, &acknowledgement) {
            Ok(prepared) => prepared,

            Err(error) => {
                panic!("ingest preparation failed: {error}")
            }
        };

        assert_eq!(prepared.request_fingerprint(), request.fingerprint());

        assert_eq!(
            prepared.acknowledgement_fingerprint(),
            transitguard_device_protocol::SynchronizationPayloadFingerprint::for_acknowledgement(
                &acknowledgement
            )
        );

        assert_eq!(
            prepared.entries()[0].transaction_fingerprint(),
            transitguard_device_protocol::SynchronizationPayloadFingerprint::for_transaction_envelope(
                request.entries()[0].transaction_envelope()
            )
        );
    }
}
