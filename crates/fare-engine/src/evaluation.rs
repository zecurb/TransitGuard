use core::{cmp::Ordering, fmt};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{
    Currency, EligibilityClassification, EventTime, FareApprovalReason, FarePolicyId,
    FarePolicyVersion, FareRejectionReason, Money,
};

use crate::{
    DiscountBasisPoints, FareCapEvaluationError, FareCapHistory, FarePolicy,
    ProductApplicationOutcome, ProductEvaluationError, TransferEvaluationError, TransferHistory,
    TransitProduct, ZoneId, apply_fare_caps, apply_transfer, apply_transit_product,
};

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

    /// Transfer history or transfer calculation was invalid.
    #[error(transparent)]
    Transfer(#[from] TransferEvaluationError),

    /// Fare-cap history or calculation was invalid.
    #[error(transparent)]
    FareCap(#[from] FareCapEvaluationError),

    /// Transit-product application failed.
    #[error(transparent)]
    Product(#[from] ProductEvaluationError),

    /// A fare calculation exceeded the supported integer range.
    #[error("fare calculation overflowed during {stage}")]
    ArithmeticOverflow {
        /// Calculation stage that failed.
        stage: FareCalculationStage,
    },
}

/// Complete immutable input for deterministic fare evaluation.
///
/// Every changing value is supplied explicitly. The evaluator does not read
/// the clock, environment, database, network, or mutable global state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct FareEvaluationInput {
    /// Time at which the fictional fare presentation occurred.
    pub event_time: EventTime,

    /// Zone where the journey began.
    pub origin_zone: ZoneId,

    /// Zone where the journey ended.
    pub destination_zone: ZoneId,

    /// Rider eligibility used for discount selection.
    pub eligibility: EligibilityClassification,

    /// Previous paid-fare history used for transfer evaluation.
    pub transfer_history: TransferHistory,

    /// Daily and weekly accumulated charges used for fare caps.
    pub fare_cap_history: FareCapHistory,

    /// Optional fictional transit product presented for this journey.
    pub transit_product: Option<TransitProduct>,

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

/// Evidence explaining every stage of the fare calculation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct FareDecisionEvidence {
    base_fare: Money,
    additional_zone_count: u16,
    zone_surcharge: Money,
    fare_before_discount: Money,
    eligibility: EligibilityClassification,
    discount_basis_points: DiscountBasisPoints,
    eligibility_discount: Money,
    fare_after_eligibility: Money,
    transfer_eligible: bool,
    transfer_discount: Money,
    fare_after_transfer: Money,
    fare_cap_discount: Money,
    fare_after_caps: Money,
    daily_cap_reached: bool,
    weekly_cap_reached: bool,
    product_outcome: ProductApplicationOutcome,
    product_discount: Money,
    fare_after_product: Money,
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

    /// Returns the rider eligibility used for evaluation.
    #[must_use]
    pub const fn eligibility(self) -> EligibilityClassification {
        self.eligibility
    }

    /// Returns the configured eligibility discount.
    #[must_use]
    pub const fn discount_basis_points(self) -> DiscountBasisPoints {
        self.discount_basis_points
    }

    /// Returns the monetary eligibility discount.
    #[must_use]
    pub const fn eligibility_discount(self) -> Money {
        self.eligibility_discount
    }

    /// Returns the fare after eligibility discounting.
    #[must_use]
    pub const fn fare_after_eligibility(self) -> Money {
        self.fare_after_eligibility
    }

    /// Reports whether the presentation qualified for transfer.
    #[must_use]
    pub const fn transfer_eligible(self) -> bool {
        self.transfer_eligible
    }

    /// Returns the transfer discount actually applied.
    #[must_use]
    pub const fn transfer_discount(self) -> Money {
        self.transfer_discount
    }

    /// Returns the fare after transfer processing.
    #[must_use]
    pub const fn fare_after_transfer(self) -> Money {
        self.fare_after_transfer
    }

    /// Returns the reduction caused by daily or weekly caps.
    #[must_use]
    pub const fn fare_cap_discount(self) -> Money {
        self.fare_cap_discount
    }

    /// Returns the fare after daily and weekly caps.
    #[must_use]
    pub const fn fare_after_caps(self) -> Money {
        self.fare_after_caps
    }

    /// Reports whether the daily cap has been reached.
    #[must_use]
    pub const fn daily_cap_reached(self) -> bool {
        self.daily_cap_reached
    }

    /// Reports whether the weekly cap has been reached.
    #[must_use]
    pub const fn weekly_cap_reached(self) -> bool {
        self.weekly_cap_reached
    }

    /// Returns the transit-product validation outcome.
    #[must_use]
    pub const fn product_outcome(self) -> ProductApplicationOutcome {
        self.product_outcome
    }

    /// Returns the amount covered by the transit product.
    #[must_use]
    pub const fn product_discount(self) -> Money {
        self.product_discount
    }

    /// Returns the fare after transit-product application.
    #[must_use]
    pub const fn fare_after_product(self) -> Money {
        self.fare_after_product
    }

    /// Returns the final fare used for the decision.
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

    /// Returns the approved or rejected result.
    #[must_use]
    pub const fn outcome(self) -> FareEvaluationOutcome {
        self.outcome
    }

    /// Returns the complete decision evidence.
    #[must_use]
    pub const fn evidence(self) -> FareDecisionEvidence {
        self.evidence
    }
}

/// Evaluates a complete TransitGuard fare decision.
///
/// Rule order:
///
/// 1. Validate available stored value.
/// 2. Calculate the base and zone fare.
/// 3. Apply the eligibility discount.
/// 4. Apply an eligible transfer benefit.
/// 5. Apply daily and weekly fare caps.
/// 6. Validate and apply a transit product.
/// 7. Check stored value when no product covers the fare.
///
/// Identical policy and input values always produce identical results.
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

    let fare_after_eligibility = fare_before_discount
        .checked_subtract(eligibility_discount)
        .map_err(|_| FareEvaluationError::ArithmeticOverflow {
            stage: FareCalculationStage::DiscountedFare,
        })?;

    let transfer_application = apply_transfer(
        policy,
        input.event_time,
        input.transfer_history,
        fare_after_eligibility,
    )?;

    let fare_cap_application = apply_fare_caps(
        policy,
        input.fare_cap_history,
        transfer_application.fare_after_transfer(),
    )?;

    let product_application = apply_transit_product(
        policy,
        input.transit_product,
        input.event_time,
        input.origin_zone,
        input.destination_zone,
        fare_cap_application.fare_after_caps(),
    )?;

    let final_fare = product_application.fare_after_product();

    let stored_value_reason = determine_stored_value_reason(
        transfer_application.discount_applied(),
        fare_cap_application.discount_applied(),
    );

    let outcome = match product_application.outcome() {
        ProductApplicationOutcome::Covered => FareEvaluationOutcome::Approved {
            charged_amount: Money::zero(policy.currency()),
            reason: FareApprovalReason::TransitProduct,
        },

        ProductApplicationOutcome::Invalid { .. } => FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::ProductInvalid,
        },

        ProductApplicationOutcome::NotPresented => {
            evaluate_stored_value(input.available_balance, final_fare, stored_value_reason)?
        }
    };

    let evidence = FareDecisionEvidence {
        base_fare: policy.base_fare(),
        additional_zone_count,
        zone_surcharge,
        fare_before_discount,
        eligibility: input.eligibility,
        discount_basis_points,
        eligibility_discount,
        fare_after_eligibility,
        transfer_eligible: transfer_application.eligible(),
        transfer_discount: transfer_application.discount_applied(),
        fare_after_transfer: transfer_application.fare_after_transfer(),
        fare_cap_discount: fare_cap_application.discount_applied(),
        fare_after_caps: fare_cap_application.fare_after_caps(),
        daily_cap_reached: fare_cap_application.daily_cap_reached(),
        weekly_cap_reached: fare_cap_application.weekly_cap_reached(),
        product_outcome: product_application.outcome(),
        product_discount: product_application.discount_applied(),
        fare_after_product: product_application.fare_after_product(),
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

fn determine_stored_value_reason(
    transfer_discount: Money,
    fare_cap_discount: Money,
) -> FareApprovalReason {
    if fare_cap_discount.minor_units() > 0 {
        FareApprovalReason::FareCap
    } else if transfer_discount.minor_units() > 0 {
        FareApprovalReason::Transfer
    } else {
        FareApprovalReason::StandardFare
    }
}

fn evaluate_stored_value(
    available_balance: Money,
    final_fare: Money,
    approval_reason: FareApprovalReason,
) -> Result<FareEvaluationOutcome, FareEvaluationError> {
    let comparison = available_balance.checked_cmp(final_fare).map_err(|_| {
        FareEvaluationError::BalanceCurrencyMismatch {
            expected: final_fare.currency(),
            actual: available_balance.currency(),
        }
    })?;

    Ok(match comparison {
        Ordering::Less => FareEvaluationOutcome::Rejected {
            reason: FareRejectionReason::InsufficientStoredValue,
        },

        Ordering::Equal | Ordering::Greater => FareEvaluationOutcome::Approved {
            charged_amount: final_fare,
            reason: approval_reason,
        },
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
        FarePolicyVersion, Money,
    };

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FareCapHistory, FarePolicy,
        FarePolicyDefinition, TransferHistory, TransferWindow, ZoneId,
    };

    use super::{FareEvaluationInput, FareEvaluationOutcome, evaluate_fare};

    fn event_time(milliseconds: i64) -> EventTime {
        let Ok(time) = EventTime::from_unix_milliseconds(milliseconds) else {
            panic!("test event time must be valid");
        };

        time
    }

    fn zone(value: u16) -> ZoneId {
        let Ok(zone) = ZoneId::new(value) else {
            panic!("positive zone must be valid");
        };

        zone
    }

    fn policy() -> FarePolicy {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("version one must be valid");
        };

        let Ok(window) = TransferWindow::from_milliseconds(5_400_000) else {
            panic!("positive transfer window must be valid");
        };

        let Ok(youth_discount) = DiscountBasisPoints::new(5_000) else {
            panic!("fifty percent must be valid");
        };

        let definition = FarePolicyDefinition {
            id: FarePolicyId::generate(),
            version,
            currency: Currency::Usd,
            base_fare: Money::from_minor_units(250, Currency::Usd),
            zone_surcharge: Money::from_minor_units(75, Currency::Usd),
            transfer_window: window,
            transfer_discount: Money::from_minor_units(250, Currency::Usd),
            daily_cap: Money::from_minor_units(750, Currency::Usd),
            weekly_cap: Money::from_minor_units(3_000, Currency::Usd),
            eligibility_discounts: EligibilityDiscounts::new(
                youth_discount,
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

    fn input() -> FareEvaluationInput {
        FareEvaluationInput {
            event_time: event_time(10_000_000),
            origin_zone: zone(1),
            destination_zone: zone(3),
            eligibility: EligibilityClassification::Standard,
            transfer_history: TransferHistory::none(),
            fare_cap_history: FareCapHistory::zero(Currency::Usd),
            transit_product: None,
            available_balance: Money::from_minor_units(1_000, Currency::Usd),
        }
    }

    #[test]
    fn standard_zone_fare_is_approved() {
        let result = evaluate_fare(policy(), input());

        let Ok(evaluation) = result else {
            panic!("standard fare must evaluate");
        };

        assert_eq!(
            evaluation.outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(400, Currency::Usd,),
                reason: FareApprovalReason::StandardFare,
            }
        );
    }

    #[test]
    fn transfer_is_applied_before_fare_caps() {
        let mut fare_input = input();

        fare_input.transfer_history =
            TransferHistory::from_previous_paid_fare(event_time(9_000_000));

        fare_input.fare_cap_history = FareCapHistory::new(
            Money::from_minor_units(700, Currency::Usd),
            Money::from_minor_units(1_000, Currency::Usd),
        );

        let result = evaluate_fare(policy(), fare_input);

        let Ok(evaluation) = result else {
            panic!("combined fare must evaluate");
        };

        assert_eq!(
            evaluation.evidence().fare_after_transfer(),
            Money::from_minor_units(150, Currency::Usd)
        );

        assert_eq!(
            evaluation.evidence().fare_after_caps(),
            Money::from_minor_units(50, Currency::Usd)
        );

        assert_eq!(
            evaluation.outcome(),
            FareEvaluationOutcome::Approved {
                charged_amount: Money::from_minor_units(50, Currency::Usd,),
                reason: FareApprovalReason::FareCap,
            }
        );
    }

    #[test]
    fn identical_inputs_produce_identical_results() {
        let policy = policy();
        let input = input();

        let first = evaluate_fare(policy, input);
        let second = evaluate_fare(policy, input);

        assert_eq!(first, second);
    }
}
