use serde::{Deserialize, Serialize};
use thiserror::Error;
use transitguard_domain::{Currency, EventTime, FarePolicyId, FarePolicyVersion, Money};

use crate::FarePolicy;

/// Previous paid-fare information available during evaluation.
///
/// TransitGuard does not query transaction history from inside the fare
/// engine. The caller provides the relevant history explicitly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TransferHistory {
    previous_paid_fare_event_time: Option<EventTime>,
}

impl TransferHistory {
    /// Creates an empty transfer history.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            previous_paid_fare_event_time: None,
        }
    }

    /// Creates transfer history from the previous paid fare.
    #[must_use]
    pub const fn from_previous_paid_fare(event_time: EventTime) -> Self {
        Self {
            previous_paid_fare_event_time: Some(event_time),
        }
    }

    /// Returns the previous paid-fare event time.
    #[must_use]
    pub const fn previous_paid_fare_event_time(self) -> Option<EventTime> {
        self.previous_paid_fare_event_time
    }
}

/// Errors produced while evaluating a transfer benefit.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TransferEvaluationError {
    /// The fare being evaluated used a currency different from the policy.
    #[error("transfer fare uses {actual}, expected policy currency {expected}")]
    CurrencyMismatch {
        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the fare.
        actual: Currency,
    },

    /// Transfer evaluation cannot process a negative fare.
    #[error("transfer fare cannot be negative: {fare}")]
    NegativeFare {
        /// Invalid fare.
        fare: Money,
    },

    /// The previous fare occurred after the current fare presentation.
    #[error(
        "previous paid fare at {previous_unix_milliseconds} occurred after current fare at {current_unix_milliseconds}"
    )]
    PreviousFareOccursAfterCurrent {
        /// Previous paid-fare timestamp.
        previous_unix_milliseconds: i64,

        /// Current fare-presentation timestamp.
        current_unix_milliseconds: i64,
    },

    /// A validated transfer subtraction unexpectedly failed.
    #[error("transfer discount subtraction failed")]
    ArithmeticFailure,
}

/// Result of applying the transfer rule to one fare.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct TransferApplication {
    policy_id: FarePolicyId,
    policy_version: FarePolicyVersion,
    eligible: bool,
    discount_applied: Money,
    fare_after_transfer: Money,
}

impl TransferApplication {
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

    /// Reports whether the previous fare was inside the transfer window.
    #[must_use]
    pub const fn eligible(self) -> bool {
        self.eligible
    }

    /// Returns the transfer discount actually applied.
    #[must_use]
    pub const fn discount_applied(self) -> Money {
        self.discount_applied
    }

    /// Returns the fare after the transfer benefit.
    #[must_use]
    pub const fn fare_after_transfer(self) -> Money {
        self.fare_after_transfer
    }
}

/// Applies a transfer benefit to a fare that has already received its
/// eligibility discount.
///
/// The transfer window is inclusive. A previous paid fare exactly on the
/// configured boundary remains eligible.
///
/// The discount cannot reduce the fare below zero.
pub fn apply_transfer(
    policy: FarePolicy,
    current_event_time: EventTime,
    history: TransferHistory,
    fare_after_eligibility: Money,
) -> Result<TransferApplication, TransferEvaluationError> {
    validate_fare(policy, fare_after_eligibility)?;

    let eligible = transfer_is_eligible(policy, current_event_time, history)?;

    let discount_applied = if eligible {
        let discount_minor_units = policy
            .transfer_discount()
            .minor_units()
            .min(fare_after_eligibility.minor_units());

        Money::from_minor_units(discount_minor_units, policy.currency())
    } else {
        Money::zero(policy.currency())
    };

    let fare_after_transfer = fare_after_eligibility
        .checked_subtract(discount_applied)
        .map_err(|_| TransferEvaluationError::ArithmeticFailure)?;

    Ok(TransferApplication {
        policy_id: policy.id(),
        policy_version: policy.version(),
        eligible,
        discount_applied,
        fare_after_transfer,
    })
}

