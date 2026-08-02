use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;
use transitguard_domain::{
    FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};

use crate::DeviceProtocolVersion;

/// HTTP endpoint used for project-owned reader synchronization.
pub const SYNCHRONIZATION_BATCH_ENDPOINT: &str = "/v1/reader-synchronization/batches";

/// HTTP header carrying the durable synchronization batch identity.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

/// HTTP header carrying the project-owned protocol version.
pub const PROTOCOL_VERSION_HEADER: &str = "X-TransitGuard-Protocol-Version";

/// Maximum entries accepted in one protocol-version-one batch.
pub const MAX_SYNCHRONIZATION_BATCH_ENTRIES: usize = 256;

/// Maximum canonical bytes accepted for one transaction envelope.
pub const MAX_TRANSACTION_ENVELOPE_BYTES: usize = 64 * 1024;

/// Maximum decoded synchronization request size.
pub const MAX_SYNCHRONIZATION_REQUEST_BYTES: usize = 1024 * 1024;

/// Maximum decoded synchronization acknowledgement size.
pub const MAX_SYNCHRONIZATION_ACKNOWLEDGEMENT_BYTES: usize = 1024 * 1024;

const MAX_ENVIRONMENT_ID_BYTES: usize = 128;
const MAX_SOFTWARE_VERSION_BYTES: usize = 128;

/// Validation errors produced by the project-owned synchronization protocol.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SynchronizationProtocolError {
    /// A required textual value was empty.
    #[error("synchronization field `{field}` cannot be empty")]
    EmptyText {
        /// Stable protocol field name.
        field: &'static str,
    },

    /// A bounded textual value exceeded its protocol limit.
    #[error("synchronization field `{field}` exceeds {max_bytes} bytes: {actual_bytes}")]
    TextTooLong {
        /// Stable protocol field name.
        field: &'static str,

        /// Maximum permitted UTF-8 bytes.
        max_bytes: usize,

        /// Actual UTF-8 bytes.
        actual_bytes: usize,
    },

    /// A textual identity contained control characters.
    #[error("synchronization field `{field}` cannot contain control characters")]
    ControlCharacter {
        /// Stable protocol field name.
        field: &'static str,
    },

    /// A transaction envelope was not valid JSON.
    #[error("transaction envelope must contain valid JSON")]
    InvalidTransactionEnvelopeJson,

    /// A transaction envelope was not a JSON object.
    #[error("transaction envelope must be a JSON object")]
    TransactionEnvelopeNotObject,

    /// A transaction envelope exceeded its canonical size limit.
    #[error("transaction envelope exceeds {max_bytes} bytes: {actual_bytes}")]
    TransactionEnvelopeTooLarge {
        /// Maximum permitted bytes.
        max_bytes: usize,

        /// Actual canonical bytes.
        actual_bytes: usize,
    },

    /// A protocol timestamp was before the Unix epoch.
    #[error("synchronization timestamp `{field}` cannot be negative: {unix_milliseconds}")]
    NegativeTimestamp {
        /// Stable timestamp field name.
        field: &'static str,

        /// Invalid Unix timestamp.
        unix_milliseconds: i64,
    },

    /// A synchronization batch contained no entries.
    #[error("synchronization batch must contain at least one entry")]
    EmptyBatch,

    /// A synchronization batch exceeded its entry limit.
    #[error("synchronization batch exceeds {max_entries} entries: {actual_entries}")]
    TooManyEntries {
        /// Maximum permitted entries.
        max_entries: usize,

        /// Actual entries.
        actual_entries: usize,
    },

    /// The declared range was reversed.
    #[error("synchronization range is invalid: first {first_sequence}, last {last_sequence}")]
    InvalidSequenceRange {
        /// Declared first sequence.
        first_sequence: u64,

        /// Declared last sequence.
        last_sequence: u64,
    },

    /// The first entry did not match the declared range.
    #[error(
        "first synchronization entry sequence {actual_sequence} does not match declared first sequence {declared_sequence}"
    )]
    FirstSequenceMismatch {
        /// Declared first sequence.
        declared_sequence: u64,

        /// Actual first entry sequence.
        actual_sequence: u64,
    },

    /// The final entry did not match the declared range.
    #[error(
        "last synchronization entry sequence {actual_sequence} does not match declared last sequence {declared_sequence}"
    )]
    LastSequenceMismatch {
        /// Declared last sequence.
        declared_sequence: u64,

        /// Actual final entry sequence.
        actual_sequence: u64,
    },

    /// Entry sequences were not strictly increasing.
    #[error("synchronization entry sequence {current_sequence} must follow {previous_sequence}")]
    EntrySequenceNotIncreasing {
        /// Previous sequence.
        previous_sequence: u64,

        /// Current invalid sequence.
        current_sequence: u64,
    },

    /// One transaction identity occurred more than once.
    #[error("duplicate synchronization transaction identity: {transaction_id:?}")]
    DuplicateTransaction {
        /// Duplicate transaction identity.
        transaction_id: FareTransactionId,
    },

    /// Outcome metadata did not match the selected outcome.
    #[error("synchronization outcome metadata is invalid for {outcome:?}")]
    InvalidOutcomeMetadata {
        /// Outcome with invalid metadata.
        outcome: SynchronizationEntryOutcome,
    },

    /// Acknowledgement identity did not match its submitted request.
    #[error("synchronization acknowledgement field `{field}` does not match the request")]
    AcknowledgementIdentityMismatch {
        /// Stable mismatched field name.
        field: &'static str,
    },

    /// The acknowledgement returned a different number of entries.
    #[error(
        "synchronization acknowledgement contains {actual_entries} entries; request contained {expected_entries}"
    )]
    AcknowledgementEntryCountMismatch {
        /// Expected request entry count.
        expected_entries: usize,

        /// Actual acknowledgement entry count.
        actual_entries: usize,
    },

    /// An acknowledgement entry returned a different transaction.
    #[error("synchronization acknowledgement transaction differs at position {position}")]
    AcknowledgementTransactionMismatch {
        /// Zero-based entry position.
        position: usize,
    },

    /// An acknowledgement entry returned a different sequence.
    #[error(
        "synchronization acknowledgement sequence differs at position {position}: expected {expected_sequence}, actual {actual_sequence}"
    )]
    AcknowledgementSequenceMismatch {
        /// Zero-based entry position.
        position: usize,

        /// Expected request sequence.
        expected_sequence: u64,

        /// Actual acknowledgement sequence.
        actual_sequence: u64,
    },
}

