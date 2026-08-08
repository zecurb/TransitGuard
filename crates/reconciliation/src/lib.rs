//! Deterministic financial reconciliation for TransitGuard.
//!
//! This crate compares immutable reader fare evidence with authoritative
//! backend fare evidence. The comparison kernel performs no database access,
//! network access, clock reads, or other hidden I/O.

use core::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{
    EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId, FarePolicyVersion,
    FareRejectionReason, Money, MoneyError,
};
use transitguard_fare_engine::{FareEvaluation, FareEvaluationOutcome, ProductApplicationOutcome};

/// Lifecycle classification of a reconciliation comparison.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReconciliationStatus {
    /// Reader and backend evidence matched.
    Matched,

    /// An explicit discrepancy was found.
    Discrepancy,

    /// The evidence requires investigation rather than automatic resolution.
    ManualReview,
}

/// Stable Phase 8 reconciliation result classifications.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReconciliationOutcome {
    /// Reader and authoritative backend evidence agree.
    Matched,

    /// Reader and backend approved different monetary amounts.
    FareAmountMismatch,

    /// Fare-policy identity or version differs.
    PolicyVersionMismatch,

    /// Eligibility evidence differs.
    EligibilityMismatch,

    /// Transit-product evidence differs.
    ProductMismatch,

    /// Transfer evidence differs.
    TransferMismatch,

    /// Daily or weekly fare-cap evidence differs.
    FareCapMismatch,

    /// Higher-level processing detected duplicate source activity.
    DuplicateTransaction,

    /// Required historical backend context is unavailable.
    MissingBackendContext,

    /// Supplied evidence violates reconciliation invariants.
    InvalidEvidence,

    /// Evidence differs in a way requiring explicit investigation.
    ManualReviewRequired,
}

impl ReconciliationOutcome {
    /// Returns the lifecycle status represented by this outcome.
    #[must_use]
    pub const fn status(self) -> ReconciliationStatus {
        match self {
            Self::Matched => ReconciliationStatus::Matched,

            Self::FareAmountMismatch
            | Self::PolicyVersionMismatch
            | Self::EligibilityMismatch
            | Self::ProductMismatch
            | Self::TransferMismatch
            | Self::FareCapMismatch => ReconciliationStatus::Discrepancy,

            Self::DuplicateTransaction
            | Self::MissingBackendContext
            | Self::InvalidEvidence
            | Self::ManualReviewRequired => ReconciliationStatus::ManualReview,
        }
    }
}

/// Fare decision normalized for reconciliation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReconciliationDecision {
    /// The fare presentation was approved.
    Approved {
        /// Amount charged.
        charged_amount: Money,

        /// Stable reason for approval.
        reason: FareApprovalReason,
    },

    /// The fare presentation was rejected.
    Rejected {
        /// Stable reason for rejection.
        reason: FareRejectionReason,
    },
}

impl ReconciliationDecision {
    /// Returns the approved amount, when the decision approved the fare.
    #[must_use]
    pub const fn charged_amount(self) -> Option<Money> {
        match self {
            Self::Approved { charged_amount, .. } => Some(charged_amount),

            Self::Rejected { .. } => None,
        }
    }
}

impl From<FareEvaluationOutcome> for ReconciliationDecision {
    fn from(value: FareEvaluationOutcome) -> Self {
        match value {
            FareEvaluationOutcome::Approved {
                charged_amount,
                reason,
            } => Self::Approved {
                charged_amount,
                reason,
            },

            FareEvaluationOutcome::Rejected { reason } => Self::Rejected { reason },
        }
    }
}

/// Product evidence reduced to the information required for reconciliation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ReconciliationProductEvidence {
    /// No product was presented.
    NotPresented,

    /// A valid product covered the fare.
    Covered,

    /// A product was presented but was invalid.
    Invalid,
}

impl From<ProductApplicationOutcome> for ReconciliationProductEvidence {
    fn from(value: ProductApplicationOutcome) -> Self {
        match value {
            ProductApplicationOutcome::NotPresented => Self::NotPresented,
            ProductApplicationOutcome::Covered => Self::Covered,
            ProductApplicationOutcome::Invalid { .. } => Self::Invalid,
        }
    }
}

