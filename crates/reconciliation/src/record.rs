use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use transitguard_domain::{
    FarePolicyId, FarePolicyVersion, FareTransactionId, Money, ReaderId, SynchronizationBatchId,
};

use crate::{
    EvidenceFingerprint, EvidenceFingerprintError, ReconciliationError, ReconciliationEvidence,
    ReconciliationId, ReconciliationOutcome, ReconciliationStatus, fingerprint_evidence,
    reconcile_fare_evidence,
};

/// Errors produced while validating a reconciliation timestamp.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReconciliationTimeError {
    /// Reconciliation cannot be recorded before the Unix epoch.
    #[error("reconciliation time cannot be negative: {unix_milliseconds}")]
    Negative {
        /// Invalid timestamp.
        unix_milliseconds: i64,
    },
}

/// Backend time at which the authoritative reconciliation was produced.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReconciliationTime(i64);

impl ReconciliationTime {
    /// Creates a reconciliation timestamp from Unix milliseconds.
    pub const fn from_unix_milliseconds(
        unix_milliseconds: i64,
    ) -> Result<Self, ReconciliationTimeError> {
        if unix_milliseconds < 0 {
            return Err(ReconciliationTimeError::Negative { unix_milliseconds });
        }

        Ok(Self(unix_milliseconds))
    }

    /// Returns Unix milliseconds.
    #[must_use]
    pub const fn unix_milliseconds(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ReconciliationTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;

        Self::from_unix_milliseconds(value).map_err(serde::de::Error::custom)
    }
}

/// Errors that prevent construction of an authoritative audit record.
#[derive(Debug, Error)]
pub enum ReconciliationRecordError {
    /// The underlying evidence could not be reconciled safely.
    #[error(transparent)]
    Reconciliation(#[from] ReconciliationError),

    /// An evidence fingerprint could not be produced.
    #[error(transparent)]
    Fingerprint(#[from] EvidenceFingerprintError),
}

/// Immutable authoritative audit record for one reconciliation comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReconciliationRecord {
    id: ReconciliationId,
    transaction_id: FareTransactionId,
    source_batch_id: Option<SynchronizationBatchId>,
    reader_id: ReaderId,
    reader_evidence_fingerprint: EvidenceFingerprint,
    backend_evidence_fingerprint: EvidenceFingerprint,
    reader_policy_id: FarePolicyId,
    reader_policy_version: FarePolicyVersion,
    backend_policy_id: FarePolicyId,
    backend_policy_version: FarePolicyVersion,
    outcome: ReconciliationOutcome,
    status: ReconciliationStatus,
    observed_amount: Option<Money>,
    expected_amount: Option<Money>,
    monetary_difference: Option<Money>,
    reconciled_at: ReconciliationTime,
}

impl ReconciliationRecord {
    /// Creates one authoritative reconciliation audit record.
    ///
    /// The comparison and both evidence fingerprints are recomputed here so
    /// callers cannot supply an audit record that disagrees with its evidence.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: ReconciliationId,
        transaction_id: FareTransactionId,
        source_batch_id: Option<SynchronizationBatchId>,
        reader_id: ReaderId,
        reader_evidence: ReconciliationEvidence,
        backend_evidence: ReconciliationEvidence,
        reconciled_at: ReconciliationTime,
    ) -> Result<Self, ReconciliationRecordError> {
        let comparison = reconcile_fare_evidence(reader_evidence, backend_evidence)?;

        let reader_evidence_fingerprint = fingerprint_evidence(reader_evidence)?;

        let backend_evidence_fingerprint = fingerprint_evidence(backend_evidence)?;

        Ok(Self {
            id,
            transaction_id,
            source_batch_id,
            reader_id,
            reader_evidence_fingerprint,
            backend_evidence_fingerprint,
            reader_policy_id: reader_evidence.policy_id(),
            reader_policy_version: reader_evidence.policy_version(),
            backend_policy_id: backend_evidence.policy_id(),
            backend_policy_version: backend_evidence.policy_version(),
            outcome: comparison.outcome(),
            status: comparison.status(),
            observed_amount: comparison.observed_amount(),
            expected_amount: comparison.expected_amount(),
            monetary_difference: comparison.monetary_difference(),
            reconciled_at,
        })
    }

    /// Returns the reconciliation identity.
    #[must_use]
    pub const fn id(self) -> ReconciliationId {
        self.id
    }

    /// Returns the reconciled transaction identity.
    #[must_use]
    pub const fn transaction_id(self) -> FareTransactionId {
        self.transaction_id
    }

    /// Returns the source synchronization batch, when available.
    #[must_use]
    pub const fn source_batch_id(self) -> Option<SynchronizationBatchId> {
        self.source_batch_id
    }

    /// Returns the reader that produced the source transaction.
    #[must_use]
    pub const fn reader_id(self) -> ReaderId {
        self.reader_id
    }

    /// Returns the immutable reader evidence fingerprint.
    #[must_use]
    pub const fn reader_evidence_fingerprint(self) -> EvidenceFingerprint {
        self.reader_evidence_fingerprint
    }

    /// Returns the immutable backend evidence fingerprint.
    #[must_use]
    pub const fn backend_evidence_fingerprint(self) -> EvidenceFingerprint {
        self.backend_evidence_fingerprint
    }

    /// Returns the reader policy identity.
    #[must_use]
    pub const fn reader_policy_id(self) -> FarePolicyId {
        self.reader_policy_id
    }