/// Backend environment associated with a reader synchronization request.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProtocolEnvironmentId(String);

impl ProtocolEnvironmentId {
    /// Creates a normalized, bounded environment identity.
    pub fn new(value: impl Into<String>) -> Result<Self, SynchronizationProtocolError> {
        Ok(Self(validate_text(
            "environment_id",
            value.into(),
            MAX_ENVIRONMENT_ID_BYTES,
        )?))
    }

    /// Returns the normalized environment identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns its string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProtocolEnvironmentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Reader software version included in synchronization evidence.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReaderSoftwareVersion(String);

impl ReaderSoftwareVersion {
    /// Creates a normalized, bounded software version.
    pub fn new(value: impl Into<String>) -> Result<Self, SynchronizationProtocolError> {
        Ok(Self(validate_text(
            "reader_software_version",
            value.into(),
            MAX_SOFTWARE_VERSION_BYTES,
        )?))
    }

    /// Returns the normalized software version.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns its string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for ReaderSoftwareVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Canonical JSON object transported for one fictional fare transaction.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CanonicalTransactionEnvelope(String);

impl CanonicalTransactionEnvelope {
    /// Parses and canonicalizes a JSON transaction envelope.
    pub fn from_json(value: &str) -> Result<Self, SynchronizationProtocolError> {
        if value.len() > MAX_TRANSACTION_ENVELOPE_BYTES {
            return Err(SynchronizationProtocolError::TransactionEnvelopeTooLarge {
                max_bytes: MAX_TRANSACTION_ENVELOPE_BYTES,
                actual_bytes: value.len(),
            });
        }

        let parsed = serde_json::from_str::<Value>(value)
            .map_err(|_| SynchronizationProtocolError::InvalidTransactionEnvelopeJson)?;

        if !parsed.is_object() {
            return Err(SynchronizationProtocolError::TransactionEnvelopeNotObject);
        }

        let canonical = serde_json::to_string(&parsed)
            .map_err(|_| SynchronizationProtocolError::InvalidTransactionEnvelopeJson)?;

        if canonical.len() > MAX_TRANSACTION_ENVELOPE_BYTES {
            return Err(SynchronizationProtocolError::TransactionEnvelopeTooLarge {
                max_bytes: MAX_TRANSACTION_ENVELOPE_BYTES,
                actual_bytes: canonical.len(),
            });
        }

        Ok(Self(canonical))
    }

    /// Returns the canonical JSON.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the canonical envelope size.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.0.len()
    }

    /// Consumes the envelope and returns canonical JSON.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for CanonicalTransactionEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        Self::from_json(&value).map_err(serde::de::Error::custom)
    }
}

/// Stable failure categories used by transport and backend ingest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizationFailureCategory {
    /// A request exceeded its configured timeout.
    NetworkTimeout,

    /// A network connection could not be established.
    ConnectionFailure,

    /// A response could not be decoded safely.
    ResponseDecodeFailure,

    /// A protocol payload exceeded a configured limit.
    PayloadTooLarge,

    /// The supplied protocol version is unsupported.
    UnsupportedProtocol,

    /// The reader is not registered.
    ReaderNotRegistered,

    /// The reader is registered but cannot currently operate.
    ReaderNotOperational,

    /// The reader and backend environments differ.
    EnvironmentMismatch,

    /// An existing batch identity has different content.
    BatchIdentityConflict,

    /// Declared and actual sequence ranges differ.
    BatchRangeMismatch,

    /// Batch entries are ordered inconsistently.
    EntryOrderMismatch,

    /// A transaction identity conflicts with stored backend state.
    TransactionIdentityConflict,

    /// A required backend dependency is temporarily unavailable.
    BackendTemporarilyUnavailable,

    /// Backend validation rejected an entry.
    BackendValidationFailure,

    /// Automated processing requires operator review.
    ManualReviewRequired,
}

impl SynchronizationFailureCategory {
    /// Returns the stable snake-case category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NetworkTimeout => "network_timeout",
            Self::ConnectionFailure => "connection_failure",
            Self::ResponseDecodeFailure => "response_decode_failure",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::ReaderNotRegistered => "reader_not_registered",
            Self::ReaderNotOperational => "reader_not_operational",
            Self::EnvironmentMismatch => "environment_mismatch",
            Self::BatchIdentityConflict => "batch_identity_conflict",
            Self::BatchRangeMismatch => "batch_range_mismatch",
            Self::EntryOrderMismatch => "entry_order_mismatch",
            Self::TransactionIdentityConflict => "transaction_identity_conflict",
            Self::BackendTemporarilyUnavailable => "backend_temporarily_unavailable",
            Self::BackendValidationFailure => "backend_validation_failure",
            Self::ManualReviewRequired => "manual_review_required",
        }
    }
}