/// Immutable evidence consumed by the reconciliation kernel.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ReconciliationEvidence {
    policy_id: FarePolicyId,
    policy_version: FarePolicyVersion,
    event_time: EventTime,
    decision: ReconciliationDecision,
    eligibility: EligibilityClassification,
    eligibility_discount: Money,
    transfer_eligible: bool,
    transfer_discount: Money,
    fare_cap_discount: Money,
    daily_cap_reached: bool,
    weekly_cap_reached: bool,
    product_outcome: ReconciliationProductEvidence,
    product_discount: Money,
    final_fare: Money,
}

impl ReconciliationEvidence {
    /// Converts one deterministic fare-engine result into reconciliation
    /// evidence.
    #[must_use]
    pub fn from_fare_evaluation(evaluation: FareEvaluation) -> Self {
        let evidence = evaluation.evidence();

        Self {
            policy_id: evaluation.policy_id(),
            policy_version: evaluation.policy_version(),
            event_time: evaluation.event_time(),
            decision: evaluation.outcome().into(),
            eligibility: evidence.eligibility(),
            eligibility_discount: evidence.eligibility_discount(),
            transfer_eligible: evidence.transfer_eligible(),
            transfer_discount: evidence.transfer_discount(),
            fare_cap_discount: evidence.fare_cap_discount(),
            daily_cap_reached: evidence.daily_cap_reached(),
            weekly_cap_reached: evidence.weekly_cap_reached(),
            product_outcome: evidence.product_outcome().into(),
            product_discount: evidence.product_discount(),
            final_fare: evidence.final_fare(),
        }
    }

    /// Returns the fare-policy identity.
    #[must_use]
    pub const fn policy_id(self) -> FarePolicyId {
        self.policy_id
    }

    /// Returns the fare-policy version.
    #[must_use]
    pub const fn policy_version(self) -> FarePolicyVersion {
        self.policy_version
    }

    /// Returns the event time used by the evaluation.
    #[must_use]
    pub const fn event_time(self) -> EventTime {
        self.event_time
    }

    /// Returns the normalized fare decision.
    #[must_use]
    pub const fn decision(self) -> ReconciliationDecision {
        self.decision
    }

    /// Returns the calculated final fare.
    #[must_use]
    pub const fn final_fare(self) -> Money {
        self.final_fare
    }
}

/// Identifies which side supplied invalid evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EvidenceSide {
    /// Reader-produced evidence.
    Reader,

    /// Authoritative backend evidence.
    Backend,
}

impl fmt::Display for EvidenceSide {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reader => formatter.write_str("reader"),
            Self::Backend => formatter.write_str("backend"),
        }
    }
}

/// Errors that prevent a trusted reconciliation result.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ReconciliationError {
    /// Currency-safe monetary arithmetic failed.
    #[error(transparent)]
    Money(#[from] MoneyError),

    /// An approval contradicted its own final-fare evidence.
    #[error("{side} approved amount {charged_amount} does not match final fare {final_fare}")]
    InconsistentApprovedFare {
        /// Side containing inconsistent evidence.
        side: EvidenceSide,

        /// Approved amount.
        charged_amount: Money,

        /// Final fare carried by the evidence.
        final_fare: Money,
    },
}

/// Complete deterministic reconciliation comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ReconciliationComparison {
    outcome: ReconciliationOutcome,
    observed_amount: Option<Money>,
    expected_amount: Option<Money>,
    monetary_difference: Option<Money>,
    reader_policy_version: FarePolicyVersion,
    backend_policy_version: FarePolicyVersion,
}

impl ReconciliationComparison {
    /// Returns the stable comparison outcome.
    #[must_use]
    pub const fn outcome(self) -> ReconciliationOutcome {
        self.outcome
    }

    /// Returns the corresponding lifecycle status.
    #[must_use]
    pub const fn status(self) -> ReconciliationStatus {
        self.outcome.status()
    }

    /// Returns the amount reported by the reader when approved.
    #[must_use]
    pub const fn observed_amount(self) -> Option<Money> {
        self.observed_amount
    }

    /// Returns the amount expected by the backend when approved.
    #[must_use]
    pub const fn expected_amount(self) -> Option<Money> {
        self.expected_amount
    }

    /// Returns `expected - observed` when both sides approved.
    ///
    /// A positive value represents a reader undercharge. A negative value
    /// represents a reader overcharge.
    #[must_use]
    pub const fn monetary_difference(self) -> Option<Money> {
        self.monetary_difference
    }

