use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use transitguard_domain::{
    EventTime, FareCredentialId, FareProcessingMode, FareTransactionId, LocalSequenceNumber,
    ReaderId,
};

use crate::DeviceProtocolVersion;

/// Errors produced while constructing protocol presentation values.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PresentationValueError {
    /// Protocol zone identifiers begin at one.
    #[error("protocol zone identifier must be greater than zero")]
    ZeroZoneId,
}

/// A validated zone identifier carried by the device protocol.
///
/// This type belongs to the fictional TransitGuard protocol. Translation into
/// fare-engine zone types occurs at the application boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProtocolZoneId(u16);

impl ProtocolZoneId {
    /// Creates a validated protocol zone identifier.
    pub const fn new(value: u16) -> Result<Self, PresentationValueError> {
        if value == 0 {
            return Err(PresentationValueError::ZeroZoneId);
        }

        Ok(Self(value))
    }

    /// Returns the numeric zone identifier.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProtocolZoneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Fictional credential medium presented to a TransitGuard reader.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum CredentialMedium {
    /// Project-owned simulated transit card.
    Card,

    /// Project-owned simulated mobile credential.
    Mobile,
}

/// Complete data used to construct a credential presentation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CredentialPresentationDefinition {
    /// Device-protocol version used to encode the message.
    pub protocol_version: DeviceProtocolVersion,

    /// Stable fare-transaction identity.
    pub transaction_id: FareTransactionId,

    /// Registered reader identity.
    pub reader_id: ReaderId,

    /// Monotonic sequence scoped to the reader.
    pub local_sequence_number: LocalSequenceNumber,

    /// Fictional credential identity.
    pub credential_id: FareCredentialId,

    /// Simulated credential medium.
    pub credential_medium: CredentialMedium,

    /// Explicit event time for the presentation.
    pub event_time: EventTime,

    /// Journey origin supplied by the simulator.
    pub origin_zone: ProtocolZoneId,

    /// Journey destination supplied by the simulator.
    pub destination_zone: ProtocolZoneId,

    /// Connectivity mode used by the reader.
    pub processing_mode: FareProcessingMode,
}

/// Versioned project-owned credential-presentation message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CredentialPresentation {
    protocol_version: DeviceProtocolVersion,
    transaction_id: FareTransactionId,
    reader_id: ReaderId,
    local_sequence_number: LocalSequenceNumber,
    credential_id: FareCredentialId,
    credential_medium: CredentialMedium,
    event_time: EventTime,
    origin_zone: ProtocolZoneId,
    destination_zone: ProtocolZoneId,
    processing_mode: FareProcessingMode,
}

impl CredentialPresentation {
    /// Creates a credential presentation from typed values.
    #[must_use]
    pub const fn from_definition(definition: CredentialPresentationDefinition) -> Self {
        Self {
            protocol_version: definition.protocol_version,
            transaction_id: definition.transaction_id,
            reader_id: definition.reader_id,
            local_sequence_number: definition.local_sequence_number,
            credential_id: definition.credential_id,
            credential_medium: definition.credential_medium,
            event_time: definition.event_time,
            origin_zone: definition.origin_zone,
            destination_zone: definition.destination_zone,
            processing_mode: definition.processing_mode,
        }
    }

    /// Returns the protocol version.
    #[must_use]
    pub const fn protocol_version(self) -> DeviceProtocolVersion {
        self.protocol_version
    }

    /// Returns the stable transaction identity.
    #[must_use]
    pub const fn transaction_id(self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the reader identity.
    #[must_use]
    pub const fn reader_id(self) -> ReaderId {
        self.reader_id
    }

    /// Returns the reader-local sequence.
    #[must_use]
    pub const fn local_sequence_number(self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the credential identity.
    #[must_use]
    pub const fn credential_id(self) -> FareCredentialId {
        self.credential_id
    }

    /// Returns the simulated credential medium.
    #[must_use]
    pub const fn credential_medium(self) -> CredentialMedium {
        self.credential_medium
    }

    /// Returns the explicit presentation time.
    #[must_use]
    pub const fn event_time(self) -> EventTime {
        self.event_time
    }

    /// Returns the journey origin.
    #[must_use]
    pub const fn origin_zone(self) -> ProtocolZoneId {
        self.origin_zone
    }

    /// Returns the journey destination.
    #[must_use]
    pub const fn destination_zone(self) -> ProtocolZoneId {
        self.destination_zone
    }

    /// Returns the reader processing mode.
    #[must_use]
    pub const fn processing_mode(self) -> FareProcessingMode {
        self.processing_mode
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        EventTime, FareCredentialId, FareProcessingMode, FareTransactionId, LocalSequenceNumber,
        ReaderId,
    };

    use crate::DeviceProtocolVersion;

    use super::{
        CredentialMedium, CredentialPresentation, CredentialPresentationDefinition,
        PresentationValueError, ProtocolZoneId,
    };

    fn event_time() -> EventTime {
        let Ok(value) = EventTime::from_unix_milliseconds(1_700_000_000_000) else {
            panic!("test event time must be valid");
        };

        value
    }

    fn sequence() -> LocalSequenceNumber {
        let Ok(value) = LocalSequenceNumber::new(1) else {
            panic!("sequence one must be valid");
        };

        value
    }

    fn zone(value: u16) -> ProtocolZoneId {
        let Ok(zone) = ProtocolZoneId::new(value) else {
            panic!("positive protocol zone must be valid");
        };

        zone
    }

    fn presentation() -> CredentialPresentation {
        CredentialPresentation::from_definition(CredentialPresentationDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            transaction_id: FareTransactionId::generate(),
            reader_id: ReaderId::generate(),
            local_sequence_number: sequence(),
            credential_id: FareCredentialId::generate(),
            credential_medium: CredentialMedium::Mobile,
            event_time: event_time(),
            origin_zone: zone(1),
            destination_zone: zone(3),
            processing_mode: FareProcessingMode::Online,
        })
    }

    #[test]
    fn zero_zone_is_rejected() {
        assert_eq!(
            ProtocolZoneId::new(0),
            Err(PresentationValueError::ZeroZoneId)
        );
    }

    #[test]
    fn presentation_round_trips_through_json() {
        let original = presentation();

        let Ok(json) = serde_json::to_string(&original) else {
            panic!("presentation must serialize");
        };

        let Ok(decoded) = serde_json::from_str::<CredentialPresentation>(&json) else {
            panic!("presentation must deserialize");
        };

        assert_eq!(decoded, original);
    }

    #[test]
    fn presentation_preserves_protocol_fields() {
        let presentation = presentation();

        assert_eq!(
            presentation.protocol_version(),
            DeviceProtocolVersion::CURRENT
        );

        assert_eq!(presentation.local_sequence_number().value(), 1);

        assert_eq!(presentation.credential_medium(), CredentialMedium::Mobile);

        assert_eq!(presentation.processing_mode(), FareProcessingMode::Online);
    }
}
