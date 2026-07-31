use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use transitguard_domain::{
    Currency, EligibilityClassification, FarePolicyId, FarePolicyVersion, Money,
};

/// Errors produced while constructing fare-policy value objects.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FarePolicyValueError {
    /// TransitGuard zone identifiers begin at one.
    #[error("zone identifier must be greater than zero")]
    ZeroZoneId,

    /// Transfer windows must contain a positive duration.
    #[error("transfer window must be greater than zero milliseconds")]
    ZeroTransferWindow,

    /// A percentage discount cannot exceed the full fare.
    #[error("discount basis points cannot exceed 10000: {basis_points}")]
    DiscountAboveFullFare {
        /// Invalid number of basis points.
        basis_points: u16,
    },
}

/// Errors produced while validating a complete fare policy.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FarePolicyError {
    /// A monetary policy field used the wrong currency.
    #[error("fare-policy field `{field}` uses {actual}, expected {expected}")]
    CurrencyMismatch {
        /// Policy field containing the invalid amount.
        field: &'static str,

        /// Currency required by the policy.
        expected: Currency,

        /// Currency supplied by the field.
        actual: Currency,
    },

    /// A monetary policy field contained a negative amount.
    #[error("fare-policy field `{field}` cannot be negative: {amount}")]
    NegativeAmount {
        /// Policy field containing the invalid amount.
        field: &'static str,

        /// Invalid amount.
        amount: Money,
    },

    /// The daily cap was lower than the normal base fare.
    #[error("daily fare cap {daily_cap} cannot be lower than base fare {base_fare}")]
    DailyCapBelowBaseFare {
        /// Configured base fare.
        base_fare: Money,

        /// Invalid daily cap.
        daily_cap: Money,
    },

    /// The weekly cap was lower than the daily cap.
    #[error("weekly fare cap {weekly_cap} cannot be lower than daily cap {daily_cap}")]
    WeeklyCapBelowDailyCap {
        /// Configured daily cap.
        daily_cap: Money,

        /// Invalid weekly cap.
        weekly_cap: Money,
    },

    /// The transfer discount exceeded the normal base fare.
    #[error("transfer discount {transfer_discount} cannot exceed base fare {base_fare}")]
    TransferDiscountExceedsBaseFare {
        /// Configured base fare.
        base_fare: Money,

        /// Invalid transfer discount.
        transfer_discount: Money,
    },
}

/// A project-owned transit fare-zone identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ZoneId(u16);

impl ZoneId {
    /// Creates a validated zone identifier.
    pub const fn new(value: u16) -> Result<Self, FarePolicyValueError> {
        if value == 0 {
            return Err(FarePolicyValueError::ZeroZoneId);
        }

        Ok(Self(value))
    }

    /// Returns the numeric zone identifier.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ZoneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Duration during which a previous fare may qualify for a transfer benefit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TransferWindow(u64);

impl TransferWindow {
    /// Creates a transfer window from milliseconds.
    pub const fn from_milliseconds(milliseconds: u64) -> Result<Self, FarePolicyValueError> {
        if milliseconds == 0 {
            return Err(FarePolicyValueError::ZeroTransferWindow);
        }

        Ok(Self(milliseconds))
    }

    /// Returns the transfer-window duration in milliseconds.
    #[must_use]
    pub const fn milliseconds(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for TransferWindow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;

        Self::from_milliseconds(value).map_err(serde::de::Error::custom)
    }
}

/// A percentage represented as one-hundredth of one percent.
///
/// Ten thousand basis points represents a 100 percent discount.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DiscountBasisPoints(u16);

impl DiscountBasisPoints {
    /// No discount.
    pub const ZERO: Self = Self(0);

    /// A full-fare discount.
    pub const FULL_FARE: Self = Self(10_000);

    /// Creates a validated discount percentage.
    pub const fn new(basis_points: u16) -> Result<Self, FarePolicyValueError> {
        if basis_points > 10_000 {
            return Err(FarePolicyValueError::DiscountAboveFullFare { basis_points });
        }

        Ok(Self(basis_points))
    }

    /// Returns the number of basis points.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DiscountBasisPoints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Eligibility-based discount configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EligibilityDiscounts {
    youth: DiscountBasisPoints,
    senior: DiscountBasisPoints,
    reduced_fare: DiscountBasisPoints,
    employee_test_account: DiscountBasisPoints,
}

impl EligibilityDiscounts {
    /// Creates an eligibility-discount configuration.
    #[must_use]
    pub const fn new(
        youth: DiscountBasisPoints,
        senior: DiscountBasisPoints,
        reduced_fare: DiscountBasisPoints,
        employee_test_account: DiscountBasisPoints,
    ) -> Self {
        Self {
            youth,
            senior,
            reduced_fare,
            employee_test_account,
        }
    }

