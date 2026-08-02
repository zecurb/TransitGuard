//! Project-owned protocol types used by TransitGuard reader simulators.
//!
//! These types do not implement or claim compatibility with any real
//! transit-card, mobile-wallet, or fare-reader protocol.

pub mod presentation;
pub mod synchronization;
pub mod version;

pub use presentation::{
    CredentialMedium, CredentialPresentation, CredentialPresentationDefinition,
    PresentationValueError, ProtocolZoneId,
};

pub use synchronization::{
    CanonicalTransactionEnvelope, IDEMPOTENCY_KEY_HEADER,
    MAX_SYNCHRONIZATION_ACKNOWLEDGEMENT_BYTES, MAX_SYNCHRONIZATION_BATCH_ENTRIES,
    MAX_SYNCHRONIZATION_REQUEST_BYTES, MAX_TRANSACTION_ENVELOPE_BYTES, PROTOCOL_VERSION_HEADER,
    ProtocolEnvironmentId, ReaderSoftwareVersion, SYNCHRONIZATION_BATCH_ENDPOINT,
    SynchronizationAcknowledgementEntry, SynchronizationBatchAcknowledgement,
    SynchronizationBatchAcknowledgementDefinition, SynchronizationBatchRequest,
    SynchronizationBatchRequestDefinition, SynchronizationEntryOutcome,
    SynchronizationFailureCategory, SynchronizationProtocolError, SynchronizationRequestEntry,
};

pub use version::{DeviceProtocolVersion, DeviceProtocolVersionError};
