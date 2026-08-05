//! TransitGuard fictional fare-reader simulator.
//!
//! The simulator uses only project-owned identities, credential formats, and
//! protocol messages.

pub mod demo;
pub mod fare_processing;
pub mod scenario;
pub mod synchronization_submission;
pub mod synchronization_transport;

pub use demo::{DemoScenarioError, run_demo_scenario};

pub use synchronization_transport::{SynchronizationHttpClient, SynchronizationHttpClientError};

pub use synchronization_submission::{
    SynchronizationFailureDisposition, SynchronizationSubmissionError,
    SynchronizationSubmissionResult, SynchronizationTransport, SynchronizationTransportFailure,
    submit_in_flight_synchronization_batch, synchronization_failure_disposition,
};

pub use fare_processing::{
    ReaderFareContext, ReaderFareEvaluation, ReaderFareProcessingError, ReaderTapDecision,
};

pub use scenario::{
    ScenarioAction, ScenarioActionKind, ScenarioFailureCategory, ScenarioReport,
    ScenarioStepRecord, ScenarioStepResult, run_scenario,
};

use serde::Serialize;
use thiserror::Error;
use transitguard_device_protocol::{
    CredentialMedium, CredentialPresentation, CredentialPresentationDefinition,
    DeviceProtocolVersion, ProtocolZoneId,
};
use transitguard_domain::{
    EventTime, FareCredentialId, FareProcessingMode, FareTransactionId, LocalSequenceNumber,
    ReaderId,
};

/// Simulated connectivity available to a reader.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderConnectivity {
    /// The fictional backend is reachable.
    Connected,

    /// The fictional backend is unavailable.
    Disconnected,
}

/// Explicit lifecycle state of a reader simulator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ReaderOperatingState {
    /// Reader configuration exists but startup is incomplete.
    Booting,

    /// Reader is ready and backend connectivity is available.
    ReadyOnline,

    /// Reader is ready for bounded local processing.
    ReadyOffline,

    /// Reader has stopped accepting presentations.
    Stopped,
}

/// Errors produced by reader lifecycle and presentation processing.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReaderSimulatorError {
    /// Software versions must use a small safe textual representation.
    #[error("reader software version must contain 1 to 64 ASCII version characters")]
    InvalidSoftwareVersion,

    /// The requested operation is not valid in the current lifecycle state.
    #[error("reader operation `{operation}` is invalid while state is {state:?}")]
    InvalidStateTransition {
        /// Current reader state.
        state: ReaderOperatingState,

        /// Attempted operation.
        operation: &'static str,
    },

    /// The reader is not currently able to accept presentations.
    #[error("reader cannot accept a presentation while state is {state:?}")]
    NotReady {
        /// Current reader state.
        state: ReaderOperatingState,
    },

    /// No additional reader-local sequence can be assigned.
    #[error("reader-local sequence space is exhausted")]
    SequenceExhausted,
}

/// Typed input for one fictional credential presentation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ReaderPresentationInput {
    credential_id: FareCredentialId,
    credential_medium: CredentialMedium,
    event_time: EventTime,
    origin_zone: ProtocolZoneId,
    destination_zone: ProtocolZoneId,
}

impl ReaderPresentationInput {
    /// Creates presentation input from validated values.
    #[must_use]
    pub const fn new(
        credential_id: FareCredentialId,
        credential_medium: CredentialMedium,
        event_time: EventTime,
        origin_zone: ProtocolZoneId,
        destination_zone: ProtocolZoneId,
    ) -> Self {
        Self {
            credential_id,
            credential_medium,
            event_time,
            origin_zone,
            destination_zone,
        }
    }

    /// Returns the credential identity.
    #[must_use]
    pub const fn credential_id(self) -> FareCredentialId {
        self.credential_id
    }

    /// Returns the credential medium.
    #[must_use]
    pub const fn credential_medium(self) -> CredentialMedium {
        self.credential_medium
    }

    /// Returns the presentation event time.
    #[must_use]
    pub const fn event_time(self) -> EventTime {
        self.event_time
    }

    /// Returns the origin zone.
    #[must_use]
    pub const fn origin_zone(self) -> ProtocolZoneId {
        self.origin_zone
    }

    /// Returns the destination zone.
    #[must_use]
    pub const fn destination_zone(self) -> ProtocolZoneId {
        self.destination_zone
    }
}

/// Observable reader status safe for simulator diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReaderHealthSnapshot {
    /// Registered reader identity.
    pub reader_id: ReaderId,

    /// Current reader lifecycle state.
    pub state: ReaderOperatingState,

    /// Installed project-owned protocol version.
    pub protocol_version: DeviceProtocolVersion,

    /// Reader simulator software version.
    pub software_version: String,

    /// Next reader-local sequence value.
    pub next_local_sequence: u64,
}