    /// Returns a configuration with no eligibility discounts.
    #[must_use]
    pub const fn none() -> Self {
        Self::new(
            DiscountBasisPoints::ZERO,
            DiscountBasisPoints::ZERO,
            DiscountBasisPoints::ZERO,
            DiscountBasisPoints::ZERO,
        )
    }

    /// Returns the configured discount for an eligibility classification.
    #[must_use]
    pub const fn rate_for(self, eligibility: EligibilityClassification) -> DiscountBasisPoints {
        match eligibility {
            EligibilityClassification::Standard => DiscountBasisPoints::ZERO,
            EligibilityClassification::Youth => self.youth,
            EligibilityClassification::Senior => self.senior,
            EligibilityClassification::ReducedFare => self.reduced_fare,
            EligibilityClassification::EmployeeTestAccount => self.employee_test_account,
        }
    }
}

/// Unvalidated fare-policy input.
///
/// Construct [`FarePolicy`] through [`FarePolicy::validate`] before using the
/// definition for fare evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FarePolicyDefinition {
    /// Stable policy identity.
    pub id: FarePolicyId,

    /// Immutable policy version.
    pub version: FarePolicyVersion,

    /// Currency used by every monetary rule.
    pub currency: Currency,

    /// Standard fare before discounts, transfers, and caps.
    pub base_fare: Money,

    /// Additional charge for each applicable zone adjustment.
    pub zone_surcharge: Money,

    /// Duration during which a transfer benefit remains available.
    pub transfer_window: TransferWindow,

    /// Amount removed from the base fare for an eligible transfer.
    pub transfer_discount: Money,

    /// Maximum amount charged during one service day.
    pub daily_cap: Money,

    /// Maximum amount charged during one service week.
    pub weekly_cap: Money,

    /// Eligibility-based percentage discounts.
    pub eligibility_discounts: EligibilityDiscounts,
}

/// A fully validated and immutable fare policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FarePolicy {
    definition: FarePolicyDefinition,
}

impl FarePolicy {
    /// Validates an entire fare-policy definition.
    pub fn validate(definition: FarePolicyDefinition) -> Result<Self, FarePolicyError> {
        let monetary_fields = [
            ("base_fare", definition.base_fare),
            ("zone_surcharge", definition.zone_surcharge),
            ("transfer_discount", definition.transfer_discount),
            ("daily_cap", definition.daily_cap),
            ("weekly_cap", definition.weekly_cap),
        ];

        for (field, amount) in monetary_fields {
            validate_money(field, amount, definition.currency)?;
        }

        if definition.daily_cap.minor_units() < definition.base_fare.minor_units() {
            return Err(FarePolicyError::DailyCapBelowBaseFare {
                base_fare: definition.base_fare,
                daily_cap: definition.daily_cap,
            });
        }

        if definition.weekly_cap.minor_units() < definition.daily_cap.minor_units() {
            return Err(FarePolicyError::WeeklyCapBelowDailyCap {
                daily_cap: definition.daily_cap,
                weekly_cap: definition.weekly_cap,
            });
        }

        if definition.transfer_discount.minor_units() > definition.base_fare.minor_units() {
            return Err(FarePolicyError::TransferDiscountExceedsBaseFare {
                base_fare: definition.base_fare,
                transfer_discount: definition.transfer_discount,
            });
        }

        Ok(Self { definition })
    }

    /// Returns the stable policy identity.
    #[must_use]
    pub const fn id(self) -> FarePolicyId {
        self.definition.id
    }

    /// Returns the immutable policy version.
    #[must_use]
    pub const fn version(self) -> FarePolicyVersion {
        self.definition.version
    }

    /// Returns the policy currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.definition.currency
    }

    /// Returns the standard base fare.
    #[must_use]
    pub const fn base_fare(self) -> Money {
        self.definition.base_fare
    }

    /// Returns the per-zone surcharge.
    #[must_use]
    pub const fn zone_surcharge(self) -> Money {
        self.definition.zone_surcharge
    }

    /// Returns the transfer window.
    #[must_use]
    pub const fn transfer_window(self) -> TransferWindow {
        self.definition.transfer_window
    }

    /// Returns the transfer discount.
    #[must_use]
    pub const fn transfer_discount(self) -> Money {
        self.definition.transfer_discount
    }

    /// Returns the daily fare cap.
    #[must_use]
    pub const fn daily_cap(self) -> Money {
        self.definition.daily_cap
    }

    /// Returns the weekly fare cap.
    #[must_use]
    pub const fn weekly_cap(self) -> Money {
        self.definition.weekly_cap
    }

    /// Returns the configured eligibility discounts.
    #[must_use]
    pub const fn eligibility_discounts(self) -> EligibilityDiscounts {
        self.definition.eligibility_discounts
    }

    /// Returns the complete validated policy definition.
    #[must_use]
    pub const fn definition(self) -> FarePolicyDefinition {
        self.definition
    }
}