    /// Returns the policy version reported by the reader.
    #[must_use]
    pub const fn reader_policy_version(self) -> FarePolicyVersion {
        self.reader_policy_version
    }

    /// Returns the policy version used by the backend.
    #[must_use]
    pub const fn backend_policy_version(self) -> FarePolicyVersion {
        self.backend_policy_version
    }
}

/// Deterministically compares reader evidence with authoritative backend
/// evidence.
///
/// Classification order is stable:
///
/// 1. policy identity and version;
/// 2. event identity;
/// 3. eligibility;
/// 4. transit product;
/// 5. transfer;
/// 6. fare caps;
/// 7. approved monetary amount;
/// 8. remaining decision differences.
///
/// The function performs no I/O.
pub fn reconcile_fare_evidence(
    reader: ReconciliationEvidence,
    backend: ReconciliationEvidence,
) -> Result<ReconciliationComparison, ReconciliationError> {
    validate_evidence(reader, EvidenceSide::Reader)?;
    validate_evidence(backend, EvidenceSide::Backend)?;

    let observed_amount = reader.decision.charged_amount();
    let expected_amount = backend.decision.charged_amount();

    let monetary_difference = match (observed_amount, expected_amount) {
        (Some(observed), Some(expected)) => Some(expected.checked_subtract(observed)?),

        _ => None,
    };

    let outcome = classify(reader, backend, observed_amount, expected_amount);

    Ok(ReconciliationComparison {
        outcome,
        observed_amount,
        expected_amount,
        monetary_difference,
        reader_policy_version: reader.policy_version,
        backend_policy_version: backend.policy_version,
    })
}

fn validate_evidence(
    evidence: ReconciliationEvidence,
    side: EvidenceSide,
) -> Result<(), ReconciliationError> {
    if let ReconciliationDecision::Approved { charged_amount, .. } = evidence.decision
        && charged_amount != evidence.final_fare
    {
        return Err(ReconciliationError::InconsistentApprovedFare {
            side,
            charged_amount,
            final_fare: evidence.final_fare,
        });
    }

    Ok(())
}

