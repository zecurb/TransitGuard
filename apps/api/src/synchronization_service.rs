use thiserror::Error;
use transitguard_application::ReaderEquipmentRepository;
use transitguard_device_protocol::{
    DeviceProtocolVersion, ProtocolEnvironmentId, SynchronizationAcknowledgementEntry,
    SynchronizationBatchAcknowledgement, SynchronizationBatchAcknowledgementDefinition,
    SynchronizationBatchRequest, SynchronizationEntryOutcome, SynchronizationFailureCategory,
    SynchronizationProtocolError,
};
use transitguard_persistence::{
    PostgresReaderEquipmentRepository, PostgresSynchronizationIngestRepository,
    PreparedSynchronizationIngest, SynchronizationIngestDisposition,
    SynchronizationIngestPersistenceError,
};

/// Stable failures produced while processing a synchronization batch.
#[derive(Debug, Error)]
pub enum SynchronizationServiceError {
    /// The request uses a protocol version unsupported by this API.
    #[error("unsupported synchronization protocol")]
    UnsupportedProtocol,

    /// The reader and API environments differ.
    #[error("reader synchronization environment does not match the API")]
    EnvironmentMismatch,

    /// The submitted reader does not exist.
    #[error("reader is not registered")]
    ReaderNotRegistered,

    /// The reader exists but cannot authenticate to the backend.
    #[error("reader is not operational")]
    ReaderNotOperational,

    /// A durable batch identity was reused with different content.
    #[error("synchronization batch identity conflicts with stored content")]
    BatchIdentityConflict,

    /// A transaction identity conflicts with stored backend state.
    #[error("synchronization transaction identity conflicts with stored content")]
    TransactionIdentityConflict,

    /// A required backend dependency was unavailable.
    #[error("backend synchronization dependency is unavailable")]
    BackendTemporarilyUnavailable,

    /// A durable ingest record could not be prepared safely.
    #[error("synchronization ingest preparation failed")]
    IngestRecord,

    /// Validated protocol values could not be constructed.
    #[error("synchronization protocol validation failed")]
    Protocol(#[from] SynchronizationProtocolError),
}

impl SynchronizationServiceError {
    /// Returns the stable protocol failure category.
    #[must_use]
    pub const fn failure_category(&self) -> SynchronizationFailureCategory {
        match self {
            Self::UnsupportedProtocol => SynchronizationFailureCategory::UnsupportedProtocol,

            Self::EnvironmentMismatch => SynchronizationFailureCategory::EnvironmentMismatch,

            Self::ReaderNotRegistered => SynchronizationFailureCategory::ReaderNotRegistered,

            Self::ReaderNotOperational => SynchronizationFailureCategory::ReaderNotOperational,

            Self::BatchIdentityConflict => SynchronizationFailureCategory::BatchIdentityConflict,

            Self::TransactionIdentityConflict => {
                SynchronizationFailureCategory::TransactionIdentityConflict
            }

            Self::BackendTemporarilyUnavailable => {
                SynchronizationFailureCategory::BackendTemporarilyUnavailable
            }

            Self::IngestRecord | Self::Protocol(_) => {
                SynchronizationFailureCategory::BackendValidationFailure
            }
        }
    }
}

/// Coordinates reader validation and atomic synchronization ingest.
#[derive(Clone, Debug)]
pub struct SynchronizationService {
    reader_repository: PostgresReaderEquipmentRepository,
    ingest_repository: PostgresSynchronizationIngestRepository,
    environment_id: ProtocolEnvironmentId,
}

impl SynchronizationService {
    /// Creates a backend synchronization service.
    #[must_use]
    pub const fn new(
        reader_repository: PostgresReaderEquipmentRepository,
        ingest_repository: PostgresSynchronizationIngestRepository,
        environment_id: ProtocolEnvironmentId,
    ) -> Self {
        Self {
            reader_repository,
            ingest_repository,
            environment_id,
        }
    }