fn validate_fare(policy: FarePolicy, fare: Money) -> Result<(), TransferEvaluationError> {
    if fare.currency() != policy.currency() {
        return Err(TransferEvaluationError::CurrencyMismatch {
            expected: policy.currency(),
            actual: fare.currency(),
        });
    }

    if fare.is_negative() {
        return Err(TransferEvaluationError::NegativeFare { fare });
    }

    Ok(())
}

fn transfer_is_eligible(
    policy: FarePolicy,
    current_event_time: EventTime,
    history: TransferHistory,
) -> Result<bool, TransferEvaluationError> {
    let Some(previous_event_time) = history.previous_paid_fare_event_time() else {
        return Ok(false);
    };

    let previous_unix_milliseconds = previous_event_time.unix_milliseconds();

    let current_unix_milliseconds = current_event_time.unix_milliseconds();

    if previous_unix_milliseconds > current_unix_milliseconds {
        return Err(TransferEvaluationError::PreviousFareOccursAfterCurrent {
            previous_unix_milliseconds,
            current_unix_milliseconds,
        });
    }

    let elapsed_milliseconds =
        (current_unix_milliseconds - previous_unix_milliseconds).unsigned_abs();

    Ok(elapsed_milliseconds <= policy.transfer_window().milliseconds())
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{Currency, EventTime, FarePolicyId, FarePolicyVersion, Money};

    use crate::{
        DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition, TransferWindow,
    };

    use super::{TransferEvaluationError, TransferHistory, apply_transfer};

    fn event_time(milliseconds: i64) -> EventTime {
        let Ok(event_time) = EventTime::from_unix_milliseconds(milliseconds) else {
            panic!("test event time must be valid");
        };

        event_time
    }

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
    fn missing_history_does_not_apply_transfer_discount() {
        let result = apply_transfer(
            policy(),
            event_time(10_000_000),
            TransferHistory::none(),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("valid transfer evaluation must succeed");
        };

        assert!(!application.eligible());

        assert_eq!(application.discount_applied(), Money::zero(Currency::Usd));

        assert_eq!(
            application.fare_after_transfer(),
            Money::from_minor_units(250, Currency::Usd)
        );
    }

    #[test]
    fn exact_transfer_window_boundary_is_eligible() {
        let result = apply_transfer(
            policy(),
            event_time(10_000_000),
            TransferHistory::from_previous_paid_fare(event_time(4_600_000)),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("boundary transfer must evaluate");
        };

        assert!(application.eligible());

        assert_eq!(
            application.discount_applied(),
            Money::from_minor_units(250, Currency::Usd)
        );

        assert_eq!(
            application.fare_after_transfer(),
            Money::zero(Currency::Usd)
        );
    }

    #[test]
    fn one_millisecond_after_window_is_not_eligible() {
        let result = apply_transfer(
            policy(),
            event_time(10_000_001),
            TransferHistory::from_previous_paid_fare(event_time(4_600_000)),
            Money::from_minor_units(250, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("expired transfer must evaluate");
        };

        assert!(!application.eligible());

        assert_eq!(
            application.fare_after_transfer(),
            Money::from_minor_units(250, Currency::Usd)
        );
    }

    #[test]
    fn future_previous_fare_is_rejected() {
        let result = apply_transfer(
            policy(),
            event_time(10_000_000),
            TransferHistory::from_previous_paid_fare(event_time(10_000_001)),
            Money::from_minor_units(250, Currency::Usd),
        );

        assert!(matches!(
            result,
            Err(TransferEvaluationError::PreviousFareOccursAfterCurrent {
                previous_unix_milliseconds: 10_000_001,
                current_unix_milliseconds: 10_000_000
            })
        ));
    }

    #[test]
    fn transfer_discount_cannot_reduce_fare_below_zero() {
        let result = apply_transfer(
            policy(),
            event_time(10_000_000),
            TransferHistory::from_previous_paid_fare(event_time(9_000_000)),
            Money::from_minor_units(100, Currency::Usd),
        );

        let Ok(application) = result else {
            panic!("clamped transfer must evaluate");
        };

        assert_eq!(
            application.discount_applied(),
            Money::from_minor_units(100, Currency::Usd)
        );

        assert_eq!(
            application.fare_after_transfer(),
            Money::zero(Currency::Usd)
        );
    }
}