/// One ordered transaction entry submitted by a reader.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SynchronizationRequestEntry {
    transaction_id: FareTransactionId,
    local_sequence_number: LocalSequenceNumber,
    transaction_envelope: CanonicalTransactionEnvelope,
}

impl SynchronizationRequestEntry {
    /// Creates a synchronization request entry.
    #[must_use]
    pub const fn new(
        transaction_id: FareTransactionId,
        local_sequence_number: LocalSequenceNumber,
        transaction_envelope: CanonicalTransactionEnvelope,
    ) -> Self {
        Self {
            transaction_id,
            local_sequence_number,
            transaction_envelope,
        }
    }

    /// Returns the stable fare transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader-local sequence.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the canonical transaction envelope.
    #[must_use]
    pub const fn transaction_envelope(&self) -> &CanonicalTransactionEnvelope {
        &self.transaction_envelope
    }
}

impl<'de> Deserialize<'de> for SynchronizationRequestEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Representation {
            transaction_id: FareTransactionId,
            local_sequence_number: LocalSequenceNumber,
            transaction_envelope: CanonicalTransactionEnvelope,
        }

        let value = Representation::deserialize(deserializer)?;

        Ok(Self::new(
            value.transaction_id,
            value.local_sequence_number,
            value.transaction_envelope,
        ))
    }
}

/// Construction values for one synchronization request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SynchronizationBatchRequestDefinition {
    /// Project-owned protocol version.
    pub protocol_version: DeviceProtocolVersion,

    /// Backend environment identity.
    pub environment_id: ProtocolEnvironmentId,

    /// Durable reader identity.
    pub reader_id: ReaderId,

    /// Reader software version.
    pub reader_software_version: ReaderSoftwareVersion,

    /// Durable synchronization batch identity.
    pub batch_id: SynchronizationBatchId,

    /// Declared first local sequence.
    pub first_local_sequence_number: LocalSequenceNumber,

    /// Declared last local sequence.
    pub last_local_sequence_number: LocalSequenceNumber,

    /// Submission time in Unix milliseconds.
    pub submitted_at_unix_milliseconds: i64,

    /// Ordered batch entries.
    pub entries: Vec<SynchronizationRequestEntry>,
}

/// Complete versioned synchronization request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SynchronizationBatchRequest {
    protocol_version: DeviceProtocolVersion,
    environment_id: ProtocolEnvironmentId,
    reader_id: ReaderId,
    reader_software_version: ReaderSoftwareVersion,
    batch_id: SynchronizationBatchId,
    first_local_sequence_number: LocalSequenceNumber,
    last_local_sequence_number: LocalSequenceNumber,
    submitted_at_unix_milliseconds: i64,
    entries: Vec<SynchronizationRequestEntry>,
}

impl SynchronizationBatchRequest {
    /// Creates and validates a complete synchronization request.
    pub fn new(
        definition: SynchronizationBatchRequestDefinition,
    ) -> Result<Self, SynchronizationProtocolError> {
        validate_timestamp(
            "submitted_at_unix_milliseconds",
            definition.submitted_at_unix_milliseconds,
        )?;

        validate_request_entries(
            definition.first_local_sequence_number,
            definition.last_local_sequence_number,
            &definition.entries,
        )?;

        Ok(Self {
            protocol_version: definition.protocol_version,
            environment_id: definition.environment_id,
            reader_id: definition.reader_id,
            reader_software_version: definition.reader_software_version,
            batch_id: definition.batch_id,
            first_local_sequence_number: definition.first_local_sequence_number,
            last_local_sequence_number: definition.last_local_sequence_number,
            submitted_at_unix_milliseconds: definition.submitted_at_unix_milliseconds,
            entries: definition.entries,
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

    /// Returns the durable batch identity.
    #[must_use]
    pub const fn batch_id(&self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns the declared first local sequence.
    #[must_use]
    pub const fn first_local_sequence_number(&self) -> LocalSequenceNumber {
        self.first_local_sequence_number
    }

    /// Returns the declared last local sequence.
    #[must_use]
    pub const fn last_local_sequence_number(&self) -> LocalSequenceNumber {
        self.last_local_sequence_number
    }

    /// Returns the submission timestamp.
    #[must_use]
    pub const fn submitted_at_unix_milliseconds(&self) -> i64 {
        self.submitted_at_unix_milliseconds
    }

    /// Returns the ordered request entries.
    #[must_use]
    pub fn entries(&self) -> &[SynchronizationRequestEntry] {
        &self.entries
    }
}

impl<'de> Deserialize<'de> for SynchronizationBatchRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = SynchronizationBatchRequestDefinition::deserialize(deserializer)?;

        Self::new(definition).map_err(serde::de::Error::custom)
    }
}

/// Stable per-entry acknowledgement outcomes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizationEntryOutcome {
    /// The backend accepted the transaction.
    Acknowledged,

    /// The transaction may be submitted in a future batch.
    RetryableFailure,

    /// The backend returned a final automated rejection.
    PermanentFailure,

    /// The transaction requires operator review.
    ManualReview,
}

/// One ordered result in a synchronization acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SynchronizationAcknowledgementEntry {
    transaction_id: FareTransactionId,
    local_sequence_number: LocalSequenceNumber,
    outcome: SynchronizationEntryOutcome,
    failure_category: Option<SynchronizationFailureCategory>,
    next_retry_at_unix_milliseconds: Option<i64>,
}

