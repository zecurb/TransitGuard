use core::{cmp::Ordering, fmt};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
    FarePolicyVersion, FareRejectionReason, Money,
};

use crate::{DiscountBasisPoints, FarePolicy, ZoneId};

/// The calculation stage that encountered an arithmetic failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FareCalculationStage {
    /// Multiplying the configured surcharge by the number of additional zones.
    ZoneSurcharge,

    /// Adding the base fare and total zone surcharge.
    BaseAndZoneFare,

    /// Calculating the eligibility discount.
    EligibilityDiscount,

    /// Subtracting the eligibility discount from the calculated fare.
    DiscountedFare,
}

impl fmt::Display for FareCalculationStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self {
            Self::ZoneSurcharge => "zone surcharge",
            Self::BaseAndZoneFare => "base and zone fare",
            Self::EligibilityDiscount => "eligibility discount",
            Self::DiscountedFare => "discounted fare",
        };

        formatter.write_str(stage)
    }
}

/// Errors produced during deterministic fare evaluation.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareEvaluationError {
    /// The available balance used a currency different from the policy.
    #[error("available balance uses {actual}, expected policy currency {expected}")]
    BalanceCurrencyMismatch {
        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the balance.
        actual: Currency,
    },

    /// Available stored value cannot be negative.
    #[error("available balance cannot be negative: {balance}")]
    NegativeAvailableBalance {
        /// Invalid available balance.
        balance: Money,
    },

    /// A fare calculation exceeded the supported integer range.
    #[error("fare calculation overflowed during {stage}")]
    ArithmeticOverflow {
        /// Calculation stage that failed.
        stage: FareCalculationStage,
    },
}

/// Immutable inputs required for the first-stage fare calculation.
///
/// Current event time is supplied explicitly so evaluation never reads the
/// system clock.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct FareEvaluationInput {
    /// Time at which the simulated fare presentation occurred.
    pub event_time: EventTime,

    /// Zone where the journey began.
    pub origin_zone: ZoneId,

    /// Zone where the journey ended.
    pub destination_zone: ZoneId,

    /// Rider eligibility used for discount selection.
    pub eligibility: EligibilityClassification,

    /// Stored value available before processing the fare.
    pub available_balance: Money,
}

/// Approved or rejected result of deterministic fare evaluation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum FareEvaluationOutcome {
    /// The fare was approved.
    Approved {
        /// Amount that should be charged.
        charged_amount: Money,

        /// Stable approval classification.
        reason: FareApprovalReason,
    },

    /// The fare was rejected.
    Rejected {
        /// Stable rejection classification.
        reason: FareRejectionReason,
    },
}

/// Evidence explaining how the final fare was calculated.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct FareDecisionEvidence {
    base_fare: Money,
    additional_zone_count: u16,
    zone_surcharge: Money,
    fare_before_discount: Money,
    eligibility: EligibilityClassification,
    discount_basis_points: DiscountBasisPoints,
    eligibility_discount: Money,
    final_fare: Money,
}

impl FareDecisionEvidence {
    /// Returns the policy base fare.
    #[must_use]
    pub const fn base_fare(self) -> Money {
        self.base_fare
    }

    /// Returns the number of zones beyond the origin zone.
    #[must_use]
    pub const fn additional_zone_count(self) -> u16 {
        self.additional_zone_count
    }

    /// Returns the total zone surcharge.
    #[must_use]
    pub const fn zone_surcharge(self) -> Money {
        self.zone_surcharge
    }

    /// Returns the fare before eligibility discounting.
    #[must_use]
    pub const fn fare_before_discount(self) -> Money {
        self.fare_before_discount
    }

    /// Returns the rider eligibility used for the decision.
    #[must_use]
    pub const fn eligibility(self) -> EligibilityClassification {
        self.eligibility
    }

    /// Returns the eligibility discount percentage.
    #[must_use]
    pub const fn discount_basis_points(self) -> DiscountBasisPoints {
        self.discount_basis_points
    }

