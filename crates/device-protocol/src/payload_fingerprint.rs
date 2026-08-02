use core::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    CanonicalTransactionEnvelope, SYNCHRONIZATION_FINGERPRINT_BYTES,
    SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH, SynchronizationBatchAcknowledgement,
    SynchronizationEntryOutcome,
};

const TRANSACTION_ENVELOPE_DOMAIN: &[u8] =
    b"transitguard.reader-synchronization.transaction-envelope.v1";

const ACKNOWLEDGEMENT_DOMAIN: &[u8] = b"transitguard.reader-synchronization.acknowledgement.v1";

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Errors produced while parsing synchronization payload fingerprints.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SynchronizationPayloadFingerprintParseError {
    /// The hexadecimal representation had an invalid length.
    #[error(
        "synchronization payload fingerprint must contain \
         {SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH} hexadecimal \
         characters; received {actual_length}"
    )]
    InvalidLength {
        /// Actual string length.
        actual_length: usize,
    },

    /// The representation contained a non-hexadecimal character.
    #[error(
        "synchronization payload fingerprint contains invalid hexadecimal \
         data at position {position}"
    )]
    InvalidHexadecimalCharacter {
        /// Zero-based character position.
        position: usize,
    },
}

/// Deterministic SHA-256 fingerprint for canonical synchronization payloads.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SynchronizationPayloadFingerprint([u8; SYNCHRONIZATION_FINGERPRINT_BYTES]);

impl SynchronizationPayloadFingerprint {
    /// Creates a fingerprint from exact SHA-256 bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; SYNCHRONIZATION_FINGERPRINT_BYTES]) -> Self {
        Self(bytes)
    }

    /// Fingerprints one canonical transaction envelope.
    #[must_use]
    pub fn for_transaction_envelope(envelope: &CanonicalTransactionEnvelope) -> Self {
        let mut hasher = Sha256::new();

        update_bytes(&mut hasher, TRANSACTION_ENVELOPE_DOMAIN);

        update_text(&mut hasher, envelope.as_str());

        finish(hasher)
    }

    /// Fingerprints the durable content of one acknowledgement.
    ///
    /// The transport-only `replayed` marker is intentionally excluded.
    /// An identical replay therefore retains the same durable fingerprint.
    #[must_use]
    pub fn for_acknowledgement(acknowledgement: &SynchronizationBatchAcknowledgement) -> Self {
        let mut hasher = Sha256::new();

        update_bytes(&mut hasher, ACKNOWLEDGEMENT_DOMAIN);

        update_u16(&mut hasher, acknowledgement.protocol_version().value());

        update_text(&mut hasher, acknowledgement.environment_id().as_str());

        update_text(&mut hasher, &acknowledgement.reader_id().to_string());

        update_text(&mut hasher, &acknowledgement.batch_id().to_string());

        update_u64(
            &mut hasher,
            acknowledgement.first_local_sequence_number().value(),
        );

        update_u64(
            &mut hasher,
            acknowledgement.last_local_sequence_number().value(),
        );

        update_i64(&mut hasher, acknowledgement.received_at_unix_milliseconds());

        update_u64(&mut hasher, acknowledgement.entries().len() as u64);

        for entry in acknowledgement.entries() {
            update_text(&mut hasher, &entry.transaction_id().to_string());

            update_u64(&mut hasher, entry.local_sequence_number().value());

            hasher.update([encode_outcome(entry.outcome())]);

            match entry.failure_category() {
                Some(category) => {
                    hasher.update([1]);
                    update_text(&mut hasher, category.as_str());
                }

                None => {
                    hasher.update([0]);
                }
            }

            match entry.next_retry_at_unix_milliseconds() {
                Some(retry_at) => {
                    hasher.update([1]);
                    update_i64(&mut hasher, retry_at);
                }

                None => {
                    hasher.update([0]);
                }
            }
        }

        finish(hasher)
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
            output.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));

            output.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }

        output
    }
}