    /// Processes one validated synchronization batch.
    ///
    /// New requests are committed atomically. Identical retries
    /// return the original durable acknowledgement with the replay
    /// indicator enabled.
    pub async fn process(
        &self,
        request: &SynchronizationBatchRequest,
        received_at_unix_milliseconds: i64,
    ) -> Result<SynchronizationBatchAcknowledgement, SynchronizationServiceError> {
        validate_request_context(request, &self.environment_id)?;

        let reader = self
            .reader_repository
            .find_by_id(request.reader_id())
            .await
            .map_err(|_| SynchronizationServiceError::BackendTemporarilyUnavailable)?
            .ok_or(SynchronizationServiceError::ReaderNotRegistered)?;

        if !reader.aggregate().may_authenticate_to_backend() {
            return Err(SynchronizationServiceError::ReaderNotOperational);
        }

        let acknowledgement = build_acknowledgement(request, received_at_unix_milliseconds)?;

        let prepared = PreparedSynchronizationIngest::prepare(request, &acknowledgement)
            .map_err(|_| SynchronizationServiceError::IngestRecord)?;

        let disposition = self
            .ingest_repository
            .store(&prepared)
            .await
            .map_err(map_ingest_error)?;

        match disposition {
            SynchronizationIngestDisposition::Stored => Ok(acknowledgement),

            SynchronizationIngestDisposition::Replayed => {
                let stored = self
                    .ingest_repository
                    .load_acknowledgement(request.batch_id())
                    .await
                    .map_err(map_ingest_error)?;

                stored
                    .map(|value| value.with_replayed(true))
                    .ok_or(SynchronizationServiceError::BackendTemporarilyUnavailable)
            }
        }
    }
}

fn validate_request_context(
    request: &SynchronizationBatchRequest,
    environment_id: &ProtocolEnvironmentId,
) -> Result<(), SynchronizationServiceError> {
    if request.protocol_version() != DeviceProtocolVersion::CURRENT {
        return Err(SynchronizationServiceError::UnsupportedProtocol);
    }

    if request.environment_id() != environment_id {
        return Err(SynchronizationServiceError::EnvironmentMismatch);
    }

    Ok(())
}

fn build_acknowledgement(
    request: &SynchronizationBatchRequest,
    received_at_unix_milliseconds: i64,
) -> Result<SynchronizationBatchAcknowledgement, SynchronizationServiceError> {
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
        .collect::<Result<Vec<_>, _>>()?;

    SynchronizationBatchAcknowledgement::new(SynchronizationBatchAcknowledgementDefinition {
        protocol_version: request.protocol_version(),
        environment_id: request.environment_id().clone(),
        reader_id: request.reader_id(),
        batch_id: request.batch_id(),
        first_local_sequence_number: request.first_local_sequence_number(),
        last_local_sequence_number: request.last_local_sequence_number(),
        received_at_unix_milliseconds,
        replayed: false,
        entries,
    })
    .map_err(SynchronizationServiceError::from)
}

fn map_ingest_error(error: SynchronizationIngestPersistenceError) -> SynchronizationServiceError {
    match error {
        SynchronizationIngestPersistenceError::BatchIdentityConflict { .. } => {
            SynchronizationServiceError::BatchIdentityConflict
        }

        SynchronizationIngestPersistenceError::TransactionIdentityConflict { .. } => {
            SynchronizationServiceError::TransactionIdentityConflict
        }

        SynchronizationIngestPersistenceError::Persistence(_) => {
            SynchronizationServiceError::BackendTemporarilyUnavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{
        CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
        ReaderSoftwareVersion, SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
        SynchronizationEntryOutcome, SynchronizationFailureCategory, SynchronizationRequestEntry,
    };
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use super::{SynchronizationServiceError, build_acknowledgement, validate_request_context};

    fn sequence(value: u64) -> LocalSequenceNumber {
        match LocalSequenceNumber::new(value) {
            Ok(sequence) => sequence,

            Err(error) => {
                panic!("valid sequence failed: {error}")
            }
        }
    }

    fn environment(value: &str) -> ProtocolEnvironmentId {
        match ProtocolEnvironmentId::new(value) {
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

    fn envelope() -> CanonicalTransactionEnvelope {
        match CanonicalTransactionEnvelope::from_json(r#"{"schema_version":1}"#) {
            Ok(envelope) => envelope,

            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        }
    }

    fn protocol_version(value: u16) -> DeviceProtocolVersion {
        match DeviceProtocolVersion::new(value) {
            Ok(version) => version,

            Err(error) => {
                panic!("valid protocol version failed: {error}")
            }
        }
    }

    fn request(
        version: DeviceProtocolVersion,
        environment_id: ProtocolEnvironmentId,
    ) -> SynchronizationBatchRequest {
        let local_sequence_number = sequence(4);

        let entry = SynchronizationRequestEntry::new(
            FareTransactionId::generate(),
            local_sequence_number,
            envelope(),
        );

        match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: version,
            environment_id,
            reader_id: ReaderId::generate(),
            reader_software_version: software_version(),
            batch_id: SynchronizationBatchId::generate(),
            first_local_sequence_number: local_sequence_number,
            last_local_sequence_number: local_sequence_number,
            submitted_at_unix_milliseconds: 100,
            entries: vec![entry],
        }) {
            Ok(request) => request,

            Err(error) => {
                panic!("valid request failed: {error}")
            }
        }
    }

    #[test]
    fn acknowledgement_preserves_request_order() {
        let request = request(DeviceProtocolVersion::CURRENT, environment("development"));

        let acknowledgement = match build_acknowledgement(&request, 200) {
            Ok(acknowledgement) => acknowledgement,

            Err(error) => {
                panic!(
                    "acknowledgement creation failed: \
                         {error}"
                )
            }
        };

        assert_eq!(acknowledgement.reader_id(), request.reader_id());

        assert_eq!(acknowledgement.batch_id(), request.batch_id());

        assert_eq!(acknowledgement.received_at_unix_milliseconds(), 200);

        assert!(!acknowledgement.replayed());

        assert_eq!(acknowledgement.entries().len(), 1);

        assert_eq!(
            acknowledgement.entries()[0].transaction_id(),
            request.entries()[0].transaction_id()
        );

        assert_eq!(
            acknowledgement.entries()[0].outcome(),
            SynchronizationEntryOutcome::Acknowledged
        );
    }

    #[test]
    fn unsupported_protocol_is_rejected() {
        let request = request(protocol_version(2), environment("development"));

        let result = validate_request_context(&request, &environment("development"));

        assert!(matches!(
            result,
            Err(SynchronizationServiceError::UnsupportedProtocol)
        ));
    }

    #[test]
    fn environment_mismatch_is_rejected() {
        let request = request(
            DeviceProtocolVersion::CURRENT,
            environment("reader-development"),
        );

        let result = validate_request_context(&request, &environment("backend-development"));

        assert!(matches!(
            result,
            Err(SynchronizationServiceError::EnvironmentMismatch)
        ));
    }

    #[test]
    fn service_errors_have_stable_categories() {
        assert_eq!(
            SynchronizationServiceError::ReaderNotRegistered.failure_category(),
            SynchronizationFailureCategory::ReaderNotRegistered
        );

        assert_eq!(
            SynchronizationServiceError::BatchIdentityConflict.failure_category(),
            SynchronizationFailureCategory::BatchIdentityConflict
        );

        assert_eq!(
            SynchronizationServiceError::BackendTemporarilyUnavailable.failure_category(),
            SynchronizationFailureCategory::BackendTemporarilyUnavailable
        );
    }
}