    /// Returns the reader policy version.
    #[must_use]
    pub const fn reader_policy_version(self) -> FarePolicyVersion {
        self.reader_policy_version
    }

    /// Returns the backend policy identity.
    #[must_use]
    pub const fn backend_policy_id(self) -> FarePolicyId {
        self.backend_policy_id
    }

    /// Returns the backend policy version.
    #[must_use]
    pub const fn backend_policy_version(self) -> FarePolicyVersion {
        self.backend_policy_version
    }

    /// Returns the reconciliation outcome.
    #[must_use]
    pub const fn outcome(self) -> ReconciliationOutcome {
        self.outcome
    }

    /// Returns the reconciliation lifecycle classification.
    #[must_use]
    pub const fn status(self) -> ReconciliationStatus {
        self.status
    }

    /// Returns the reader-observed approved amount.
    #[must_use]
    pub const fn observed_amount(self) -> Option<Money> {
        self.observed_amount
    }

    /// Returns the authoritative expected approved amount.
    #[must_use]
    pub const fn expected_amount(self) -> Option<Money> {
        self.expected_amount
    }

    /// Returns `expected - observed` when both sides approved.
    #[must_use]
    pub const fn monetary_difference(self) -> Option<Money> {
        self.monetary_difference
    }

    /// Returns when reconciliation was recorded.
    #[must_use]
    pub const fn reconciled_at(self) -> ReconciliationTime {
        self.reconciled_at
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, FareTransactionId, Money, ReaderId, SynchronizationBatchId,
    };

    use crate::{
        ReconciliationDecision, ReconciliationError, ReconciliationEvidence, ReconciliationId,
        ReconciliationOutcome, ReconciliationProductEvidence,
    };

    use super::{
        ReconciliationRecord, ReconciliationRecordError, ReconciliationTime,
        ReconciliationTimeError,
    };

    fn test_time() -> ReconciliationTime {
        match ReconciliationTime::from_unix_milliseconds(1_700_000_100_000) {
            Ok(value) => value,
            Err(error) => panic!("test time failed: {error}"),
        }
    }

    fn evidence(policy_id: FarePolicyId, minor_units: i64) -> ReconciliationEvidence {
        let policy_version = match FarePolicyVersion::new(1) {
            Ok(value) => value,
            Err(error) => panic!("test policy version failed: {error}"),
        };

        let event_time = match EventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,
            Err(error) => panic!("test event time failed: {error}"),
        };

        let amount = Money::from_minor_units(minor_units, Currency::Usd);

        ReconciliationEvidence {
            policy_id,
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
    fn matched_record_preserves_audit_identity() {
        let policy_id = FarePolicyId::generate();
        let reader_evidence = evidence(policy_id, 250);
        let backend_evidence = reader_evidence;

        let id = ReconciliationId::generate();
        let transaction_id = FareTransactionId::generate();
        let batch_id = SynchronizationBatchId::generate();
        let reader_id = ReaderId::generate();

        let result = ReconciliationRecord::create(
            id,
            transaction_id,
            Some(batch_id),
            reader_id,
            reader_evidence,
            backend_evidence,
            test_time(),
        );

        assert!(matches!(
            result,
            Ok(record)
                if record.id() == id
                    && record.transaction_id() == transaction_id
                    && record.source_batch_id() == Some(batch_id)
                    && record.reader_id() == reader_id
                    && record.outcome()
                        == ReconciliationOutcome::Matched
                    && record.reader_evidence_fingerprint()
                        == record.backend_evidence_fingerprint()
        ));
    }

    #[test]
    fn fare_difference_is_preserved_in_audit_record() {
        let policy_id = FarePolicyId::generate();

        let reader_evidence = evidence(policy_id, 250);
        let backend_evidence = evidence(policy_id, 300);

        let result = ReconciliationRecord::create(
            ReconciliationId::generate(),
            FareTransactionId::generate(),
            None,
            ReaderId::generate(),
            reader_evidence,
            backend_evidence,
            test_time(),
        );

        assert!(matches!(
            result,
            Ok(record)
                if record.outcome()
                    == ReconciliationOutcome::FareAmountMismatch
                    && record.monetary_difference()
                        == Some(Money::from_minor_units(
                            50,
                            Currency::Usd
                        ))
                    && record.reader_evidence_fingerprint()
                        != record.backend_evidence_fingerprint()
        ));
    }

    #[test]
    fn inconsistent_evidence_cannot_create_a_record() {
        let policy_id = FarePolicyId::generate();

        let reader_evidence = evidence(policy_id, 250);
        let mut backend_evidence = reader_evidence;

        backend_evidence.final_fare = Money::from_minor_units(300, Currency::Usd);

        let result = ReconciliationRecord::create(
            ReconciliationId::generate(),
            FareTransactionId::generate(),
            None,
            ReaderId::generate(),
            reader_evidence,
            backend_evidence,
            test_time(),
        );

        assert!(matches!(
            result,
            Err(ReconciliationRecordError::Reconciliation(
                ReconciliationError::InconsistentApprovedFare { .. }
            ))
        ));
    }

    #[test]
    fn reconciliation_time_rejects_negative_values() {
        let result = ReconciliationTime::from_unix_milliseconds(-1);

        assert_eq!(
            result,
            Err(ReconciliationTimeError::Negative {
                unix_milliseconds: -1
            })
        );
    }
}