impl SynchronizationAcknowledgementEntry {
    /// Creates and validates one acknowledgement result.
    pub fn new(
        transaction_id: FareTransactionId,
        local_sequence_number: LocalSequenceNumber,
        outcome: SynchronizationEntryOutcome,
        failure_category: Option<SynchronizationFailureCategory>,
        next_retry_at_unix_milliseconds: Option<i64>,
    ) -> Result<Self, SynchronizationProtocolError> {
        validate_outcome_metadata(outcome, failure_category, next_retry_at_unix_milliseconds)?;

        Ok(Self {
            transaction_id,
            local_sequence_number,
            outcome,
            failure_category,
            next_retry_at_unix_milliseconds,
        })
    }

    /// Returns the stable transaction identity.
    #[must_use]
    pub const fn transaction_id(&self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader-local sequence.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the backend outcome.
    #[must_use]
    pub const fn outcome(&self) -> SynchronizationEntryOutcome {
        self.outcome
    }

    /// Returns the sanitized failure category.
    #[must_use]
    pub const fn failure_category(&self) -> Option<SynchronizationFailureCategory> {
        self.failure_category
    }

    /// Returns the earliest retry time.
    #[must_use]
    pub const fn next_retry_at_unix_milliseconds(&self) -> Option<i64> {
        self.next_retry_at_unix_milliseconds
    }
}

impl<'de> Deserialize<'de> for SynchronizationAcknowledgementEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Representation {
            transaction_id: FareTransactionId,
            local_sequence_number: LocalSequenceNumber,
            outcome: SynchronizationEntryOutcome,
            failure_category: Option<SynchronizationFailureCategory>,
            next_retry_at_unix_milliseconds: Option<i64>,
        }

        let value = Representation::deserialize(deserializer)?;

        Self::new(
            value.transaction_id,
            value.local_sequence_number,
            value.outcome,
            value.failure_category,
            value.next_retry_at_unix_milliseconds,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Construction values for a complete acknowledgement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SynchronizationBatchAcknowledgementDefinition {
    /// Project-owned protocol version.
    pub protocol_version: DeviceProtocolVersion,

    /// Backend environment identity.
    pub environment_id: ProtocolEnvironmentId,

    /// Durable reader identity.
    pub reader_id: ReaderId,

    /// Durable synchronization batch identity.
    pub batch_id: SynchronizationBatchId,

    /// Declared first local sequence.
    pub first_local_sequence_number: LocalSequenceNumber,

    /// Declared last local sequence.
    pub last_local_sequence_number: LocalSequenceNumber,

    /// Backend receipt time in Unix milliseconds.
    pub received_at_unix_milliseconds: i64,

    /// Whether the backend returned a stored identical replay.
    pub replayed: bool,

    /// Ordered per-entry outcomes.
    pub entries: Vec<SynchronizationAcknowledgementEntry>,
}

/// Complete backend acknowledgement for one durable batch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SynchronizationBatchAcknowledgement {
    protocol_version: DeviceProtocolVersion,
    environment_id: ProtocolEnvironmentId,
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    first_local_sequence_number: LocalSequenceNumber,
    last_local_sequence_number: LocalSequenceNumber,
    received_at_unix_milliseconds: i64,
    replayed: bool,
    entries: Vec<SynchronizationAcknowledgementEntry>,
}

impl SynchronizationBatchAcknowledgement {
    /// Creates and validates a complete acknowledgement.
    pub fn new(
        definition: SynchronizationBatchAcknowledgementDefinition,
    ) -> Result<Self, SynchronizationProtocolError> {
        validate_timestamp(
            "received_at_unix_milliseconds",
            definition.received_at_unix_milliseconds,
        )?;

        validate_acknowledgement_entries(
            definition.first_local_sequence_number,
            definition.last_local_sequence_number,
            &definition.entries,
        )?;

        Ok(Self {
            protocol_version: definition.protocol_version,
            environment_id: definition.environment_id,
            reader_id: definition.reader_id,
            batch_id: definition.batch_id,
            first_local_sequence_number: definition.first_local_sequence_number,
            last_local_sequence_number: definition.last_local_sequence_number,
            received_at_unix_milliseconds: definition.received_at_unix_milliseconds,
            replayed: definition.replayed,
            entries: definition.entries,
        })
    }

    /// Validates the acknowledgement against its submitted request.
    pub fn validate_against_request(
        &self,
        request: &SynchronizationBatchRequest,
    ) -> Result<(), SynchronizationProtocolError> {
        if self.protocol_version != request.protocol_version {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch {
                    field: "protocol_version",
                },
            );
        }

        if self.environment_id != request.environment_id {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch {
                    field: "environment_id",
                },
            );
        }

