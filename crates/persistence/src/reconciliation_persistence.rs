use thiserror::Error;
use transitguard_reconciliation::{
    DiscrepancyCase, DiscrepancyCaseError, ProposedAdjustment, ProposedAdjustmentError,
    ReconciliationEvidence, ReconciliationOutcome, ReconciliationRecord, ReconciliationRecordError,
};

/// Errors produced while preparing one reconciliation for durable storage.
#[derive(Debug, Error)]
pub enum ReconciliationPreparationError {
    /// Supplied evidence could not reproduce a trusted reconciliation.
    #[error(transparent)]
    Reconciliation(#[from] ReconciliationRecordError),

    /// The supplied record does not agree with the supplied immutable evidence.
    #[error("reconciliation record does not match its reader and backend evidence")]
    RecordEvidenceMismatch,

    /// A required discrepancy case could not be produced.
    #[error(transparent)]
    Discrepancy(#[from] DiscrepancyCaseError),

    /// A required proposed adjustment could not be produced.
    #[error(transparent)]
    Adjustment(#[from] ProposedAdjustmentError),
}

/// Complete validated unit that the PostgreSQL reconciliation repository stores.
///
/// Preparation recomputes the authoritative reconciliation from its immutable
/// reader and backend evidence. This prevents persistence code from accepting a
/// record whose outcome, monetary difference, policy evidence, or fingerprints
/// disagree with the evidence that will be written beside it.
///
/// Discrepancy and proposed-adjustment records are derived here rather than
/// supplied independently, preventing callers from accidentally omitting or
/// mismatching dependent financial-reconciliation state.
#[derive(Debug)]
pub struct PreparedReconciliationPersistence {
    record: ReconciliationRecord,
    reader_evidence: ReconciliationEvidence,
    backend_evidence: ReconciliationEvidence,
    discrepancy_case: Option<DiscrepancyCase>,
    proposed_adjustment: Option<ProposedAdjustment>,
}

impl PreparedReconciliationPersistence {
    /// Validates and prepares one reconciliation for transactional persistence.
    pub fn prepare(
        record: ReconciliationRecord,
        reader_evidence: ReconciliationEvidence,
        backend_evidence: ReconciliationEvidence,
    ) -> Result<Self, ReconciliationPreparationError> {
        let reconstructed = ReconciliationRecord::create(
            record.id(),
            record.transaction_id(),
            record.source_batch_id(),
            record.reader_id(),
            reader_evidence,
            backend_evidence,
            record.reconciled_at(),
        )?;

        if reconstructed != record {
            return Err(ReconciliationPreparationError::RecordEvidenceMismatch);
        }

        let discrepancy_case = if record.outcome() == ReconciliationOutcome::Matched {
            None
        } else {
            Some(DiscrepancyCase::from_reconciliation(record)?)
        };

        let proposed_adjustment = if record.outcome() == ReconciliationOutcome::FareAmountMismatch {
            Some(ProposedAdjustment::from_reconciliation(record)?)
        } else {
            None
        };

        Ok(Self {
            record,
            reader_evidence,
            backend_evidence,
            discrepancy_case,
            proposed_adjustment,
        })
    }

    /// Returns the authoritative reconciliation record.
    #[must_use]
    pub const fn record(&self) -> ReconciliationRecord {
        self.record
    }

    /// Returns immutable reader-produced reconciliation evidence.
    #[must_use]
    pub const fn reader_evidence(&self) -> ReconciliationEvidence {
        self.reader_evidence
    }

    /// Returns immutable backend-produced reconciliation evidence.
    #[must_use]
    pub const fn backend_evidence(&self) -> ReconciliationEvidence {
        self.backend_evidence
    }

    /// Returns the required discrepancy case for an unmatched result.
    #[must_use]
    pub const fn discrepancy_case(&self) -> Option<&DiscrepancyCase> {
        self.discrepancy_case.as_ref()
    }

    /// Returns the deterministic proposed adjustment for a fare mismatch.
    #[must_use]
    pub const fn proposed_adjustment(&self) -> Option<ProposedAdjustment> {
        self.proposed_adjustment
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, FareTransactionId, Money, ReaderId,
    };
    use transitguard_reconciliation::{
        DiscrepancyCategory, ProposedAdjustmentDirection, ReconciliationDecision,
        ReconciliationEvidence, ReconciliationId, ReconciliationOutcome,
        ReconciliationProductEvidence, ReconciliationRecord, ReconciliationTime,
    };

    use super::{PreparedReconciliationPersistence, ReconciliationPreparationError};

    fn policy_version() -> FarePolicyVersion {
        match FarePolicyVersion::new(1) {
            Ok(value) => value,

            Err(error) => {
                panic!("test policy version failed: {error}")
            }
        }
    }

    fn event_time() -> EventTime {
        match EventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,

            Err(error) => {
                panic!("test event time failed: {error}")
            }
        }
    }

    fn reconciliation_time() -> ReconciliationTime {
        match ReconciliationTime::from_unix_milliseconds(1_700_000_100_000) {
            Ok(value) => value,

            Err(error) => {
                panic!("test reconciliation time failed: {error}")
            }
        }
    }

    fn evidence(policy_id: FarePolicyId, minor_units: i64) -> ReconciliationEvidence {
        let amount = Money::from_minor_units(minor_units, Currency::Usd);

        ReconciliationEvidence::test_fixture(
            policy_id,
            policy_version(),
            event_time(),
            ReconciliationDecision::Approved {
                charged_amount: amount,
                reason: FareApprovalReason::StandardFare,
            },
            EligibilityClassification::Standard,
            Money::zero(Currency::Usd),
            false,
            Money::zero(Currency::Usd),
            Money::zero(Currency::Usd),
            false,
            false,
            ReconciliationProductEvidence::NotPresented,
            Money::zero(Currency::Usd),
            amount,
        )
    }

    fn record(
        reader_evidence: ReconciliationEvidence,
        backend_evidence: ReconciliationEvidence,
    ) -> ReconciliationRecord {
        match ReconciliationRecord::create(
            ReconciliationId::generate(),
            FareTransactionId::generate(),
            None,
            ReaderId::generate(),
            reader_evidence,
            backend_evidence,
            reconciliation_time(),
        ) {
            Ok(value) => value,

            Err(error) => {
                panic!("test reconciliation failed: {error}")
            }
        }
    }

    #[test]
    fn matched_reconciliation_has_no_dependent_records() {
        let policy_id = FarePolicyId::generate();
        let reader = evidence(policy_id, 250);
        let backend = reader;

        let reconciliation = record(reader, backend);

        let prepared = PreparedReconciliationPersistence::prepare(reconciliation, reader, backend);

        assert!(matches!(
            prepared,
            Ok(prepared)
                if prepared.record() == reconciliation
                    && prepared.discrepancy_case().is_none()
                    && prepared.proposed_adjustment().is_none()
        ));
    }

    #[test]
    fn fare_mismatch_derives_discrepancy_and_adjustment() {
        let policy_id = FarePolicyId::generate();

        let reader = evidence(policy_id, 250);
        let backend = evidence(policy_id, 300);

        let reconciliation = record(reader, backend);

        let prepared = PreparedReconciliationPersistence::prepare(reconciliation, reader, backend);

        assert!(matches!(
            prepared,
            Ok(prepared)
                if prepared.record().outcome()
                    == ReconciliationOutcome::FareAmountMismatch
                    && prepared
                        .discrepancy_case()
                        .is_some_and(|case| {
                            case.category()
                                == DiscrepancyCategory::FareAmountMismatch
                        })
                    && prepared
                        .proposed_adjustment()
                        .is_some_and(|adjustment| {
                            adjustment.direction()
                                == ProposedAdjustmentDirection::IncreaseRecordedFare
                                && adjustment.correction_amount()
                                    == Money::from_minor_units(
                                        50,
                                        Currency::Usd
                                    )
                        })
        ));
    }

    #[test]
    fn conflicting_evidence_is_rejected_before_persistence() {
        let policy_id = FarePolicyId::generate();

        let reader = evidence(policy_id, 250);
        let original_backend = evidence(policy_id, 300);

        let reconciliation = record(reader, original_backend);

        let conflicting_backend = evidence(policy_id, 350);

        let prepared =
            PreparedReconciliationPersistence::prepare(reconciliation, reader, conflicting_backend);

        assert!(matches!(
            prepared,
            Err(ReconciliationPreparationError::RecordEvidenceMismatch)
        ));
    }

    #[test]
    fn evidence_json_round_trips_for_future_database_loading() {
        let policy_id = FarePolicyId::generate();
        let original = evidence(policy_id, 250);

        let json = match serde_json::to_value(original) {
            Ok(value) => value,

            Err(error) => {
                panic!("test serialization failed: {error}")
            }
        };

        let decoded = serde_json::from_value::<ReconciliationEvidence>(json);

        assert!(matches!(
            decoded,
            Ok(value) if value == original
        ));
    }
}