impl fmt::Debug for SynchronizationPayloadFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SynchronizationPayloadFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for SynchronizationPayloadFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for SynchronizationPayloadFingerprint {
    type Err = SynchronizationPayloadFingerprintParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != SYNCHRONIZATION_FINGERPRINT_HEX_LENGTH {
            return Err(SynchronizationPayloadFingerprintParseError::InvalidLength {
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

impl Serialize for SynchronizationPayloadFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for SynchronizationPayloadFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        value.parse().map_err(serde::de::Error::custom)
    }
}

const fn encode_outcome(outcome: SynchronizationEntryOutcome) -> u8 {
    match outcome {
        SynchronizationEntryOutcome::Acknowledged => 1,
        SynchronizationEntryOutcome::RetryableFailure => 2,
        SynchronizationEntryOutcome::PermanentFailure => 3,
        SynchronizationEntryOutcome::ManualReview => 4,
    }
}

fn finish(hasher: Sha256) -> SynchronizationPayloadFingerprint {
    let digest = hasher.finalize();

    let mut bytes = [0_u8; SYNCHRONIZATION_FINGERPRINT_BYTES];

    bytes.copy_from_slice(&digest);

    SynchronizationPayloadFingerprint::from_bytes(bytes)
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
) -> Result<u8, SynchronizationPayloadFingerprintParseError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),

