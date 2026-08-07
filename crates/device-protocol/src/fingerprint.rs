use core::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::SynchronizationBatchRequest;

/// Number of bytes in a SHA-256 synchronization fingerprint.
pub const SYNCHRONIZATION_FINGERPRINT_BYTES: usize = 32;

/// Number of hexadecimal characters in a synchronization fingerprint.
pub const SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH: usize = SYNCHRONIZATION_FINGERPRINT_BYTES * 2;

const FINGERPRINT_DOMAIN: &[u8] = b"transitguard.reader-synchronization.request.v1";

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Errors produced while parsing synchronization fingerprints.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SynchronizationFingerprintParseError {
    /// The hexadecimal representation had an invalid length.
    #[error(
        "synchronization fingerprint must contain \
         {SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH} hexadecimal \
         characters; received {actual_length}"
    )]
    InvalidLength {
        /// Actual string length.
        actual_length: usize,
    },

    /// The representation contained a non-hexadecimal character.
    #[error(
        "synchronization fingerprint contains invalid hexadecimal \
         data at position {position}"
    )]
    InvalidHexadecimalCharacter {
        /// Zero-based character position.
        position: usize,
    },
}

/// Deterministic SHA-256 identity for a synchronization request.
///
/// The fingerprint is calculated from validated protocol values rather than
/// arbitrary HTTP header order or JSON whitespace.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SynchronizationRequestFingerprint([u8; SYNCHRONIZATION_FINGERPRINT_BYTES]);

impl SynchronizationRequestFingerprint {
    /// Creates a fingerprint from its exact SHA-256 bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SYNCHRONIZATION_FINGERPRINT_BYTES]) -> Self {
        Self(bytes)
    }

    /// Calculates the deterministic fingerprint for a request.
    #[must_use]
    pub fn calculate(request: &SynchronizationBatchRequest) -> Self {
        let mut hasher = Sha256::new();

        update_bytes(&mut hasher, FINGERPRINT_DOMAIN);
        update_u16(&mut hasher, request.protocol_version().value());
        update_text(&mut hasher, request.environment_id().as_str());
        update_text(&mut hasher, &request.reader_id().to_string());
        update_text(&mut hasher, request.reader_software_version().as_str());
        update_text(&mut hasher, &request.batch_id().to_string());
        update_u64(&mut hasher, request.first_local_sequence_number().value());
        update_u64(&mut hasher, request.last_local_sequence_number().value());
        update_i64(&mut hasher, request.submitted_at_unix_milliseconds());
        update_u64(&mut hasher, request.entries().len() as u64);

        for entry in request.entries() {
            update_text(&mut hasher, &entry.transaction_id().to_string());
            update_u64(&mut hasher, entry.local_sequence_number().value());
            update_text(&mut hasher, entry.transaction_envelope().as_str());
        }

        let digest = hasher.finalize();
        let mut bytes = [0_u8; SYNCHRONIZATION_FINGERPRINT_BYTES];

        bytes.copy_from_slice(&digest);

        Self(bytes)
    }

    /// Returns the exact SHA-256 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; SYNCHRONIZATION_FINGERPRINT_BYTES] {
        &self.0
    }

    /// Returns the lowercase hexadecimal representation.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH);

        for byte in self.0 {
            output.push(HEX_DIGITS[usize::from(byte >> 4)] as char);
            output.push(HEX_DIGITS[usize::from(byte & 0x0f)] as char);
        }

        output
    }
}

impl SynchronizationBatchRequest {
    /// Calculates the deterministic replay fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> SynchronizationRequestFingerprint {
        SynchronizationRequestFingerprint::calculate(self)
    }
}

impl fmt::Debug for SynchronizationRequestFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SynchronizationRequestFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for SynchronizationRequestFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for SynchronizationRequestFingerprint {
    type Err = SynchronizationFingerprintParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH {
            return Err(SynchronizationFingerprintParseError::InvalidLength {
                actual_length: value.len(),
            });
        }

        let encoded = value.as_bytes();
        let mut decoded = [0_u8; SYNCHRONIZATION_FINGERPRINT_BYTES];

        for (index, output) in decoded.iter_mut().enumerate() {
            let high_position = index * 2;
            let low_position = high_position + 1;

            let high = decode_hexadecimal_digit(encoded[high_position], high_position)?;

            let low = decode_hexadecimal_digit(encoded[low_position], low_position)?;

            *output = (high << 4) | low;
        }

        Ok(Self(decoded))
    }
}

impl Serialize for SynchronizationRequestFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for SynchronizationRequestFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        value.parse().map_err(serde::de::Error::custom)
    }
}

fn update_bytes(hasher: &mut Sha256, value: &[u8]) {
    update_u64(hasher, value.len() as u64);
    hasher.update(value);
}

fn update_text(hasher: &mut Sha256, value: &str) {
    update_bytes(hasher, value.as_bytes());
}

fn update_u16(hasher: &mut Sha256, value: u16) {
    hasher.update(value.to_be_bytes());
}

fn update_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_be_bytes());
}

