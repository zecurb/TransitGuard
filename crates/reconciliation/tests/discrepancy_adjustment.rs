use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
    FarePolicyVersion, FareTransactionId, Money, ReaderId,
};
use transitguard_reconciliation::{
    DiscrepancyCase, DiscrepancyCaseError, DiscrepancyCategory, DiscrepancyResolutionReason,
    DiscrepancyState, ProposedAdjustment, ProposedAdjustmentDirection, ProposedAdjustmentError,
    ReconciliationDecision, ReconciliationEvidence, ReconciliationId, ReconciliationOutcome,
    ReconciliationProductEvidence, ReconciliationRecord, ReconciliationTime, ResolutionActorId,
};

fn reconciliation_time(value: i64) -> ReconciliationTime {
    match ReconciliationTime::from_unix_milliseconds(value) {
        Ok(time) => time,
        Err(error) => panic!("test time failed: {error}"),
    }
}

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

fn evidence(policy_id: FarePolicyId, amount: i64) -> ReconciliationEvidence {
    let amount = Money::from_minor_units(amount, Currency::Usd);

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

fn mismatch_record() -> ReconciliationRecord {
    let policy_id = FarePolicyId::generate();

    match ReconciliationRecord::create(
        ReconciliationId::generate(),
        FareTransactionId::generate(),
        None,
        ReaderId::generate(),
        evidence(policy_id, 250),
        evidence(policy_id, 300),
        reconciliation_time(1_700_000_100_000),
    ) {
        Ok(record) => record,
        Err(error) => {
            panic!("test record failed: {error}")
        }
    }
}

#[test]
fn fare_mismatch_creates_open_discrepancy() {
    let record = mismatch_record();

    let result = DiscrepancyCase::from_reconciliation(record);

    assert!(matches!(
        result,
        Ok(case)
            if case.category()
                == DiscrepancyCategory::FareAmountMismatch
                && case.state()
                    == DiscrepancyState::Open
                && case.reconciliation_id()
                    == record.id()
                && case.id().reconciliation_id()
                    == record.id()
    ));
}

#[test]
fn resolution_preserves_transition_history() {
    let record = mismatch_record();

    let mut case = match DiscrepancyCase::from_reconciliation(record) {
        Ok(value) => value,
        Err(error) => {
            panic!("test discrepancy failed: {error}")
        }
    };

    let actor = ResolutionActorId::generate();
    let resolved_at = reconciliation_time(1_700_000_200_000);

    let result = case.resolve(
        actor,
        DiscrepancyResolutionReason::BackendEvidenceConfirmed,
        resolved_at,
    );

    assert!(result.is_ok());
    assert_eq!(case.state(), DiscrepancyState::Resolved);
    assert_eq!(case.history().len(), 1);
    assert_eq!(case.history()[0].from(), DiscrepancyState::Open);
    assert_eq!(case.history()[0].to(), DiscrepancyState::Resolved);
    assert_eq!(case.history()[0].occurred_at(), resolved_at);
}

#[test]
fn identical_resolution_replay_is_idempotent() {
    let record = mismatch_record();

    let mut case = match DiscrepancyCase::from_reconciliation(record) {
        Ok(value) => value,
        Err(error) => {
            panic!("test discrepancy failed: {error}")
        }
    };

    let actor = ResolutionActorId::generate();
    let resolved_at = reconciliation_time(1_700_000_200_000);

    let first = case.resolve(
        actor,
        DiscrepancyResolutionReason::BackendEvidenceConfirmed,
        resolved_at,
    );

    let second = case.resolve(
        actor,
        DiscrepancyResolutionReason::BackendEvidenceConfirmed,
        resolved_at,
    );

    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(case.history().len(), 1);
}

#[test]
fn conflicting_final_resolution_is_rejected() {
    let record = mismatch_record();

    let mut case = match DiscrepancyCase::from_reconciliation(record) {
        Ok(value) => value,
        Err(error) => {
            panic!("test discrepancy failed: {error}")
        }
    };

    let actor = ResolutionActorId::generate();
    let resolved_at = reconciliation_time(1_700_000_200_000);

    let first = case.resolve(
        actor,
        DiscrepancyResolutionReason::BackendEvidenceConfirmed,
        resolved_at,
    );

    assert!(first.is_ok());

    let second = case.dismiss(
        actor,
        DiscrepancyResolutionReason::NoFinancialImpact,
        resolved_at,
    );

    assert!(matches!(
        second,
        Err(DiscrepancyCaseError::AlreadyFinalized {
            state: DiscrepancyState::Resolved
        })
    ));
}

#[test]
fn fare_mismatch_proposes_stable_adjustment() {
    let record = mismatch_record();

    let first = ProposedAdjustment::from_reconciliation(record);

    let second = ProposedAdjustment::from_reconciliation(record);

    assert!(matches!(
        (first, second),
        (Ok(left), Ok(right))
            if left == right
                && left.id() == right.id()
                && left.reconciliation_id()
                    == record.id()
                && left.correction_amount()
                    == Money::from_minor_units(
                        50,
                        Currency::Usd
                    )
                && left.direction()
                    == ProposedAdjustmentDirection::IncreaseRecordedFare
    ));
}

#[test]
fn matched_reconciliation_has_no_discrepancy_or_adjustment() {
    let policy_id = FarePolicyId::generate();
    let same = evidence(policy_id, 250);

    let record = match ReconciliationRecord::create(
        ReconciliationId::generate(),
        FareTransactionId::generate(),
        None,
        ReaderId::generate(),
        same,
        same,
        reconciliation_time(1_700_000_100_000),
    ) {
        Ok(record) => record,
        Err(error) => {
            panic!("test record failed: {error}")
        }
    };

    assert_eq!(record.outcome(), ReconciliationOutcome::Matched);

    assert!(matches!(
        DiscrepancyCase::from_reconciliation(record),
        Err(DiscrepancyCaseError::MatchedReconciliation)
    ));

    assert!(matches!(
        ProposedAdjustment::from_reconciliation(record),
        Err(ProposedAdjustmentError::UnsupportedOutcome {
            outcome: ReconciliationOutcome::Matched
        })
    ));
}