        _ => Err(
            SynchronizationPayloadFingerprintParseError::InvalidHexadecimalCharacter { position },
        ),
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };

    use crate::{
        CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
        SynchronizationAcknowledgementEntry, SynchronizationBatchAcknowledgement,
        SynchronizationBatchAcknowledgementDefinition, SynchronizationEntryOutcome,
        SynchronizationFailureCategory,
    };

    use super::{SynchronizationPayloadFingerprint, SynchronizationPayloadFingerprintParseError};

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

    fn envelope(value: &str) -> CanonicalTransactionEnvelope {
        match CanonicalTransactionEnvelope::from_json(value) {
            Ok(envelope) => envelope,

            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        }
    }

    fn acknowledgement(
        reader_id: ReaderId,
        batch_id: SynchronizationBatchId,
        transaction_id: FareTransactionId,
        outcome: SynchronizationEntryOutcome,
        replayed: bool,
    ) -> SynchronizationBatchAcknowledgement {
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

        let entry = match SynchronizationAcknowledgementEntry::new(
            transaction_id,
            sequence(1),
            outcome,
            metadata.0,
            metadata.1,
        ) {
            Ok(entry) => entry,

            Err(error) => {
                panic!("valid acknowledgement entry failed: {error}")
            }
        };

        match SynchronizationBatchAcknowledgement::new(
            SynchronizationBatchAcknowledgementDefinition {
                protocol_version: DeviceProtocolVersion::CURRENT,
                environment_id: environment(),
                reader_id,
                batch_id,
                first_local_sequence_number: sequence(1),
                last_local_sequence_number: sequence(1),
                received_at_unix_milliseconds: TEST_TIME,
                replayed,
                entries: vec![entry],
            },
        ) {
            Ok(acknowledgement) => acknowledgement,

            Err(error) => {
                panic!("valid acknowledgement failed: {error}")
            }
        }
    }

    #[test]
    fn equivalent_envelopes_have_identical_fingerprints() {
        let first = envelope(r#"{"z":2,"a":1}"#);
        let second = envelope(r#"{ "a": 1, "z": 2 }"#);

        assert_eq!(
            SynchronizationPayloadFingerprint::for_transaction_envelope(&first),
            SynchronizationPayloadFingerprint::for_transaction_envelope(&second),
        );
    }

    #[test]
    fn changed_envelope_changes_fingerprint() {
        let first = envelope(r#"{"value":1}"#);
        let second = envelope(r#"{"value":2}"#);

        assert_ne!(
            SynchronizationPayloadFingerprint::for_transaction_envelope(&first),
            SynchronizationPayloadFingerprint::for_transaction_envelope(&second),
        );
    }

    #[test]
    fn identical_acknowledgements_have_identical_fingerprints() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let first = acknowledgement(
            reader_id,
            batch_id,
            transaction_id,
            SynchronizationEntryOutcome::Acknowledged,
            false,
        );

        let second = first.clone();

        assert_eq!(
            SynchronizationPayloadFingerprint::for_acknowledgement(&first),
            SynchronizationPayloadFingerprint::for_acknowledgement(&second),
        );
    }

    #[test]
    fn replay_marker_does_not_change_acknowledgement_fingerprint() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let original = acknowledgement(
            reader_id,
            batch_id,
            transaction_id,
            SynchronizationEntryOutcome::Acknowledged,
            false,
        );

        let replay = acknowledgement(
            reader_id,
            batch_id,
            transaction_id,
            SynchronizationEntryOutcome::Acknowledged,
            true,
        );

        assert_eq!(
            SynchronizationPayloadFingerprint::for_acknowledgement(&original),
            SynchronizationPayloadFingerprint::for_acknowledgement(&replay),
        );
    }

    #[test]
    fn changed_outcome_changes_acknowledgement_fingerprint() {
        let reader_id = ReaderId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let transaction_id = FareTransactionId::generate();

        let acknowledged = acknowledgement(
            reader_id,
            batch_id,
            transaction_id,
            SynchronizationEntryOutcome::Acknowledged,
            false,
        );

        let rejected = acknowledgement(
            reader_id,
            batch_id,
            transaction_id,
            SynchronizationEntryOutcome::PermanentFailure,
            false,
        );

        assert_ne!(
            SynchronizationPayloadFingerprint::for_acknowledgement(&acknowledged),
            SynchronizationPayloadFingerprint::for_acknowledgement(&rejected),
        );
    }

    #[test]
    fn payload_fingerprint_round_trips_through_text() {
        let fingerprint = SynchronizationPayloadFingerprint::for_transaction_envelope(&envelope(
            r#"{"value":1}"#,
        ));

        let encoded = fingerprint.to_string();

        let decoded = match encoded.parse::<SynchronizationPayloadFingerprint>() {
            Ok(decoded) => decoded,

            Err(error) => {
                panic!("valid payload fingerprint failed: {error}")
            }
        };

        assert_eq!(encoded.len(), 64);
        assert_eq!(decoded, fingerprint);
    }

    #[test]
    fn payload_fingerprint_round_trips_through_json() {
        let fingerprint = SynchronizationPayloadFingerprint::for_transaction_envelope(&envelope(
            r#"{"value":1}"#,
        ));

        let encoded = match serde_json::to_string(&fingerprint) {
            Ok(encoded) => encoded,

            Err(error) => {
                panic!("payload fingerprint serialization failed: {error}")
            }
        };

        let decoded = match serde_json::from_str::<SynchronizationPayloadFingerprint>(&encoded) {
            Ok(decoded) => decoded,

            Err(error) => {
                panic!("payload fingerprint deserialization failed: {error}")
            }
        };

        assert_eq!(decoded, fingerprint);
    }

    #[test]
    fn invalid_payload_fingerprint_length_is_rejected() {
        let result = "abcd".parse::<SynchronizationPayloadFingerprint>();

        assert_eq!(
            result,
            Err(SynchronizationPayloadFingerprintParseError::InvalidLength { actual_length: 4 })
        );
    }

    #[test]
    fn invalid_payload_hexadecimal_character_is_rejected() {
        let mut encoded = "0".repeat(64);
        encoded.replace_range(8..9, "x");

        let result = encoded.parse::<SynchronizationPayloadFingerprint>();

        assert_eq!(
            result,
            Err(
                SynchronizationPayloadFingerprintParseError::InvalidHexadecimalCharacter {
                    position: 8,
                }
            )
        );
    }
}