fn update_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_be_bytes());
}

fn decode_hexadecimal_digit(
    value: u8,
    position: usize,
) -> Result<u8, SynchronizationFingerprintParseError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(SynchronizationFingerprintParseError::InvalidHexadecimalCharacter { position }),
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use crate::{
        CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
        ReaderSoftwareVersion, SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
        SynchronizationRequestEntry,
    };

    use super::{
        SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH, SynchronizationFingerprintParseError,
        SynchronizationRequestFingerprint,
    };

    const TEST_TIME: i64 = 1_700_000_000_000;

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

    fn envelope(json: &str) -> CanonicalTransactionEnvelope {
        match CanonicalTransactionEnvelope::from_json(json) {
            Ok(envelope) => envelope,
            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        }
    }

    fn request(
        reader_id: ReaderId,
        batch_id: SynchronizationBatchId,
        transaction_id: FareTransactionId,
        transaction_envelope: CanonicalTransactionEnvelope,
        submitted_at_unix_milliseconds: i64,
    ) -> SynchronizationBatchRequest {
        match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            environment_id: environment(),
            reader_id,
            reader_software_version: software_version(),
            batch_id,
            first_local_sequence_number: sequence(1),
            last_local_sequence_number: sequence(1),
            submitted_at_unix_milliseconds,
            entries: vec![SynchronizationRequestEntry::new(
                transaction_id,
                sequence(1),
                transaction_envelope,
            )],
        }) {
            Ok(request) => request,
            Err(error) => {
                panic!("valid request failed: {error}")
            }
        }
    }

    #[test]
    fn identical_requests_have_identical_fingerprints() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let first = request(
            reader_id,
            batch_id,
            transaction_id,
            envelope(r#"{"schema_version":1,"value":42}"#),
            TEST_TIME,
        );

        let second = first.clone();

        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn equivalent_json_envelopes_have_identical_fingerprints() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let first = request(
            reader_id,
            batch_id,
            transaction_id,
            envelope(r#"{"z":2,"a":1}"#),
            TEST_TIME,
        );

        let second = request(
            reader_id,
            batch_id,
            transaction_id,
            envelope(r#"{ "a": 1, "z": 2 }"#),
            TEST_TIME,
        );

        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn changed_envelope_changes_fingerprint() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let first = request(
            reader_id,
            batch_id,
            transaction_id,
            envelope(r#"{"value":1}"#),
            TEST_TIME,
        );

        let second = request(
            reader_id,
            batch_id,
            transaction_id,
            envelope(r#"{"value":2}"#),
            TEST_TIME,
        );

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn changed_submission_time_changes_fingerprint() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();
        let transaction_envelope = envelope(r#"{"value":1}"#);

        let first = request(
            reader_id,
            batch_id,
            transaction_id,
            transaction_envelope.clone(),
            TEST_TIME,
        );

        let second = request(
            reader_id,
            batch_id,
            transaction_id,
            transaction_envelope,
            TEST_TIME + 1,
        );

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn fingerprint_round_trips_through_text() {
        let request = request(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            FareTransactionId::generate(),
            envelope(r#"{"value":1}"#),
            TEST_TIME,
        );

        let fingerprint = request.fingerprint();
        let encoded = fingerprint.to_string();

        let decoded = match encoded.parse::<SynchronizationRequestFingerprint>() {
            Ok(decoded) => decoded,
            Err(error) => {
                panic!("valid fingerprint failed: {error}")
            }
        };

        assert_eq!(encoded.len(), 64);
        assert_eq!(decoded, fingerprint);
    }

    #[test]
    fn fingerprint_round_trips_through_json() {
        let request = request(
            ReaderId::generate(),
            SynchronizationBatchId::generate(),
            FareTransactionId::generate(),
            envelope(r#"{"value":1}"#),
            TEST_TIME,
        );

        let fingerprint = request.fingerprint();

        let encoded = match serde_json::to_string(&fingerprint) {
            Ok(encoded) => encoded,
            Err(error) => {
                panic!("fingerprint serialization failed: {error}")
            }
        };

        let decoded = match serde_json::from_str::<SynchronizationRequestFingerprint>(&encoded) {
            Ok(decoded) => decoded,
            Err(error) => {
                panic!("fingerprint deserialization failed: {error}")
            }
        };

        assert_eq!(decoded, fingerprint);
    }

    #[test]
    fn invalid_fingerprint_length_is_rejected() {
        let result = "abcd".parse::<SynchronizationRequestFingerprint>();

        assert_eq!(
            result,
            Err(SynchronizationFingerprintParseError::InvalidLength { actual_length: 4 })
        );
    }

    #[test]
    fn invalid_hexadecimal_character_is_rejected() {
        let mut encoded = "0".repeat(SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH);

        encoded.replace_range(10..11, "x");

        let result = encoded.parse::<SynchronizationRequestFingerprint>();

        assert_eq!(
            result,
            Err(SynchronizationFingerprintParseError::InvalidHexadecimalCharacter { position: 10 })
        );
    }
}
