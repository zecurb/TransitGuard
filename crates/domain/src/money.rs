use core::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A currency supported by TransitGuard's fictional fare environment.
///
/// All currently supported currencies use two decimal minor units.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Currency {
    /// United States dollar.
    Usd,

    /// Canadian dollar.
    Cad,

    /// Euro.
    Eur,

    /// British pound sterling.
    Gbp,
}

impl Currency {
    /// Returns the ISO-style three-letter currency code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Cad => "CAD",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
        }
    }

    /// Returns the number of decimal places used by the currency.
    #[must_use]
    pub const fn minor_unit_scale(self) -> u8 {
        2
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// An operation that can fail because of integer overflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoneyOperation {
    /// Addition of two monetary values.
    Addition,

    /// Subtraction of one monetary value from another.
    Subtraction,

    /// Negation of a monetary value.
    Negation,
}

impl fmt::Display for MoneyOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = match self {
            Self::Addition => "addition",
            Self::Subtraction => "subtraction",
            Self::Negation => "negation",
        };

        formatter.write_str(operation)
    }
}

/// Errors produced by monetary operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum MoneyError {
    /// An operation attempted to combine different currencies.
    #[error("currency mismatch: cannot combine {left} with {right}")]
    CurrencyMismatch {
        /// Currency of the left operand.
        left: Currency,

        /// Currency of the right operand.
        right: Currency,
    },

    /// An integer operation exceeded the supported range.
    #[error("money {operation} overflowed the supported range")]
    ArithmeticOverflow {
        /// Operation that overflowed.
        operation: MoneyOperation,
    },
}

/// A monetary value represented using signed integer minor units.
///
/// TransitGuard never uses floating-point arithmetic for balances, fares,
/// adjustments, or transaction amounts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Money {
    minor_units: i64,
    currency: Currency,
}

impl Money {
    /// Creates a monetary value from signed minor units.
    ///
    /// For currencies with a scale of two, `1_250` represents `12.50`.
    #[must_use]
    pub const fn from_minor_units(minor_units: i64, currency: Currency) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    /// Creates a zero monetary value in the supplied currency.
    #[must_use]
    pub const fn zero(currency: Currency) -> Self {
        Self::from_minor_units(0, currency)
    }

    /// Returns the signed number of minor units.
    #[must_use]
    pub const fn minor_units(self) -> i64 {
        self.minor_units
    }

    /// Returns the value's currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// Returns whether the value is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.minor_units == 0
    }

    /// Returns whether the value is greater than zero.
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.minor_units > 0
    }

    /// Returns whether the value is less than zero.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.minor_units < 0
    }

    /// Adds another monetary value after validating its currency.
    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.ensure_same_currency(other)?;

        let minor_units = self.minor_units.checked_add(other.minor_units).ok_or(
            MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Addition,
            },
        )?;

        Ok(Self::from_minor_units(minor_units, self.currency))
    }

    /// Subtracts another monetary value after validating its currency.
    pub fn checked_subtract(self, other: Self) -> Result<Self, MoneyError> {
        self.ensure_same_currency(other)?;

        let minor_units = self.minor_units.checked_sub(other.minor_units).ok_or(
            MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Subtraction,
            },
        )?;

        Ok(Self::from_minor_units(minor_units, self.currency))
    }

    /// Returns the safely negated monetary value.
    pub fn checked_negate(self) -> Result<Self, MoneyError> {
        let minor_units = self
            .minor_units
            .checked_neg()
            .ok_or(MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Negation,
            })?;

        Ok(Self::from_minor_units(minor_units, self.currency))
    }

    fn ensure_same_currency(self, other: Self) -> Result<(), MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            });
        }

        Ok(())
    }
}

impl fmt::Display for Money {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let absolute_minor_units = self.minor_units.unsigned_abs();
        let major_units = absolute_minor_units / 100;
        let remaining_minor_units = absolute_minor_units % 100;

