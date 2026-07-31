use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{Currency, Money, MoneyError, RiderId, TransitAccountId};

/// The operational status of a transit account.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum TransitAccountStatus {
    /// The account may participate in normal fare processing.
    Active,

    /// Fare activity is temporarily prohibited.
    Suspended,

    /// The account is permanently closed.
    Closed,
}

impl fmt::Display for TransitAccountStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Closed => "closed",
        };

        formatter.write_str(status)
    }
}

/// A project-owned classification that may affect fare calculation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum EligibilityClassification {
    /// Standard simulated fare eligibility.
    Standard,

    /// Fictional youth eligibility.
    Youth,

    /// Fictional senior eligibility.
    Senior,

    /// Fictional reduced-fare eligibility.
    ReducedFare,

    /// Development-only employee test eligibility.
    EmployeeTestAccount,
}

/// Errors produced while managing stored value.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StoredValueError {
    /// A stored-value balance cannot be negative.
    #[error("stored-value balance cannot be negative: {balance}")]
    NegativeBalance {
        /// Invalid negative balance.
        balance: Money,
    },

    /// A credit must increase the balance.
    #[error("stored-value credit must be positive: {amount}")]
    NonPositiveCredit {
        /// Invalid credit amount.
        amount: Money,
    },

    /// A debit must reduce the balance.
    #[error("stored-value debit must be positive: {amount}")]
    NonPositiveDebit {
        /// Invalid debit amount.
        amount: Money,
    },

    /// The account did not have enough stored value.
    #[error("insufficient stored value: available {available}, requested {requested}")]
    InsufficientStoredValue {
        /// Balance available before the attempted debit.
        available: Money,

        /// Amount requested by the attempted debit.
        requested: Money,
    },

    /// The monetary operation itself was invalid.
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// A non-negative simulated balance held by a transit account.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct StoredValueBalance(Money);

impl StoredValueBalance {
    /// Creates a validated stored-value balance.
    pub fn new(amount: Money) -> Result<Self, StoredValueError> {
        if amount.is_negative() {
            return Err(StoredValueError::NegativeBalance { balance: amount });
        }

        Ok(Self(amount))
    }

    /// Creates a zero balance in the supplied currency.
    #[must_use]
    pub const fn zero(currency: Currency) -> Self {
        Self(Money::zero(currency))
    }

    /// Returns the current monetary amount.
    #[must_use]
    pub const fn amount(self) -> Money {
        self.0
    }

    /// Returns the balance currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.0.currency()
    }

    /// Adds a positive amount to the balance.
    pub fn credit(&mut self, amount: Money) -> Result<(), StoredValueError> {
        if !amount.is_positive() {
            return Err(StoredValueError::NonPositiveCredit { amount });
        }

        self.0 = self.0.checked_add(amount)?;
        Ok(())
    }

    /// Removes a positive amount from the balance.
    pub fn debit(&mut self, amount: Money) -> Result<(), StoredValueError> {
        if !amount.is_positive() {
            return Err(StoredValueError::NonPositiveDebit { amount });
        }

        let updated = self.0.checked_subtract(amount)?;

        if updated.is_negative() {
            return Err(StoredValueError::InsufficientStoredValue {
                available: self.0,
                requested: amount,
            });
        }

        self.0 = updated;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for StoredValueBalance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let amount = Money::deserialize(deserializer)?;

        Self::new(amount).map_err(serde::de::Error::custom)
    }
}

/// Errors produced by transit-account operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TransitAccountError {
    /// An operation requires an active account.
    #[error("transit account must be active; current status is {status}")]
    AccountNotActive {
        /// Current account status.
        status: TransitAccountStatus,
    },

    /// Closed accounts cannot receive further changes.
    #[error("closed transit account cannot be modified")]
    AccountClosed,

    /// The requested lifecycle transition is prohibited.
    #[error("cannot transition transit account from {from} to {to}")]
    InvalidStatusTransition {
        /// Status before the attempted transition.
        from: TransitAccountStatus,

        /// Requested status.
        to: TransitAccountStatus,
    },

    /// A stored-value operation failed.
    #[error(transparent)]
    StoredValue(#[from] StoredValueError),
}

/// The aggregate that owns a rider's simulated fare-related account state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TransitAccount {
    id: TransitAccountId,
    rider_id: RiderId,
    status: TransitAccountStatus,
    eligibility: EligibilityClassification,
    stored_value: StoredValueBalance,
}