fn classify(
    reader: ReconciliationEvidence,
    backend: ReconciliationEvidence,
    observed_amount: Option<Money>,
    expected_amount: Option<Money>,
) -> ReconciliationOutcome {
    if reader.policy_id != backend.policy_id || reader.policy_version != backend.policy_version {
        return ReconciliationOutcome::PolicyVersionMismatch;
    }

    if reader.event_time != backend.event_time {
        return ReconciliationOutcome::InvalidEvidence;
    }

    if reader.eligibility != backend.eligibility
        || reader.eligibility_discount != backend.eligibility_discount
    {
        return ReconciliationOutcome::EligibilityMismatch;
    }

    if reader.product_outcome != backend.product_outcome
        || reader.product_discount != backend.product_discount
    {
        return ReconciliationOutcome::ProductMismatch;
    }

    if reader.transfer_eligible != backend.transfer_eligible
        || reader.transfer_discount != backend.transfer_discount
    {
        return ReconciliationOutcome::TransferMismatch;
    }

    if reader.fare_cap_discount != backend.fare_cap_discount
        || reader.daily_cap_reached != backend.daily_cap_reached
        || reader.weekly_cap_reached != backend.weekly_cap_reached
    {
        return ReconciliationOutcome::FareCapMismatch;
    }

    match (reader.decision, backend.decision) {
        (
            ReconciliationDecision::Approved {
                reason: reader_reason,
                ..
            },
            ReconciliationDecision::Approved {
                reason: backend_reason,
                ..
            },
        ) => {
            if observed_amount != expected_amount {
                ReconciliationOutcome::FareAmountMismatch
            } else if reader_reason != backend_reason {
                ReconciliationOutcome::ManualReviewRequired
            } else {
                ReconciliationOutcome::Matched
            }
        }

        (
            ReconciliationDecision::Rejected {
                reason: reader_reason,
            },
            ReconciliationDecision::Rejected {
                reason: backend_reason,
            },
        ) => {
            if reader_reason == backend_reason {
                ReconciliationOutcome::Matched
            } else {
                ReconciliationOutcome::ManualReviewRequired
            }
        }

        _ => ReconciliationOutcome::ManualReviewRequired,
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, FareRejectionReason, Money,
    };

    use super::{
        ReconciliationDecision, ReconciliationEvidence, ReconciliationOutcome,
        ReconciliationProductEvidence, ReconciliationStatus, reconcile_fare_evidence,
    };

    fn policy_version(value: u64) -> FarePolicyVersion {
        match FarePolicyVersion::new(value) {
            Ok(version) => version,
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

    fn approved_evidence(minor_units: i64) -> ReconciliationEvidence {
        let amount = Money::from_minor_units(minor_units, Currency::Usd);

        ReconciliationEvidence {
            policy_id: FarePolicyId::generate(),
            policy_version: policy_version(1),
            event_time: event_time(),
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
    fn identical_evidence_matches() {
        let reader = approved_evidence(250);
        let backend = reader;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::Matched
                    && comparison.status()
                        == ReconciliationStatus::Matched
                    && comparison.monetary_difference()
                        == Some(Money::zero(Currency::Usd))
        ));
    }

    #[test]
    fn fare_amount_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;
        let expected = Money::from_minor_units(300, Currency::Usd);

        backend.decision = ReconciliationDecision::Approved {
            charged_amount: expected,
            reason: FareApprovalReason::StandardFare,
        };
        backend.final_fare = expected;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::FareAmountMismatch
                    && comparison.monetary_difference()
                        == Some(Money::from_minor_units(
                            50,
                            Currency::Usd
                        ))
        ));
    }

    #[test]
    fn policy_version_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        backend.policy_version = policy_version(2);

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::PolicyVersionMismatch
        ));
    }

    #[test]
    fn eligibility_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        backend.eligibility = EligibilityClassification::Youth;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::EligibilityMismatch
        ));
    }

    #[test]
    fn product_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        backend.product_outcome = ReconciliationProductEvidence::Covered;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::ProductMismatch
        ));
    }

    #[test]
    fn transfer_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        backend.transfer_eligible = true;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::TransferMismatch
        ));
    }

    #[test]
    fn fare_cap_difference_is_explicit() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        backend.daily_cap_reached = true;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::FareCapMismatch
        ));
    }

    #[test]
    fn rejection_difference_requires_review() {
        let mut reader = approved_evidence(0);

        reader.decision = ReconciliationDecision::Rejected {
            reason: FareRejectionReason::InsufficientStoredValue,
        };

        let mut backend = reader;

        backend.decision = ReconciliationDecision::Rejected {
            reason: FareRejectionReason::AccountSuspended,
        };

        let result = reconcile_fare_evidence(reader, backend);

        assert!(matches!(
            result,
            Ok(comparison)
                if comparison.outcome()
                    == ReconciliationOutcome::ManualReviewRequired
                    && comparison.status()
                        == ReconciliationStatus::ManualReview
        ));
    }

    #[test]
    fn cross_currency_approved_amounts_are_rejected() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        let euros = Money::from_minor_units(250, Currency::Eur);

        backend.decision = ReconciliationDecision::Approved {
            charged_amount: euros,
            reason: FareApprovalReason::StandardFare,
        };
        backend.final_fare = euros;

        let result = reconcile_fare_evidence(reader, backend);

        assert!(result.is_err());
    }

    #[test]
    fn repeated_comparison_is_deterministic() {
        let reader = approved_evidence(250);
        let mut backend = reader;

        let expected = Money::from_minor_units(300, Currency::Usd);

        backend.decision = ReconciliationDecision::Approved {
            charged_amount: expected,
            reason: FareApprovalReason::StandardFare,
        };
        backend.final_fare = expected;

        let first = reconcile_fare_evidence(reader, backend);

        let second = reconcile_fare_evidence(reader, backend);

        assert_eq!(first, second);
    }

    #[test]
    fn all_phase_eight_outcomes_are_defined() {
        let outcomes = [
            ReconciliationOutcome::Matched,
            ReconciliationOutcome::FareAmountMismatch,
            ReconciliationOutcome::PolicyVersionMismatch,
            ReconciliationOutcome::EligibilityMismatch,
            ReconciliationOutcome::ProductMismatch,
            ReconciliationOutcome::TransferMismatch,
            ReconciliationOutcome::FareCapMismatch,
            ReconciliationOutcome::DuplicateTransaction,
            ReconciliationOutcome::MissingBackendContext,
            ReconciliationOutcome::InvalidEvidence,
            ReconciliationOutcome::ManualReviewRequired,
        ];

        assert_eq!(outcomes.len(), 11);
    }
}