        if self.minor_units < 0 {
            write!(
                formatter,
                "{} -{}.{:02}",
                self.currency, major_units, remaining_minor_units
            )
        } else {
            write!(
                formatter,
                "{} {}.{:02}",
                self.currency, major_units, remaining_minor_units
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Currency, Money, MoneyError, MoneyOperation};

    #[test]
    fn currency_exposes_code_and_scale() {
        assert_eq!(Currency::Usd.code(), "USD");
        assert_eq!(Currency::Usd.minor_unit_scale(), 2);
    }

    #[test]
    fn money_preserves_minor_units_and_currency() {
        let money = Money::from_minor_units(1_250, Currency::Usd);

        assert_eq!(money.minor_units(), 1_250);
        assert_eq!(money.currency(), Currency::Usd);
        assert!(money.is_positive());
        assert!(!money.is_zero());
        assert!(!money.is_negative());
    }

    #[test]
    fn zero_money_is_zero() {
        let money = Money::zero(Currency::Usd);

        assert!(money.is_zero());
        assert!(!money.is_positive());
        assert!(!money.is_negative());
    }

    #[test]
    fn negative_money_is_negative() {
        let money = Money::from_minor_units(-125, Currency::Usd);

        assert!(money.is_negative());
        assert!(!money.is_positive());
        assert!(!money.is_zero());
    }

    #[test]
    fn same_currency_values_can_be_added() {
        let left = Money::from_minor_units(1_250, Currency::Usd);
        let right = Money::from_minor_units(250, Currency::Usd);

        let result = left.checked_add(right);

        assert!(matches!(
            result,
            Ok(value)
                if value
                    == Money::from_minor_units(
                        1_500,
                        Currency::Usd
                    )
        ));
    }

    #[test]
    fn same_currency_values_can_be_subtracted() {
        let left = Money::from_minor_units(1_250, Currency::Usd);
        let right = Money::from_minor_units(250, Currency::Usd);

        let result = left.checked_subtract(right);

        assert!(matches!(
            result,
            Ok(value)
                if value
                    == Money::from_minor_units(
                        1_000,
                        Currency::Usd
                    )
        ));
    }

    #[test]
    fn different_currencies_cannot_be_combined() {
        let dollars = Money::from_minor_units(1_000, Currency::Usd);
        let euros = Money::from_minor_units(1_000, Currency::Eur);

        let result = dollars.checked_add(euros);

        assert!(matches!(
            result,
            Err(MoneyError::CurrencyMismatch {
                left: Currency::Usd,
                right: Currency::Eur
            })
        ));
    }

    #[test]
    fn addition_overflow_is_reported() {
        let maximum = Money::from_minor_units(i64::MAX, Currency::Usd);
        let one = Money::from_minor_units(1, Currency::Usd);

        let result = maximum.checked_add(one);

        assert!(matches!(
            result,
            Err(MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Addition
            })
        ));
    }

    #[test]
    fn subtraction_overflow_is_reported() {
        let minimum = Money::from_minor_units(i64::MIN, Currency::Usd);
        let one = Money::from_minor_units(1, Currency::Usd);

        let result = minimum.checked_subtract(one);

        assert!(matches!(
            result,
            Err(MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Subtraction
            })
        ));
    }

    #[test]
    fn negation_overflow_is_reported() {
        let minimum = Money::from_minor_units(i64::MIN, Currency::Usd);

        let result = minimum.checked_negate();

        assert!(matches!(
            result,
            Err(MoneyError::ArithmeticOverflow {
                operation: MoneyOperation::Negation
            })
        ));
    }

    #[test]
    fn positive_money_has_stable_display_format() {
        let money = Money::from_minor_units(12_345, Currency::Usd);

        assert_eq!(money.to_string(), "USD 123.45");
    }

    #[test]
    fn negative_money_has_stable_display_format() {
        let money = Money::from_minor_units(-105, Currency::Gbp);

        assert_eq!(money.to_string(), "GBP -1.05");
    }

    #[test]
    fn minimum_integer_has_stable_display_format() {
        let money = Money::from_minor_units(i64::MIN, Currency::Usd);

        assert_eq!(money.to_string(), "USD -92233720368547758.08");
    }
}