impl TransitAccount {
    /// Creates a new active transit account.
    pub fn new(
        id: TransitAccountId,
        rider_id: RiderId,
        eligibility: EligibilityClassification,
        initial_balance: Money,
    ) -> Result<Self, TransitAccountError> {
        let stored_value = StoredValueBalance::new(initial_balance)?;

        Ok(Self {
            id,
            rider_id,
            status: TransitAccountStatus::Active,
            eligibility,
            stored_value,
        })
    }

    /// Returns the account identifier.
    #[must_use]
    pub const fn id(&self) -> TransitAccountId {
        self.id
    }

    /// Returns the associated rider identifier.
    #[must_use]
    pub const fn rider_id(&self) -> RiderId {
        self.rider_id
    }

    /// Returns the current account status.
    #[must_use]
    pub const fn status(&self) -> TransitAccountStatus {
        self.status
    }

    /// Returns the current eligibility classification.
    #[must_use]
    pub const fn eligibility(&self) -> EligibilityClassification {
        self.eligibility
    }

    /// Returns the current stored-value balance.
    #[must_use]
    pub const fn stored_value(&self) -> StoredValueBalance {
        self.stored_value
    }

    /// Returns whether the account is active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.status, TransitAccountStatus::Active)
    }

    /// Credits positive simulated funds to the account.
    ///
    /// Suspended accounts may receive funds, but closed accounts cannot.
    pub fn credit_stored_value(&mut self, amount: Money) -> Result<(), TransitAccountError> {
        if self.status == TransitAccountStatus::Closed {
            return Err(TransitAccountError::AccountClosed);
        }

        self.stored_value.credit(amount)?;
        Ok(())
    }

    /// Debits stored value from an active account.
    pub fn debit_stored_value(&mut self, amount: Money) -> Result<(), TransitAccountError> {
        self.ensure_active()?;
        self.stored_value.debit(amount)?;
        Ok(())
    }

    /// Suspends an active account.
    ///
    /// Suspending an already suspended account is idempotent.
    pub fn suspend(&mut self) -> Result<(), TransitAccountError> {
        match self.status {
            TransitAccountStatus::Active => {
                self.status = TransitAccountStatus::Suspended;
                Ok(())
            }
            TransitAccountStatus::Suspended => Ok(()),
            TransitAccountStatus::Closed => Err(TransitAccountError::InvalidStatusTransition {
                from: TransitAccountStatus::Closed,
                to: TransitAccountStatus::Suspended,
            }),
        }
    }

    /// Reactivates a suspended account.
    ///
    /// Reactivating an already active account is idempotent.
    pub fn reactivate(&mut self) -> Result<(), TransitAccountError> {
        match self.status {
            TransitAccountStatus::Active => Ok(()),
            TransitAccountStatus::Suspended => {
                self.status = TransitAccountStatus::Active;
                Ok(())
            }
            TransitAccountStatus::Closed => Err(TransitAccountError::InvalidStatusTransition {
                from: TransitAccountStatus::Closed,
                to: TransitAccountStatus::Active,
            }),
        }
    }

    /// Permanently closes the account.
    ///
    /// Closing an already closed account is idempotent.
    pub fn close(&mut self) {
        self.status = TransitAccountStatus::Closed;
    }

    /// Changes the account's eligibility classification.
    pub fn change_eligibility(
        &mut self,
        eligibility: EligibilityClassification,
    ) -> Result<(), TransitAccountError> {
        if self.status == TransitAccountStatus::Closed {
            return Err(TransitAccountError::AccountClosed);
        }

        self.eligibility = eligibility;
        Ok(())
    }

    fn ensure_active(&self) -> Result<(), TransitAccountError> {
        if self.status != TransitAccountStatus::Active {
            return Err(TransitAccountError::AccountNotActive {
                status: self.status,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Currency, Money, RiderId, TransitAccountId};

    use super::{
        EligibilityClassification, StoredValueBalance, StoredValueError, TransitAccount,
        TransitAccountError, TransitAccountStatus,
    };

    fn active_account(minor_units: i64) -> TransitAccount {
        let result = TransitAccount::new(
            TransitAccountId::generate(),
            RiderId::generate(),
            EligibilityClassification::Standard,
            Money::from_minor_units(minor_units, Currency::Usd),
        );

        match result {
            Ok(account) => account,
            Err(error) => {
                panic!("test account construction failed: {error}")
            }
        }
    }

    #[test]
    fn new_account_is_active() {
        let account = active_account(1_000);

        assert_eq!(account.status(), TransitAccountStatus::Active);
        assert!(account.is_active());
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(1_000, Currency::Usd)
        );
    }

    #[test]
    fn negative_initial_balance_is_rejected() {
        let result = TransitAccount::new(
            TransitAccountId::generate(),
            RiderId::generate(),
            EligibilityClassification::Standard,
            Money::from_minor_units(-1, Currency::Usd),
        );

        assert!(matches!(
            result,
            Err(TransitAccountError::StoredValue(
                StoredValueError::NegativeBalance { .. }
            ))
        ));
    }

    #[test]
    fn zero_balance_is_valid() {
        let result = StoredValueBalance::new(Money::zero(Currency::Usd));

        assert!(matches!(
            result,
            Ok(balance) if balance.amount()
                == Money::zero(Currency::Usd)
        ));
    }

    #[test]
    fn active_account_can_be_credited() {
        let mut account = active_account(1_000);

        let result = account.credit_stored_value(Money::from_minor_units(250, Currency::Usd));

        assert!(result.is_ok());
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(1_250, Currency::Usd)
        );
    }

    #[test]
    fn active_account_can_be_debited() {
        let mut account = active_account(1_000);

        let result = account.debit_stored_value(Money::from_minor_units(250, Currency::Usd));

        assert!(result.is_ok());
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(750, Currency::Usd)
        );
    }

    #[test]
    fn insufficient_balance_is_rejected_without_mutation() {
        let mut account = active_account(100);

        let result = account.debit_stored_value(Money::from_minor_units(101, Currency::Usd));

        assert!(matches!(
            result,
            Err(TransitAccountError::StoredValue(
                StoredValueError::InsufficientStoredValue { .. }
            ))
        ));
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(100, Currency::Usd)
        );
    }

    #[test]
    fn zero_credit_is_rejected() {
        let mut account = active_account(100);

        let result = account.credit_stored_value(Money::zero(Currency::Usd));

        assert!(matches!(
            result,
            Err(TransitAccountError::StoredValue(
                StoredValueError::NonPositiveCredit { .. }
            ))
        ));
    }

    #[test]
    fn zero_debit_is_rejected() {
        let mut account = active_account(100);

        let result = account.debit_stored_value(Money::zero(Currency::Usd));

        assert!(matches!(
            result,
            Err(TransitAccountError::StoredValue(
                StoredValueError::NonPositiveDebit { .. }
            ))
        ));
    }

    #[test]
    fn suspended_account_cannot_be_debited() {
        let mut account = active_account(1_000);

        assert!(account.suspend().is_ok());

        let result = account.debit_stored_value(Money::from_minor_units(100, Currency::Usd));

        assert!(matches!(
            result,
            Err(TransitAccountError::AccountNotActive {
                status: TransitAccountStatus::Suspended
            })
        ));
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(1_000, Currency::Usd)
        );
    }

    #[test]
    fn suspended_account_may_receive_credit() {
        let mut account = active_account(1_000);

        assert!(account.suspend().is_ok());

        let result = account.credit_stored_value(Money::from_minor_units(100, Currency::Usd));

        assert!(result.is_ok());
        assert_eq!(
            account.stored_value().amount(),
            Money::from_minor_units(1_100, Currency::Usd)
        );
    }

    #[test]
    fn suspended_account_can_be_reactivated() {
        let mut account = active_account(1_000);

        assert!(account.suspend().is_ok());
        assert!(account.reactivate().is_ok());

        assert_eq!(account.status(), TransitAccountStatus::Active);
    }

    #[test]
    fn closed_account_is_terminal() {
        let mut account = active_account(1_000);
        account.close();

        let reactivate_result = account.reactivate();
        let credit_result =
            account.credit_stored_value(Money::from_minor_units(100, Currency::Usd));

        assert!(matches!(
            reactivate_result,
            Err(TransitAccountError::InvalidStatusTransition {
                from: TransitAccountStatus::Closed,
                to: TransitAccountStatus::Active
            })
        ));
        assert!(matches!(
            credit_result,
            Err(TransitAccountError::AccountClosed)
        ));
    }

    #[test]
    fn eligibility_can_change_before_closure() {
        let mut account = active_account(1_000);

        let result = account.change_eligibility(EligibilityClassification::ReducedFare);

        assert!(result.is_ok());
        assert_eq!(
            account.eligibility(),
            EligibilityClassification::ReducedFare
        );
    }

    #[test]
    fn closed_account_eligibility_cannot_change() {
        let mut account = active_account(1_000);
        account.close();

        let result = account.change_eligibility(EligibilityClassification::Senior);

        assert!(matches!(result, Err(TransitAccountError::AccountClosed)));
    }

    #[test]
    fn currency_mismatch_is_preserved() {
        let mut account = active_account(1_000);

        let result = account.credit_stored_value(Money::from_minor_units(100, Currency::Eur));

        assert!(matches!(
            result,
            Err(TransitAccountError::StoredValue(StoredValueError::Money(_)))
        ));
    }
}
