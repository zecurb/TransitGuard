use core::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::ReconciliationEvidence;

/// Current canonical evidence-fingerprint format.
pub const EVIDENCE_FINGERPRINT_VERSION: u8 = 1;

/// SHA-256 fingerprint of one immutable reconciliation evidence value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct EvidenceFingerprint {
    version: u8,
    digest: [u8; 32],
}

impl EvidenceFingerprint {
    /// Returns the fingerprint format version.
    #[must_use]
    pub const fn version(self) -> u8 {
        self.version
    }

    /// Returns the raw SHA-256 digest.
    #[must_use]
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
}

impl fmt::Display for EvidenceFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "v{}:", self.version)?;

        for byte in self.digest {
            write!(formatter, "{byte:02x}")?;
        }

        Ok(())
    }
}

/// Errors produced while fingerprinting reconciliation evidence.
#[derive(Debug, Error)]
pub enum EvidenceFingerprintError {
    /// Immutable evidence could not be serialized.
    #[error("reconciliation evidence serialization failed")]
    Serialization(#[from] serde_json::Error),
}

/// Produces the versioned canonical fingerprint of reconciliation evidence.
///
/// The fingerprint is domain-separated from other TransitGuard hashes and
/// contains only the immutable evidence structure. It does not contain
/// credentials, secrets, database errors, or transport response bodies.
pub fn fingerprint_evidence(
    evidence: ReconciliationEvidence,
) -> Result<EvidenceFingerprint, EvidenceFingerprintError> {
    let payload = serde_json::to_vec(&evidence)?;

    let mut hasher = Sha256::new();

    hasher.update(b"transitguard/reconciliation-evidence");
    hasher.update([EVIDENCE_FINGERPRINT_VERSION]);
    hasher.update(payload);

    let result = hasher.finalize();

    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&result);

    Ok(EvidenceFingerprint {
        version: EVIDENCE_FINGERPRINT_VERSION,
        digest,
    })
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, Money,
    };

    use crate::{ReconciliationDecision, ReconciliationEvidence, ReconciliationProductEvidence};

    use super::{EVIDENCE_FINGERPRINT_VERSION, fingerprint_evidence};

    fn test_evidence() -> ReconciliationEvidence {
        let policy_version = match FarePolicyVersion::new(1) {
            Ok(value) => value,
            Err(error) => panic!("test policy version failed: {error}"),
        };

        let event_time = match EventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,
            Err(error) => panic!("test event time failed: {error}"),
        };

        let amount = Money::from_minor_units(250, Currency::Usd);

        ReconciliationEvidence {
            policy_id: FarePolicyId::generate(),
            policy_version,
            event_time,
            decision: ReconciliationDecision::Approved {
                charged_amount: amount,
                reason: FareApprovalReason::StandardFare,
            },
            eligibility: EligibilityClassification::Standard,
            eligibility_discount: Money::zero(Currency::Usd),
            transfer_eligible: false,
            transfer_discount: Money::zero(Currency::Usd),
            fare_cap_discount: Money::zero(Currency::Usd),
            daily_cap_reached: false,
            weekly_cap_reached: false,
            product_outcome: ReconciliationProductEvidence::NotPresented,
            product_discount: Money::zero(Currency::Usd),
            final_fare: amount,
        }
    }

    #[test]
    fn identical_evidence_has_identical_fingerprint() {
        let evidence = test_evidence();

        let first = fingerprint_evidence(evidence);
        let second = fingerprint_evidence(evidence);

        assert!(matches!(
            (first, second),
            (Ok(left), Ok(right))
                if left == right
                    && left.version()
                        == EVIDENCE_FINGERPRINT_VERSION
        ));
    }

    #[test]
    fn changed_evidence_changes_fingerprint() {
        let original = test_evidence();
        let mut changed = original;

        changed.daily_cap_reached = true;

        let first = fingerprint_evidence(original);
        let second = fingerprint_evidence(changed);

        assert!(matches!(
            (first, second),
            (Ok(left), Ok(right)) if left != right
        ));
    }
}