fn validate_money(
    field: &'static str,
    amount: Money,
    expected_currency: Currency,
) -> Result<(), FarePolicyError> {
    if amount.currency() != expected_currency {
        return Err(FarePolicyError::CurrencyMismatch {
            field,
            expected: expected_currency,
            actual: amount.currency(),
        });
    }

    if amount.is_negative() {
        return Err(FarePolicyError::NegativeAmount { field, amount });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        Currency, EligibilityClassification, FarePolicyId, FarePolicyVersion, Money,
    };

    use super::{
        DiscountBasisPoints, EligibilityDiscounts, FarePolicy, FarePolicyDefinition,
        FarePolicyError, FarePolicyValueError, TransferWindow, ZoneId,
    };

    fn policy_version() -> FarePolicyVersion {
        let Ok(version) = FarePolicyVersion::new(1) else {
            panic!("version one must be valid");
        };

        version
    }

    fn transfer_window() -> TransferWindow {
        let Ok(window) = TransferWindow::from_milliseconds(90 * 60 * 1_000) else {
            panic!("positive transfer window must be valid");
        };

        window
    }

    fn valid_definition() -> FarePolicyDefinition {
        FarePolicyDefinition {
            id: FarePolicyId::generate(),
            version: policy_version(),
            currency: Currency::Usd,
            base_fare: Money::from_minor_units(250, Currency::Usd),
            zone_surcharge: Money::from_minor_units(75, Currency::Usd),
            transfer_window: transfer_window(),
            transfer_discount: Money::from_minor_units(250, Currency::Usd),
            daily_cap: Money::from_minor_units(750, Currency::Usd),
            weekly_cap: Money::from_minor_units(3_000, Currency::Usd),
            eligibility_discounts: EligibilityDiscounts::none(),
        }
    }

    #[test]
    fn valid_policy_preserves_identity_version_and_amounts() {
        let definition = valid_definition();

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Ok(policy)
                if policy.id() == definition.id
                    && policy.version() == definition.version
                    && policy.base_fare() == definition.base_fare
                    && policy.daily_cap() == definition.daily_cap
        ));
    }

    #[test]
    fn policy_rejects_cross_currency_amounts() {
        let mut definition = valid_definition();

        definition.daily_cap = Money::from_minor_units(750, Currency::Eur);

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Err(FarePolicyError::CurrencyMismatch {
                field: "daily_cap",
                expected: Currency::Usd,
                actual: Currency::Eur
            })
        ));
    }

    #[test]
    fn policy_rejects_negative_amounts() {
        let mut definition = valid_definition();

        definition.zone_surcharge = Money::from_minor_units(-1, Currency::Usd);

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Err(FarePolicyError::NegativeAmount {
                field: "zone_surcharge",
                ..
            })
        ));
    }

    #[test]
    fn daily_cap_cannot_be_below_base_fare() {
        let mut definition = valid_definition();

        definition.daily_cap = Money::from_minor_units(200, Currency::Usd);

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Err(FarePolicyError::DailyCapBelowBaseFare { .. })
        ));
    }

    #[test]
    fn weekly_cap_cannot_be_below_daily_cap() {
        let mut definition = valid_definition();

        definition.weekly_cap = Money::from_minor_units(700, Currency::Usd);

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Err(FarePolicyError::WeeklyCapBelowDailyCap { .. })
        ));
    }

    #[test]
    fn transfer_discount_cannot_exceed_base_fare() {
        let mut definition = valid_definition();

        definition.transfer_discount = Money::from_minor_units(251, Currency::Usd);

        let result = FarePolicy::validate(definition);

        assert!(matches!(
            result,
            Err(FarePolicyError::TransferDiscountExceedsBaseFare { .. })
        ));
    }

    #[test]
    fn value_objects_reject_invalid_boundaries() {
        assert!(matches!(
            ZoneId::new(0),
            Err(FarePolicyValueError::ZeroZoneId)
        ));

        assert!(matches!(
            TransferWindow::from_milliseconds(0),
            Err(FarePolicyValueError::ZeroTransferWindow)
        ));

        assert!(matches!(
            DiscountBasisPoints::new(10_001),
            Err(FarePolicyValueError::DiscountAboveFullFare {
                basis_points: 10_001
            })
        ));
    }

    #[test]
    fn eligibility_discounts_are_selected_deterministically() {
        let Ok(youth) = DiscountBasisPoints::new(5_000) else {
            panic!("fifty percent must be valid");
        };

        let Ok(senior) = DiscountBasisPoints::new(6_000) else {
            panic!("sixty percent must be valid");
        };

        let discounts = EligibilityDiscounts::new(
            youth,
            senior,
            DiscountBasisPoints::FULL_FARE,
            DiscountBasisPoints::FULL_FARE,
        );

        assert_eq!(
            discounts.rate_for(EligibilityClassification::Standard),
            DiscountBasisPoints::ZERO
        );

        assert_eq!(discounts.rate_for(EligibilityClassification::Youth), youth);

        assert_eq!(
            discounts.rate_for(EligibilityClassification::Senior),
            senior
        );
    }
}