        if self.reader_id != request.reader_id {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch {
                    field: "reader_id",
                },
            );
        }

        if self.batch_id != request.batch_id {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch { field: "batch_id" },
            );
        }

        if self.first_local_sequence_number != request.first_local_sequence_number {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch {
                    field: "first_local_sequence_number",
                },
            );
        }

        if self.last_local_sequence_number != request.last_local_sequence_number {
            return Err(
                SynchronizationProtocolError::AcknowledgementIdentityMismatch {
                    field: "last_local_sequence_number",
                },
            );
        }

        if self.entries.len() != request.entries.len() {
            return Err(
                SynchronizationProtocolError::AcknowledgementEntryCountMismatch {
                    expected_entries: request.entries.len(),
                    actual_entries: self.entries.len(),
                },
            );
        }

        for (position, (acknowledgement, submitted)) in
            self.entries.iter().zip(request.entries.iter()).enumerate()
        {
            if acknowledgement.transaction_id != submitted.transaction_id {
                return Err(
                    SynchronizationProtocolError::AcknowledgementTransactionMismatch { position },
                );
            }

            if acknowledgement.local_sequence_number != submitted.local_sequence_number {
                return Err(
                    SynchronizationProtocolError::AcknowledgementSequenceMismatch {
                        position,
                        expected_sequence: submitted.local_sequence_number.value(),
                        actual_sequence: acknowledgement.local_sequence_number.value(),
                    },
                );
            }
        }

        Ok(())
    }

    /// Returns the project-owned protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the backend environment.
    #[must_use]
    pub const fn environment_id(&self) -> &ProtocolEnvironmentId {
        &self.environment_id
    }

    /// Returns the durable reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the durable synchronization batch identity.
    #[must_use]
    pub const fn batch_id(&self) -> SynchronizationBatchId {
        self.batch_id
    }

    /// Returns the declared first local sequence.
    #[must_use]
    pub const fn first_local_sequence_number(&self) -> LocalSequenceNumber {
        self.first_local_sequence_number
    }

    /// Returns the declared last local sequence.
    #[must_use]
    pub const fn last_local_sequence_number(&self) -> LocalSequenceNumber {
        self.last_local_sequence_number
    }

    /// Returns the backend receipt time.
    #[must_use]
    pub const fn received_at_unix_milliseconds(&self) -> i64 {
        self.received_at_unix_milliseconds
    }

    /// Returns whether the response represents an identical replay.
    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }

    /// Returns this acknowledgement with the replay indicator changed.
    #[must_use]
    pub fn with_replayed(mut self, replayed: bool) -> Self {
        self.replayed = replayed;
        self
    }

    /// Returns the ordered acknowledgement entries.
    #[must_use]
    pub fn entries(&self) -> &[SynchronizationAcknowledgementEntry] {
        &self.entries
    }
}

impl<'de> Deserialize<'de> for SynchronizationBatchAcknowledgement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = SynchronizationBatchAcknowledgementDefinition::deserialize(deserializer)?;

        Self::new(definition).map_err(serde::de::Error::custom)
    }
}

fn validate_text(
    field: &'static str,
    value: String,
    max_bytes: usize,
) -> Result<String, SynchronizationProtocolError> {
    let normalized = value.trim();

    if normalized.is_empty() {
        return Err(SynchronizationProtocolError::EmptyText { field });
    }

    if normalized.len() > max_bytes {
        return Err(SynchronizationProtocolError::TextTooLong {
            field,
            max_bytes,
            actual_bytes: normalized.len(),
        });
    }

    if normalized.chars().any(char::is_control) {
        return Err(SynchronizationProtocolError::ControlCharacter { field });
    }

    Ok(normalized.to_owned())
}

fn validate_timestamp(
    field: &'static str,
    unix_milliseconds: i64,
) -> Result<(), SynchronizationProtocolError> {
    if unix_milliseconds < 0 {
        return Err(SynchronizationProtocolError::NegativeTimestamp {
            field,
            unix_milliseconds,
        });
    }

    Ok(())
}

fn validate_request_entries(
    first: LocalSequenceNumber,
    last: LocalSequenceNumber,
    entries: &[SynchronizationRequestEntry],
) -> Result<(), SynchronizationProtocolError> {
    validate_entry_count(entries.len())?;
    validate_range(first, last)?;

    let first_entry = match entries.first() {
        Some(value) => value,
        None => {
            return Err(SynchronizationProtocolError::EmptyBatch);
        }
    };

    let last_entry = match entries.last() {
        Some(value) => value,
        None => {
            return Err(SynchronizationProtocolError::EmptyBatch);
        }
    };

    validate_boundary_sequences(
        first,
        last,
        first_entry.local_sequence_number,
        last_entry.local_sequence_number,
    )?;

    let mut transaction_ids = BTreeSet::new();

    for entry in entries {
        if !transaction_ids.insert(entry.transaction_id) {
            return Err(SynchronizationProtocolError::DuplicateTransaction {
                transaction_id: entry.transaction_id,
            });
        }
    }

    validate_increasing_sequences(entries.iter().map(|entry| entry.local_sequence_number))
}

fn validate_acknowledgement_entries(
    first: LocalSequenceNumber,
    last: LocalSequenceNumber,
    entries: &[SynchronizationAcknowledgementEntry],
) -> Result<(), SynchronizationProtocolError> {
    validate_entry_count(entries.len())?;
    validate_range(first, last)?;

    let first_entry = match entries.first() {
        Some(value) => value,
        None => {
            return Err(SynchronizationProtocolError::EmptyBatch);
        }
    };

    let last_entry = match entries.last() {
        Some(value) => value,
        None => {
            return Err(SynchronizationProtocolError::EmptyBatch);
        }
    };

    validate_boundary_sequences(
        first,
        last,
        first_entry.local_sequence_number,
        last_entry.local_sequence_number,
    )?;

    let mut transaction_ids = BTreeSet::new();

    for entry in entries {
        if !transaction_ids.insert(entry.transaction_id) {
            return Err(SynchronizationProtocolError::DuplicateTransaction {
                transaction_id: entry.transaction_id,
            });
        }
    }

    validate_increasing_sequences(entries.iter().map(|entry| entry.local_sequence_number))
}