/// In-memory Phase 5 reader simulator.
///
/// Durable state and restart recovery are added during Phase 6.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReaderSimulator {
    reader_id: ReaderId,
    protocol_version: DeviceProtocolVersion,
    software_version: String,
    state: ReaderOperatingState,
    next_local_sequence: u64,
}

impl ReaderSimulator {
    /// Creates a reader in the booting state.
    pub fn new(
        reader_id: ReaderId,
        protocol_version: DeviceProtocolVersion,
        software_version: impl Into<String>,
    ) -> Result<Self, ReaderSimulatorError> {
        let software_version = software_version.into();

        validate_software_version(&software_version)?;

        Ok(Self {
            reader_id,
            protocol_version,
            software_version,
            state: ReaderOperatingState::Booting,
            next_local_sequence: 1,
        })
    }

    /// Starts the reader with explicit connectivity.
    pub fn start(&mut self, connectivity: ReaderConnectivity) -> Result<(), ReaderSimulatorError> {
        if self.state != ReaderOperatingState::Booting {
            return Err(ReaderSimulatorError::InvalidStateTransition {
                state: self.state,
                operation: "start",
            });
        }

        self.state = state_for_connectivity(connectivity);

        Ok(())
    }

    /// Updates reader connectivity after startup.
    pub fn set_connectivity(
        &mut self,
        connectivity: ReaderConnectivity,
    ) -> Result<(), ReaderSimulatorError> {
        match self.state {
            ReaderOperatingState::ReadyOnline | ReaderOperatingState::ReadyOffline => {
                self.state = state_for_connectivity(connectivity);

                Ok(())
            }

            ReaderOperatingState::Booting | ReaderOperatingState::Stopped => {
                Err(ReaderSimulatorError::InvalidStateTransition {
                    state: self.state,
                    operation: "set_connectivity",
                })
            }
        }
    }

    /// Stops the reader and prevents further presentations.
    pub fn stop(&mut self) -> Result<(), ReaderSimulatorError> {
        if self.state == ReaderOperatingState::Stopped {
            return Err(ReaderSimulatorError::InvalidStateTransition {
                state: self.state,
                operation: "stop",
            });
        }

        self.state = ReaderOperatingState::Stopped;

        Ok(())
    }

    /// Accepts one typed simulated credential presentation.
    pub fn present_credential(
        &mut self,
        input: ReaderPresentationInput,
    ) -> Result<CredentialPresentation, ReaderSimulatorError> {
        let processing_mode = match self.state {
            ReaderOperatingState::ReadyOnline => FareProcessingMode::Online,

            ReaderOperatingState::ReadyOffline => FareProcessingMode::Offline,

            ReaderOperatingState::Booting | ReaderOperatingState::Stopped => {
                return Err(ReaderSimulatorError::NotReady { state: self.state });
            }
        };

        let following_sequence = self
            .next_local_sequence
            .checked_add(1)
            .ok_or(ReaderSimulatorError::SequenceExhausted)?;

        let local_sequence_number = LocalSequenceNumber::new(self.next_local_sequence)
            .map_err(|_| ReaderSimulatorError::SequenceExhausted)?;

        let presentation =
            CredentialPresentation::from_definition(CredentialPresentationDefinition {
                protocol_version: self.protocol_version,
                transaction_id: FareTransactionId::generate(),
                reader_id: self.reader_id,
                local_sequence_number,
                credential_id: input.credential_id(),
                credential_medium: input.credential_medium(),
                event_time: input.event_time(),
                origin_zone: input.origin_zone(),
                destination_zone: input.destination_zone(),
                processing_mode,
            });

        self.next_local_sequence = following_sequence;

        Ok(presentation)
    }

    /// Returns the reader identity.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> ReaderOperatingState {
        self.state
    }

    /// Returns a diagnostic health snapshot.
    #[must_use]
    pub fn health_snapshot(&self) -> ReaderHealthSnapshot {
        ReaderHealthSnapshot {
            reader_id: self.reader_id,
            state: self.state,
            protocol_version: self.protocol_version,
            software_version: self.software_version.clone(),
            next_local_sequence: self.next_local_sequence,
        }
    }
}

fn state_for_connectivity(connectivity: ReaderConnectivity) -> ReaderOperatingState {
    match connectivity {
        ReaderConnectivity::Connected => ReaderOperatingState::ReadyOnline,

        ReaderConnectivity::Disconnected => ReaderOperatingState::ReadyOffline,
    }
}