    /// Returns the monetary eligibility discount.
    #[must_use]
    pub const fn eligibility_discount(self) -> Money {
        self.eligibility_discount
    }

    /// Returns the final fare presented to the balance check.
    #[must_use]
    pub const fn final_fare(self) -> Money {
        self.final_fare
    }
}

/// Complete deterministic fare-engine result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct FareEvaluation {
    policy_id: FarePolicyId,
    policy_version: FarePolicyVersion,
    event_time: EventTime,
    outcome: FareEvaluationOutcome,
    evidence: FareDecisionEvidence,
}

impl FareEvaluation {
    /// Returns the policy identity used for evaluation.
    #[must_use]
    pub const fn policy_id(self) -> FarePolicyId {
        self.policy_id
    }

    /// Returns the immutable policy version used for evaluation.
    #[must_use]
    pub const fn policy_version(self) -> FarePolicyVersion {
        self.policy_version
    }

    /// Returns the event time supplied to the evaluator.
    #[must_use]
    pub const fn event_time(self) -> EventTime {
        self.event_time
    }

    /// Returns the approved or rejected outcome.
    #[must_use]
    pub const fn outcome(self) -> FareEvaluationOutcome {
        self.outcome
    }

    /// Returns the complete calculation evidence.
    #[must_use]
    pub const fn evidence(self) -> FareDecisionEvidence {
        self.evidence
    }
}

/// Evaluates base fare, zone surcharge, eligibility discount, and balance.
///
/// Rule order:
///
/// 1. Validate the balance currency and amount.
/// 2. Add the base fare.
/// 3. Add one surcharge for each zone beyond the origin zone.
/// 4. Apply the eligibility discount.
/// 5. Compare the resulting fare with the available balance.
///
/// Percentage discounts round down to the nearest minor unit.
pub fn evaluate_fare(
    policy: FarePolicy,
    input: FareEvaluationInput,
) -> Result<FareEvaluation, FareEvaluationError> {
    validate_balance(policy, input.available_balance)?;

    let additional_zone_count = input
        .origin_zone
        .value()
        .abs_diff(input.destination_zone.value());

    let zone_surcharge = calculate_zone_surcharge(policy.zone_surcharge(), additional_zone_count)?;

    let fare_before_discount = policy
        .base_fare()
        .checked_add(zone_surcharge)
        .map_err(|_| FareEvaluationError::ArithmeticOverflow {
            stage: FareCalculationStage::BaseAndZoneFare,
        })?;

    let discount_basis_points = policy.eligibility_discounts().rate_for(input.eligibility);

    let eligibility_discount = calculate_percentage(fare_before_discount, discount_basis_points)?;

    let final_fare = fare_before_discount
        .checked_subtract(eligibility_discount)
        .map_err(|_| FareEvaluationError::ArithmeticOverflow {
            stage: FareCalculationStage::DiscountedFare,
        })?;

    let balance_comparison = input
        .available_balance
        .checked_cmp(final_fare)
        .map_err(|_| FareEvaluationError::BalanceCurrencyMismatch {
            expected: policy.currency(),
            actual: input.available_balance.currency(),
        })?;

    let outcome = match balance_comparison {
        Ordering::Less => FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::InsufficientStoredValue,
        },
        Ordering::Equal | Ordering::Greater => FareEvaluationOutcome::Approved {
            charged_amount: final_fare,
            reason: FareApprovalReason::StandardFare,
        },
    };

    let evidence = FareDecisionEvidence {
        base_fare: policy.base_fare(),
        additional_zone_count,
        zone_surcharge,
        fare_before_discount,
        eligibility: input.eligibility,
        discount_basis_points,
        eligibility_discount,
        final_fare,
    };

    Ok(FareEvaluation {
        policy_id: policy.id(),
        policy_version: policy.version(),
        event_time: input.event_time,
        outcome,
        evidence,
    })
}

fn validate_balance(policy: FarePolicy, balance: Money) -> Result<(), FareEvaluationError> {
    if balance.currency() != policy.currency() {
        return Err(FareEvaluationError::BalanceCurrencyMismatch {
            expected: policy.currency(),
            actual: balance.currency(),
        });
    }

    if balance.is_negative() {
        return Err(FareEvaluationError::NegativeAvailableBalance { balance });
    }

    Ok(())
}