fn validate_entry_count(entry_count: usize) -> Result<(), SynchronizationProtocolError> {
    if entry_count == 0 {
        return Err(SynchronizationProtocolError::EmptyBatch);
    }

    if entry_count > MAX_SYNCHRONIZATION_BATCH_ENTRIES {
        return Err(SynchronizationProtocolError::TooManyEntries {
            max_entries: MAX_SYNCHRONIZATION_BATCH_ENTRIES,
            actual_entries: entry_count,
        });
    }

    Ok(())
}

fn validate_range(
    first: LocalSequenceNumber,
    last: LocalSequenceNumber,
) -> Result<(), SynchronizationProtocolError> {
    if first > last {
        return Err(SynchronizationProtocolError::InvalidSequenceRange {
            first_sequence: first.value(),
            last_sequence: last.value(),
        });
    }

    Ok(())
}

fn validate_boundary_sequences(
    declared_first: LocalSequenceNumber,
    declared_last: LocalSequenceNumber,
    actual_first: LocalSequenceNumber,
    actual_last: LocalSequenceNumber,
) -> Result<(), SynchronizationProtocolError> {
    if declared_first != actual_first {
        return Err(SynchronizationProtocolError::FirstSequenceMismatch {
            declared_sequence: declared_first.value(),
            actual_sequence: actual_first.value(),
        });
    }

    if declared_last != actual_last {
        return Err(SynchronizationProtocolError::LastSequenceMismatch {
            declared_sequence: declared_last.value(),
            actual_sequence: actual_last.value(),
        });
    }

    Ok(())
}

fn validate_increasing_sequences(
    sequences: impl IntoIterator<Item = LocalSequenceNumber>,
) -> Result<(), SynchronizationProtocolError> {
    let mut previous = None;

    for current in sequences {
        if let Some(previous_value) = previous
            && current <= previous_value
        {
            return Err(SynchronizationProtocolError::EntrySequenceNotIncreasing {
                previous_sequence: previous_value.value(),
                current_sequence: current.value(),
            });
        }

        previous = Some(current);
    }

    Ok(())
}

