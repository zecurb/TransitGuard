use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{Currency, FarePolicyId, FarePolicyVersion, Money};

use crate::FarePolicy;

/// Fare amounts already charged during the current service periods.
///
/// The caller determines the applicable service day and service week and
/// supplies the accumulated amounts explicitly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct FareCapHistory {
    daily_charged: Money,
    weekly_charged: Money,
}

impl FareCapHistory {
    /// Creates fare-cap history.
    #[must_use]
    pub const fn new(daily_charged: Money, weekly_charged: Money) -> Self {
        Self {
            daily_charged,
            weekly_charged,
        }
    }

    /// Creates empty fare-cap history for a currency.
    #[must_use]
    pub const fn zero(currency: Currency) -> Self {
        Self::new(Money::zero(currency), Money::zero(currency))
    }

    /// Returns the amount already charged today.
    #[must_use]
    pub const fn daily_charged(self) -> Money {
        self.daily_charged
    }

    /// Returns the amount already charged this week.
    #[must_use]
    pub const fn weekly_charged(self) -> Money {
        self.weekly_charged
    }
}

/// Errors produced while applying fare caps.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareCapEvaluationError {
    /// The fare used a currency different from the policy.
    #[error("fare-cap input `{field}` uses {actual}, expected policy currency {expected}")]
    CurrencyMismatch {
        /// Invalid input field.
        field: &'static str,

        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the input.
        actual: Currency,
    },

    /// A fare-cap input contained a negative amount.
    #[error("fare-cap input `{field}` cannot be negative: {amount}")]
    NegativeAmount {
        /// Invalid input field.
        field: &'static str,

        /// Invalid amount.
        amount: Money,
    },

    /// Daily charges cannot exceed the total weekly charges.
    #[error(
        "daily charged amount {daily_charged} cannot exceed weekly charged amount {weekly_charged}"
    )]
    DailyChargedExceedsWeeklyCharged {
        /// Amount already charged today.
        daily_charged: Money,

        /// Amount already charged this week.
        weekly_charged: Money,
    },

    /// A validated fare-cap subtraction unexpectedly failed.
    #[error("fare-cap calculation failed")]
    ArithmeticFailure,
}

/// Result of applying daily and weekly fare caps.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct FareCapApplication {
    policy_id: FarePolicyId,
    policy_version: FarePolicyVersion,
    daily_remaining_before: Money,
    weekly_remaining_before: Money,
    discount_applied: Money,
    fare_after_caps: Money,
    daily_cap_reached: bool,
    weekly_cap_reached: bool,
}

impl FareCapApplication {
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

    /// Returns the remaining daily allowance before this fare.
    #[must_use]
    pub const fn daily_remaining_before(self) -> Money {
        self.daily_remaining_before
    }

    /// Returns the remaining weekly allowance before this fare.
    #[must_use]
    pub const fn weekly_remaining_before(self) -> Money {
        self.weekly_remaining_before
    }

    /// Returns the reduction caused by the caps.
    #[must_use]
    pub const fn discount_applied(self) -> Money {
        self.discount_applied
    }

    /// Returns the fare after applying both caps.
    #[must_use]
    pub const fn fare_after_caps(self) -> Money {
        self.fare_after_caps
    }

    /// Reports whether this result reaches or preserves the daily cap.
    #[must_use]
    pub const fn daily_cap_reached(self) -> bool {
        self.daily_cap_reached
    }

    /// Reports whether this result reaches or preserves the weekly cap.
    #[must_use]
    pub const fn weekly_cap_reached(self) -> bool {
        self.weekly_cap_reached
    }
}

/// Applies daily and weekly fare caps.
///
/// The resulting fare is the smallest of:
///
/// - the fare after transfer processing;
/// - the amount remaining under the daily cap;
/// - the amount remaining under the weekly cap.
///
/// A cap can reduce the fare to zero but can never produce a negative charge.
pub fn apply_fare_caps(
    policy: FarePolicy,
    history: FareCapHistory,
    fare_after_transfer: Money,
) -> Result<FareCapApplication, FareCapEvaluationError> {
    validate_amount(policy, "daily_charged", history.daily_charged())?;

    validate_amount(policy, "weekly_charged", history.weekly_charged())?;

    validate_amount(policy, "fare_after_transfer", fare_after_transfer)?;

    if history.daily_charged().minor_units() > history.weekly_charged().minor_units() {
        return Err(FareCapEvaluationError::DailyChargedExceedsWeeklyCharged {
            daily_charged: history.daily_charged(),
            weekly_charged: history.weekly_charged(),
        });
    }

    let daily_remaining_before = remaining_allowance(policy.daily_cap(), history.daily_charged())?;

    let weekly_remaining_before =
        remaining_allowance(policy.weekly_cap(), history.weekly_charged())?;

    let fare_minor_units = fare_after_transfer
        .minor_units()
        .min(daily_remaining_before.minor_units())
        .min(weekly_remaining_before.minor_units());

    let fare_after_caps = Money::from_minor_units(fare_minor_units, policy.currency());

    let discount_applied = fare_after_transfer
        .checked_subtract(fare_after_caps)
        .map_err(|_| FareCapEvaluationError::ArithmeticFailure)?;

    let daily_cap_reached = history.daily_charged().minor_units() + fare_after_caps.minor_units()
        >= policy.daily_cap().minor_units();

    let weekly_cap_reached = history.weekly_charged().minor_units() + fare_after_caps.minor_units()
        >= policy.weekly_cap().minor_units();

    Ok(FareCapApplication {
        policy_id: policy.id(),
        policy_version: policy.version(),
        daily_remaining_before,
        weekly_remaining_before,
        discount_applied,
        fare_after_caps,
        daily_cap_reached,
        weekly_cap_reached,
    })
}