fn calculate_zone_surcharge(
    surcharge_per_zone: Money,
    additional_zone_count: u16,
) -> Result<Money, FareEvaluationError> {
    let minor_units = surcharge_per_zone
        .minor_units()
        .checked_mul(i64::from(additional_zone_count))
        .ok_or(FareEvaluationError::ArithmeticOverflow {
            stage: FareCalculationStage::ZoneSurcharge,
        })?;

    Ok(Money::from_minor_units(
        minor_units,
        surcharge_per_zone.currency(),
    ))
}

fn calculate_percentage(
    amount: Money,
    rate: DiscountBasisPoints,
) -> Result<Money, FareEvaluationError> {
    let numerator = i128::from(amount.minor_units()) * i128::from(rate.value());

    let discounted_minor_units = numerator / 10_000;

    let minor_units = i64::try_from(discounted_minor_units).map_err(|_| {
        FareEvaluationError::ArithmeticOverflow {
            stage: FareCalculationStage::EligibilityDiscount,
        }
    })?;

    Ok(Money::from_minor_units(minor_units, amount.currency()))
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
        FarePolicyVersion, FareRejectionReason, Money,
    };

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition,
        TransferWindow, ZoneId,
    };

    use super::{FareEvaluationError, FareEvaluationInput, FareEvaluationOutcome, evaluate_fare};

    fn policy_version() -> FarePolicyVersion {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("version one must be valid");
        };

        version
    }

    fn event_time() -> EventTime {
        let Ok(time) = EventTime::from_unix_milliseconds(1_700_000_000_000) else {
            panic!("positive event time must be valid");
        };

        time
    }

    fn zone(value: u16) -> ZoneId {
        let Ok(zone) = ZoneId::new(value) else {
            panic!("positive zone identifier must be valid");
        };

        zone
    }

    fn discount(value: u16) -> DiscountBasisPoints {
        let Ok(discount) = DiscountBasisPoints::new(value) else {
            panic!("test discount must be valid");
        };

        discount
    }

    fn transfer_window() -> TransferWindow {
        let Ok(window) = TransferWindow::from_milliseconds(90 * 60 * 1_000) else {
            panic!("positive transfer window must be valid");
        };

        window
    }

    fn policy(base_fare: i64, zone_surcharge: i64, youth_discount: u16) -> FarePolicy {
        let definition = FarePolicyDefinition {
            id: FarePolicyId::generate(),
            version: policy_version(),
            currency: Currency::Usd,
            base_fare: Money::from_minor_units(base_fare, Currency::Usd),
            zone_surcharge: Money::from_minor_units(zone_surcharge, Currency::Usd),
            transfer_window: transfer_window(),
            transfer_discount: Money::from_minor_units(base_fare, Currency::Usd),
            daily_cap: Money::from_minor_units(2_000, Currency::Usd),
            weekly_cap: Money::from_minor_units(8_000, Currency::Usd),
            eligibility_discounts: EligibilityDiscounts::new(
                discount(youth_discount),
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::ZERO,
                DiscountBasisPoints::FULL_FARE,
            ),
        };

        let Ok(policy) = FarePolicy::validate(definition) else {
            panic!("test policy must be valid");
        };

        policy
    }

    fn input(
        balance: Money,
        origin_zone: u16,
        destination_zone: u16,
        eligibility: EligibilityClassification,
    ) -> FareEvaluationInput {
        FareEvaluationInput {
            event_time: event_time(),
            origin_zone: zone(origin_zone),
            destination_zone: zone(destination_zone),
            eligibility,
            available_balance: balance,
        }
    }

    #[test]
    fn same_zone_standard_fare_is_approved() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(1_000, Currency::Usd),
                1,
                1,
                EligibilityClassification::Standard,
            ),
        );

        let Ok(evaluation) = result else {
            panic!("valid fare must evaluate");
        };

        assert_eq!(
            evaluation.outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(250, Currency::Usd),
                reason: FareApprovalReason::StandardFare,
            }
        );

        assert_eq!(evaluation.evidence().additional_zone_count(), 0);

        assert_eq!(
            evaluation.evidence().zone_surcharge(),
            Money::zero(Currency::Usd)
        );
    }

    #[test]
    fn additional_zones_add_configured_surcharges() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(1_000, Currency::Usd),
                1,
                3,
                EligibilityClassification::Standard,
            ),
        );

        let Ok(evaluation) = result else {
            panic!("valid zone fare must evaluate");
        };

        assert_eq!(evaluation.evidence().additional_zone_count(), 2);

        assert_eq!(
            evaluation.evidence().zone_surcharge(),
            Money::from_minor_units(150, Currency::Usd)
        );

        assert_eq!(
            evaluation.evidence().final_fare(),
            Money::from_minor_units(400, Currency::Usd)
        );
    }

    #[test]
    fn eligibility_discount_applies_after_zone_pricing() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(1_000, Currency::Usd),
                1,
                3,
                EligibilityClassification::Youth,
            ),
        );

        let Ok(evaluation) = result else {
            panic!("discounted fare must evaluate");
        };

        assert_eq!(
            evaluation.evidence().fare_before_discount(),
            Money::from_minor_units(400, Currency::Usd)
        );

        assert_eq!(
            evaluation.evidence().eligibility_discount(),
            Money::from_minor_units(200, Currency::Usd)
        );

        assert_eq!(
            evaluation.evidence().final_fare(),
            Money::from_minor_units(200, Currency::Usd)
        );
    }

    #[test]
    fn percentage_discount_rounds_down_to_minor_unit() {
        let policy = policy(255, 0, 3_333);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(1_000, Currency::Usd),
                1,
                1,
                EligibilityClassification::Youth,
            ),
        );

        let Ok(evaluation) = result else {
            panic!("rounded fare must evaluate");
        };

        assert_eq!(
            evaluation.evidence().eligibility_discount(),
            Money::from_minor_units(84, Currency::Usd)
        );

        assert_eq!(
            evaluation.evidence().final_fare(),
            Money::from_minor_units(171, Currency::Usd)
        );
    }

    #[test]
    fn insufficient_balance_rejects_without_partial_charge() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(249, Currency::Usd),
                1,
                1,
                EligibilityClassification::Standard,
            ),
        );

        let Ok(evaluation) = result else {
            panic!("insufficient balance must produce a decision");
        };

        assert_eq!(
            evaluation.outcome(),
            FareEvaluationOutcome::Rejected {
                reason: FareRejectionReason::InsufficientStoredValue,
            }
        );

        assert_eq!(
            evaluation.evidence().final_fare(),
            Money::from_minor_units(250, Currency::Usd)
        );
    }

    #[test]
    fn cross_currency_balance_is_rejected_as_invalid_input() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(1_000, Currency::Eur),
                1,
                1,
                EligibilityClassification::Standard,
            ),
        );

        assert!(matches!(
            result,
            Err(FareEvaluationError::BalanceCurrencyMismatch {
                expected: Currency::Usd,
                actual: Currency::Eur
            })
        ));
    }

    #[test]
    fn negative_available_balance_is_rejected() {
        let policy = policy(250, 75, 5_000);

        let result = evaluate_fare(
            policy,
            input(
                Money::from_minor_units(-1, Currency::Usd),
                1,
                1,
                EligibilityClassification::Standard,
            ),
        );

        assert!(matches!(
            result,
            Err(FareEvaluationError::NegativeAvailableBalance { .. })
        ));
    }

    #[test]
    fn identical_inputs_produce_identical_results() {
        let policy = policy(250, 75, 5_000);

        let input = input(
            Money::from_minor_units(1_000, Currency::Usd),
            1,
            3,
            EligibilityClassification::Youth,
        );

        let first = evaluate_fare(policy, input);
        let second = evaluate_fare(policy, input);

        assert_eq!(first, second);
    }
}