fn validate_outcome_metadata(
    outcome: SynchronizationEntryOutcome,
    failure_category: Option<SynchronizationFailureCategory>,
    next_retry_at_unix_milliseconds: Option<i64>,
) -> Result<(), SynchronizationProtocolError> {
    if let Some(retry_at) = next_retry_at_unix_milliseconds {
        validate_timestamp("next_retry_at_unix_milliseconds", retry_at)?;
    }

    let valid = match outcome {
        SynchronizationEntryOutcome::Acknowledged => {
            failure_category.is_none() && next_retry_at_unix_milliseconds.is_none()
        }

        SynchronizationEntryOutcome::RetryableFailure => {
            failure_category.is_some() && next_retry_at_unix_milliseconds.is_some()
        }

        SynchronizationEntryOutcome::PermanentFailure
        | SynchronizationEntryOutcome::ManualReview => {
            failure_category.is_some() && next_retry_at_unix_milliseconds.is_none()
        }
    };

    if !valid {
        return Err(SynchronizationProtocolError::InvalidOutcomeMetadata { outcome });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use crate::DeviceProtocolVersion;

    use super::{
        CanonicalTransactionEnvelope, MAX_SYNCHRONIZATION_BATCH_ENTRIES, ProtocolEnvironmentId,
        ReaderSoftwareVersion, SynchronizationAcknowledgementEntry,
        SynchronizationBatchAcknowledgement, SynchronizationBatchAcknowledgementDefinition,
        SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
        SynchronizationEntryOutcome, SynchronizationFailureCategory, SynchronizationProtocolError,
        SynchronizationRequestEntry,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

    fn sequence(value: u64) -> LocalSequenceNumber {
        match LocalSequenceNumber::new(value) {
            Ok(sequence) => sequence,
            Err(error) => {
                panic!("valid local sequence failed: {error}")
            }
        }
    }

    fn environment() -> ProtocolEnvironmentId {
        match ProtocolEnvironmentId::new("development") {
            Ok(value) => value,
            Err(error) => {
                panic!("valid environment failed: {error}")
            }
        }
    }

    fn software_version() -> ReaderSoftwareVersion {
        match ReaderSoftwareVersion::new("0.1.0") {
            Ok(value) => value,
            Err(error) => {
                panic!("valid software version failed: {error}")
            }
        }
    }

    fn envelope(value: u64) -> CanonicalTransactionEnvelope {
        let json = format!("{{\"schema_version\":1,\"value\":{value}}}");

        match CanonicalTransactionEnvelope::from_json(&json) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        }
    }

    fn request_entry(
        transaction_id: FareTransactionId,
        sequence_value: u64,
    ) -> SynchronizationRequestEntry {
        SynchronizationRequestEntry::new(
            transaction_id,
            sequence(sequence_value),
            envelope(sequence_value),
        )
    }

    fn request_with_entries(
        reader_id: ReaderId,
        batch_id: SynchronizationBatchId,
        entries: Vec<SynchronizationRequestEntry>,
    ) -> Result<SynchronizationBatchRequest, SynchronizationProtocolError> {
        let first = match entries.first() {
            Some(entry) => entry.local_sequence_number(),
            None => sequence(1),
        };

        let last = match entries.last() {
            Some(entry) => entry.local_sequence_number(),
            None => sequence(1),
        };

        SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            environment_id: environment(),
            reader_id,
            reader_software_version: software_version(),
            batch_id,
            first_local_sequence_number: first,
            last_local_sequence_number: last,
            submitted_at_unix_milliseconds: TEST_TIME,
            entries,
        })
    }

    fn acknowledgement_entry(
        request_entry: &SynchronizationRequestEntry,
        outcome: SynchronizationEntryOutcome,
    ) -> SynchronizationAcknowledgementEntry {
        let metadata = match outcome {
            SynchronizationEntryOutcome::Acknowledged => (None, None),

            SynchronizationEntryOutcome::RetryableFailure => (
                Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable),
                Some(TEST_TIME + 1_000),
            ),

            SynchronizationEntryOutcome::PermanentFailure => (
                Some(SynchronizationFailureCategory::BackendValidationFailure),
                None,
            ),

            SynchronizationEntryOutcome::ManualReview => (
                Some(SynchronizationFailureCategory::ManualReviewRequired),
                None,
            ),
        };

        match SynchronizationAcknowledgementEntry::new(
            request_entry.transaction_id(),
            request_entry.local_sequence_number(),
            outcome,
            metadata.0,
            metadata.1,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        }
    }

    fn acknowledgement_for(
        request: &SynchronizationBatchRequest,
    ) -> SynchronizationBatchAcknowledgement {
        let entries = request
            .entries()
            .iter()
            .map(|entry| acknowledgement_entry(entry, SynchronizationEntryOutcome::Acknowledged))
            .collect();

        match SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: request.protocol_version(),
                environment_id: request.environment_id().clone(),
                reader_id: request.reader_id(),
                batch_id: request.batch_id(),
                first_local_sequence_number: request.first_local_sequence_number(),
                last_local_sequence_number: request.last_local_sequence_number(),
                received_at_unix_milliseconds: TEST_TIME + 500,
                replayed: false,
                entries,
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid acknowledgement failed: {error}")
            }
        }
    }

    #[test]
    fn transaction_envelope_is_canonicalized() {
        let envelope = CanonicalTransactionEnvelope::from_json("{ \"z\": 2, \"a\": 1 }");

        assert!(matches!(
            envelope,
            Ok(value)
                if value.as_str() == "{\"a\":1,\"z\":2}"
        ));
    }

    #[test]
    fn non_object_transaction_envelope_is_rejected() {
        assert_eq!(
            CanonicalTransactionEnvelope::from_json("[1,2,3]"),
            Err(SynchronizationProtocolError::TransactionEnvelopeNotObject)
        );
    }

    #[test]
    fn environment_and_version_are_normalized() {
        assert!(matches!(
            ProtocolEnvironmentId::new(" development "),
            Ok(value) if value.as_str() == "development"
        ));

        assert!(matches!(
            ReaderSoftwareVersion::new(" 0.1.0 "),
            Ok(value) if value.as_str() == "0.1.0"
        ));
    }

    #[test]
    fn valid_request_preserves_order_and_identity() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();

        let first_transaction = FareTransactionId::generate();
        let second_transaction = FareTransactionId::generate();

        let request = request_with_entries(
            reader_id,
            batch_id,
            vec![
                request_entry(first_transaction, 10),
                request_entry(second_transaction, 12),
            ],
        );

        assert!(matches!(
            request,
            Ok(value)
                if value.reader_id() == reader_id
                    && value.batch_id() == batch_id
                    && value.entries()[0].transaction_id()
                        == first_transaction
                    && value.entries()[1].transaction_id()
                        == second_transaction
                    && value
                        .first_local_sequence_number()
                        .value()
                        == 10
                    && value
                        .last_local_sequence_number()
                        .value()
                        == 12
        ));
    }

    #[test]
    fn empty_request_is_rejected() {
        let result = request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            Vec::new(),
        );

        assert_eq!(result, Err(SynchronizationProtocolError::EmptyBatch));
    }

    #[test]
    fn oversized_request_batch_is_rejected() {
        let entries = (1..=MAX_SYNCHRONIZATION_BATCH_ENTRIES + 1)
            .map(|value| request_entry(FareTransactionId::generate(), value as u64))
            .collect();

        let result = request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            entries,
        );

        assert_eq!(
            result,
            Err(SynchronizationProtocolError::TooManyEntries {
                max_entries: MAX_SYNCHRONIZATION_BATCH_ENTRIES,
                actual_entries: MAX_SYNCHRONIZATION_BATCH_ENTRIES + 1,
            })
        );
    }

    #[test]
    fn duplicate_transaction_identity_is_rejected() {
        let transaction_id = FareTransactionId::generate();

        let result = request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            vec![
                request_entry(transaction_id, 1),
                request_entry(transaction_id, 2),
            ],
        );

        assert_eq!(
            result,
            Err(SynchronizationProtocolError::DuplicateTransaction { transaction_id })
        );
    }

    #[test]
    fn entry_sequences_must_be_strictly_increasing() {
        let entries = vec![
            request_entry(FareTransactionId::generate(), 2),
            request_entry(FareTransactionId::generate(), 1),
            request_entry(FareTransactionId::generate(), 3),
        ];

        let result = SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            environment_id: environment(),
            reader_id: ReaderId::generate(),
            reader_software_version: software_version(),
            batch_id: SynchronizationBatchId::generate(),
            first_local_sequence_number: sequence(2),
            last_local_sequence_number: sequence(3),
            submitted_at_unix_milliseconds: TEST_TIME,
            entries,
        });

        assert_eq!(
            result,
            Err(SynchronizationProtocolError::EntrySequenceNotIncreasing {
                previous_sequence: 2,
                current_sequence: 1,
            })
        );
    }

    #[test]
    fn request_round_trips_through_json() {
        let request = match request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            vec![
                request_entry(FareTransactionId::generate(), 1),
                request_entry(FareTransactionId::generate(), 2),
            ],
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid request failed: {error}")
            }
        };

        let encoded = match serde_json::to_string(&request) {
            Ok(value) => value,
            Err(error) => {
                panic!("request serialization failed: {error}")
            }
        };

        let decoded = match serde_json::from_str::<SynchronizationBatchRequest>(&encoded) {
            Ok(value) => value,
            Err(error) => {
                panic!("request deserialization failed: {error}")
            }
        };

        assert_eq!(decoded, request);
    }

    #[test]
    fn acknowledged_outcome_rejects_failure_metadata() {
        let result = SynchronizationAcknowledgementEntry::new(
            FareTransactionId::generate(),
            sequence(1),
            SynchronizationEntryOutcome::Acknowledged,
            Some(SynchronizationFailureCategory::BackendValidationFailure),
            None,
        );

        assert_eq!(
            result,
            Err(SynchronizationProtocolError::InvalidOutcomeMetadata {
                outcome: SynchronizationEntryOutcome::Acknowledged,
            })
        );
    }

    #[test]
    fn retryable_outcome_requires_retry_time() {
        let result = SynchronizationAcknowledgementEntry::new(
            FareTransactionId::generate(),
            sequence(1),
            SynchronizationEntryOutcome::RetryableFailure,
            Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable),
            None,
        );

        assert_eq!(
            result,
            Err(SynchronizationProtocolError::InvalidOutcomeMetadata {
                outcome: SynchronizationEntryOutcome::RetryableFailure,
            })
        );
    }

    #[test]
    fn mixed_acknowledgement_outcomes_are_valid() {
        let request = match request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            vec![
                request_entry(FareTransactionId::generate(), 1),
                request_entry(FareTransactionId::generate(), 2),
                request_entry(FareTransactionId::generate(), 3),
                request_entry(FareTransactionId::generate(), 4),
            ],
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid request failed: {error}")
            }
        };

        let outcomes = [
            SynchronizationEntryOutcome::Acknowledged,
            SynchronizationEntryOutcome::RetryableFailure,
            SynchronizationEntryOutcome::PermanentFailure,
            SynchronizationEntryOutcome::ManualReview,
        ];

        let entries = request
            .entries()
            .iter()
            .zip(outcomes)
            .map(|(entry, outcome)| acknowledgement_entry(entry, outcome))
            .collect();

        let acknowledgement = SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: request.protocol_version(),
                environment_id: request.environment_id().clone(),
                reader_id: request.reader_id(),
                batch_id: request.batch_id(),
                first_local_sequence_number: request.first_local_sequence_number(),
                last_local_sequence_number: request.last_local_sequence_number(),
                received_at_unix_milliseconds: TEST_TIME + 500,
                replayed: false,
                entries,
            },
        );

        assert!(matches!(
            acknowledgement,
            Ok(value)
                if value.validate_against_request(&request).is_ok()
        ));
    }

    #[test]
    fn acknowledgement_transaction_mismatch_is_rejected() {
        let request = match request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            vec![request_entry(FareTransactionId::generate(), 1)],
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid request failed: {error}")
            }
        };

        let mismatched_entry = match SynchronizationAcknowledgementEntry::new(
            FareTransactionId::generate(),
            sequence(1),
            SynchronizationEntryOutcome::Acknowledged,
            None,
            None,
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        };

        let acknowledgement = match SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: request.protocol_version(),
                environment_id: request.environment_id().clone(),
                reader_id: request.reader_id(),
                batch_id: request.batch_id(),
                first_local_sequence_number: request.first_local_sequence_number(),
                last_local_sequence_number: request.last_local_sequence_number(),
                received_at_unix_milliseconds: TEST_TIME + 500,
                replayed: false,
                entries: vec![mismatched_entry],
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid acknowledgement failed: {error}")
            }
        };

        assert_eq!(
            acknowledgement.validate_against_request(&request),
            Err(SynchronizationProtocolError::AcknowledgementTransactionMismatch { position: 0 })
        );
    }

    #[test]
    fn acknowledgement_round_trips_through_json() {
        let request = match request_with_entries(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            vec![request_entry(FareTransactionId::generate(), 1)],
        ) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid request failed: {error}")
            }
        };

        let acknowledgement = acknowledgement_for(&request);

        let encoded = match serde_json::to_string(&acknowledgement) {
            Ok(value) => value,
            Err(error) => {
                panic!("acknowledgement serialization failed: {error}")
            }
        };

        let decoded = match serde_json::from_str::<SynchronizationBatchAcknowledgement>(&encoded) {
            Ok(value) => value,
            Err(error) => {
                panic!("acknowledgement deserialization failed: {error}")
            }
        };

        assert_eq!(decoded, acknowledgement);
        assert!(decoded.validate_against_request(&request).is_ok());
    }
}