fn validate_amount(
    policy: FarePolicy,
    field: &'static str,
    amount: Money,
) -> Result<(), FareCapEvaluationError> {
    if amount.currency() != policy.currency() {
        return Err(FareCapEvaluationError::CurrencyMismatch {
            field,
            expected: policy.currency(),
            actual: amount.currency(),
        });
    }

    if amount.is_negative() {
        return Err(FareCapEvaluationError::NegativeAmount { field, amount });
    }

    Ok(())
}

fn remaining_allowance(cap: Money, charged: Money) -> Result<Money, FareCapEvaluationError> {
    if charged.minor_units() >= cap.minor_units() {
        return Ok(Money::zero(cap.currency()));
    }

    cap.checked_subtract(charged)
        .map_err(|_| FareCapEvaluationError::ArithmeticFailure)
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{Currency, FarePolicyId, FarePolicyVersion, Money};

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition, TransferWindow,
    };

    use super::{FareCapEvaluationError, FareCapHistory, apply_fare_caps};

    fn policy_version() -> FarePolicyVersion {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("version one must be valid");
        };

        version
    }

    fn transfer_window() -> TransferWindow {
        let Ok(window) = TransferWindow::from_milliseconds(5_400_000) else {
            panic!("positive transfer window must be valid");
        };

        window
    }

    fn policy() -> FarePolicy {
        let definition = FarePolicyDefinition {
            id: FarePolicyId::generate(),
            version: policy_version(),
            currency: Currency::Usd,
            base_fare: Money::from_minor_units(250, Currency::Usd),
            zone_surcharge: Money::from_minor_units(75, Currency::Usd),
            transfer_window: transfer_window(),
            transfer_discount: Money::from_minor_units(250, Currency::Usd),
            daily_cap: Money::from_minor_units(750, Currency::Usd),
            weekly_cap: Money::from_minor_units(3_000, Currency::Usd),
            eligibility_discounts: EligibilityDiscounts::new(
                DiscountBasisPoints::ZERO,
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

    #[test]
    fn empty_history_does_not_reduce_fare() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::zero(Currency::Usd),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("valid cap evaluation must succeed");
        };

        assert_eq!(
            application.fare_after_caps(),
            Money::from_minor_units(250, Currency::Usd)
        );

        assert_eq!(application.discount_applied(), Money::zero(Currency::Usd));
    }

    #[test]
    fn daily_cap_can_partially_reduce_fare() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(650, Currency::Usd),
                Money::from_minor_units(1_400, Currency::Usd),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("partial cap must evaluate");
        };

        assert_eq!(
            application.fare_after_caps(),
            Money::from_minor_units(100, Currency::Usd)
        );

        assert_eq!(
            application.discount_applied(),
            Money::from_minor_units(150, Currency::Usd)
        );

        assert!(application.daily_cap_reached());
    }

    #[test]
    fn reached_daily_cap_produces_zero_fare() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(750, Currency::Usd),
                Money::from_minor_units(1_500, Currency::Usd),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("reached cap must evaluate");
        };

        assert_eq!(application.fare_after_caps(), Money::zero(Currency::Usd));

        assert_eq!(
            application.discount_applied(),
            Money::from_minor_units(250, Currency::Usd)
        );

        assert!(application.daily_cap_reached());
    }

    #[test]
    fn weekly_cap_can_be_the_tighter_limit() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(500, Currency::Usd),
                Money::from_minor_units(2_950, Currency::Usd),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("weekly cap must evaluate");
        };

        assert_eq!(
            application.fare_after_caps(),
            Money::from_minor_units(50, Currency::Usd)
        );

        assert!(application.weekly_cap_reached());
    }

    #[test]
    fn daily_charges_cannot_exceed_weekly_charges() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(500, Currency::Usd),
                Money::from_minor_units(400, Currency::Usd),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        assert!(matches!(
            result,
            Err(FareCapEvaluationError::DailyChargedExceedsWeeklyCharged { .. })
        ));
    }

    #[test]
    fn cross_currency_history_is_rejected() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(100, Currency::Eur),
                Money::from_minor_units(100, Currency::Eur),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        assert!(matches!(
            result,
            Err(FareCapEvaluationError::CurrencyMismatch {
                field: "daily_charged",
                expected: Currency::Usd,
                actual: Currency::Eur
            })
        ));
    }

    #[test]
    fn negative_history_is_rejected() {
        let result = apply_fare_caps(
            policy(),
            FareCapHistory::new(
                Money::from_minor_units(-1, Currency::Usd),
                Money::zero(Currency::Usd),
            ),
            Money::from_minor_units(250, Currency::Usd),
        );

        assert!(matches!(
            result,
            Err(FareCapEvaluationError::NegativeAmount {
                field: "daily_charged",
                ..
            })
        ));
    }
}