fn validate_software_version(value: &str) -> Result<(), ReaderSimulatorError> {
    let valid_length = !value.is_empty() && value.len() <= 64;

    let valid_characters = value.bytes().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, b'.' | b'-' | b'+' | b'_')
    });

    if !valid_length || !valid_characters {
        return Err(ReaderSimulatorError::InvalidSoftwareVersion);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use transitguard_device_protocol::{CredentialMedium, DeviceProtocolVersion, ProtocolZoneId};
    use transitguard_domain::{EventTime, FareCredentialId, FareProcessingMode, ReaderId};

    use super::{
        ReaderConnectivity, ReaderOperatingState, ReaderPresentationInput, ReaderSimulator,
        ReaderSimulatorError,
    };

    fn event_time() -> EventTime {
        let Ok(value) = EventTime::from_unix_milliseconds(1_700_000_000_000) else {
            panic!("test event time must be valid");
        };

        value
    }

    fn zone(value: u16) -> ProtocolZoneId {
        let Ok(zone) = ProtocolZoneId::new(value) else {
            panic!("positive protocol zone must be valid");
        };

        zone
    }

    fn input() -> ReaderPresentationInput {
        ReaderPresentationInput::new(
            FareCredentialId::generate(),
            CredentialMedium::Card,
            event_time(),
            zone(1),
            zone(3),
        )
    }

    fn reader() -> ReaderSimulator {
        let result = ReaderSimulator::new(
            ReaderId::generate(),
            DeviceProtocolVersion::CURRENT,
            "0.1.0",
        );

        let Ok(reader) = result else {
            panic!("test reader must be valid");
        };

        reader
    }

    #[test]
    fn reader_starts_online() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected), Ok(()));

        assert_eq!(reader.state(), ReaderOperatingState::ReadyOnline);
    }

    #[test]
    fn online_presentations_receive_monotonic_sequences() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected), Ok(()));

        let first = reader.present_credential(input());
        let second = reader.present_credential(input());

        let Ok(first) = first else {
            panic!("first presentation must succeed");
        };

        let Ok(second) = second else {
            panic!("second presentation must succeed");
        };

        assert_eq!(first.local_sequence_number().value(), 1);

        assert_eq!(second.local_sequence_number().value(), 2);

        assert_ne!(first.transaction_id(), second.transaction_id());

        assert_eq!(first.processing_mode(), FareProcessingMode::Online);
    }

    #[test]
    fn disconnected_reader_uses_offline_mode() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Disconnected,), Ok(()));

        let presentation = reader.present_credential(input());

        let Ok(presentation) = presentation else {
            panic!("offline presentation must succeed");
        };

        assert_eq!(presentation.processing_mode(), FareProcessingMode::Offline);

        assert_eq!(reader.state(), ReaderOperatingState::ReadyOffline);
    }

    #[test]
    fn connectivity_transition_changes_processing_mode() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected), Ok(()));

        assert_eq!(
            reader.set_connectivity(ReaderConnectivity::Disconnected,),
            Ok(())
        );

        let presentation = reader.present_credential(input());

        let Ok(presentation) = presentation else {
            panic!("offline presentation must succeed");
        };

        assert_eq!(presentation.processing_mode(), FareProcessingMode::Offline);
    }

    #[test]
    fn presentation_before_startup_is_rejected() {
        let mut reader = reader();

        assert_eq!(
            reader.present_credential(input()),
            Err(ReaderSimulatorError::NotReady {
                state: ReaderOperatingState::Booting,
            })
        );
    }

    #[test]
    fn stopped_reader_rejects_presentations() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected), Ok(()));

        assert_eq!(reader.stop(), Ok(()));

        assert_eq!(
            reader.present_credential(input()),
            Err(ReaderSimulatorError::NotReady {
                state: ReaderOperatingState::Stopped,
            })
        );
    }

    #[test]
    fn invalid_software_version_is_rejected() {
        assert_eq!(
            ReaderSimulator::new(
                ReaderId::generate(),
                DeviceProtocolVersion::CURRENT,
                "version with spaces",
            ),
            Err(ReaderSimulatorError::InvalidSoftwareVersion)
        );
    }

    #[test]
    fn health_snapshot_reports_next_sequence() {
        let mut reader = reader();

        assert_eq!(reader.start(ReaderConnectivity::Connected), Ok(()));

        let result = reader.present_credential(input());

        assert!(result.is_ok());

        let health = reader.health_snapshot();

        assert_eq!(health.state, ReaderOperatingState::ReadyOnline);

        assert_eq!(health.next_local_sequence, 2);
        assert_eq!(health.protocol_version, DeviceProtocolVersion::CURRENT);
    }
}
