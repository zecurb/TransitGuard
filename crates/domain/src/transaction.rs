use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{FareCredentialId, FareTransactionId, Money, ReaderId, TransitAccountId};

/// Errors produced while constructing fare-transaction value objects.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareTransactionValueError {
    /// Reader-local sequence numbers begin at one.
    #[error("local sequence number must be greater than zero")]
    ZeroLocalSequenceNumber,

    /// Fare-policy versions begin at one.
    #[error("fare-policy version must be greater than zero")]
    ZeroFarePolicyVersion,

    /// Event time cannot be before the Unix epoch.
    #[error("event time cannot be negative: {unix_milliseconds}")]
    NegativeEventTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },
}

/// A reader-generated monotonically increasing transaction number.
///
/// The number is scoped to one reader identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LocalSequenceNumber(u64);

impl LocalSequenceNumber {
    /// Creates a validated local sequence number.
    pub const fn new(value: u64) -> Result<Self, FareTransactionValueError> {
        if value == 0 {
            return Err(FareTransactionValueError::ZeroLocalSequenceNumber);
        }

        Ok(Self(value))
    }

    /// Returns the numeric sequence value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for LocalSequenceNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// The immutable version of the fare policy used for a decision.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FarePolicyVersion(u64);

impl FarePolicyVersion {
    /// Creates a validated fare-policy version.
    pub const fn new(value: u64) -> Result<Self, FareTransactionValueError> {
        if value == 0 {
            return Err(FareTransactionValueError::ZeroFarePolicyVersion);
        }

        Ok(Self(value))
    }

    /// Returns the numeric version.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FarePolicyVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// The time at which a simulated tap occurred.
///
/// Event time is distinct from backend receipt and processing time.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EventTime(i64);

impl EventTime {
    /// Creates an event time from Unix milliseconds.
    pub const fn from_unix_milliseconds(
        unix_milliseconds: i64,
    ) -> Result<Self, FareTransactionValueError> {
        if unix_milliseconds < 0 {
            return Err(FareTransactionValueError::NegativeEventTime { unix_milliseconds });
        }

        Ok(Self(unix_milliseconds))
    }

    /// Returns Unix milliseconds.
    #[must_use]
    pub const fn unix_milliseconds(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for EventTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;

        Self::from_unix_milliseconds(value).map_err(serde::de::Error::custom)
    }
}

/// The connectivity mode used when the fare decision was produced.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareProcessingMode {
    /// The backend was available during processing.
    Online,

    /// The reader used bounded local processing.
    Offline,
}

/// The reason an approved fare transaction was permitted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareApprovalReason {
    /// A normal fare was charged.
    StandardFare,

    /// A transfer benefit affected the fare.
    Transfer,

    /// A transit product covered the fare.
    TransitProduct,

    /// A fare cap reduced or eliminated the charge.
    FareCap,

    /// A reader issued a bounded provisional offline approval.
    OfflineProvisional,
}

/// The reason a fare presentation was rejected.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareRejectionReason {
    /// The presented credential was revoked.
    CredentialRevoked,

    /// The presented credential was expired.
    CredentialExpired,

    /// The associated account was suspended.
    AccountSuspended,

    /// The reader was disabled.
    ReaderDisabled,

    /// Credential verification failed.
    InvalidSignature,

    /// The protocol version was unsupported.
    UnsupportedProtocol,

    /// The account lacked sufficient stored value.
    InsufficientStoredValue,

    /// The associated transit product was invalid.
    ProductInvalid,

    /// A configured offline risk limit was exceeded.
    OfflineLimitExceeded,

    /// The cached fare policy was too old.
    StalePolicy,

    /// The cached revocation data was too old.
    StaleRevocationData,
}

/// Errors produced while constructing fare decisions.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareDecisionError {
    /// Approved fares cannot charge a negative amount.
    #[error("approved fare amount cannot be negative: {amount}")]
    NegativeApprovedAmount {
        /// Invalid approved amount.
        amount: Money,
    },
}

/// The deterministic result of processing a fare presentation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum FareDecision {
    /// The simulated journey event was permitted.
    Approved {
        /// Simulated amount charged.
        charged_amount: Money,

        /// Reason for approval.
        reason: FareApprovalReason,
    },

    /// The simulated journey event was denied.
    Rejected {
        /// Reason for rejection.
        reason: FareRejectionReason,
    },
}

#[derive(Deserialize)]
enum FareDecisionRepresentation {
    Approved {
        charged_amount: Money,
        reason: FareApprovalReason,
    },

    Rejected {
        reason: FareRejectionReason,
    },
}

impl<'de> Deserialize<'de> for FareDecision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let representation = FareDecisionRepresentation::deserialize(deserializer)?;

        match representation {
            FareDecisionRepresentation::Approved {
                charged_amount,
                reason,
            } => Self::approved(charged_amount, reason).map_err(serde::de::Error::custom),

            FareDecisionRepresentation::Rejected { reason } => Ok(Self::rejected(reason)),
        }
    }
}

impl FareDecision {
    /// Creates an approved decision.
    ///
    /// Zero-value approvals are valid for transfers, products, or fare caps.
    pub fn approved(
        charged_amount: Money,
        reason: FareApprovalReason,
    ) -> Result<Self, FareDecisionError> {
        if charged_amount.is_negative() {
            return Err(FareDecisionError::NegativeApprovedAmount {
                amount: charged_amount,
            });
        }

        Ok(Self::Approved {
            charged_amount,
            reason,
        })
    }

    /// Creates a rejected decision.
    #[must_use]
    pub const fn rejected(reason: FareRejectionReason) -> Self {
        Self::Rejected { reason }
    }

    /// Returns whether this decision approved the presentation.
    #[must_use]
    pub const fn is_approved(self) -> bool {
        matches!(self, Self::Approved { .. })
    }

    /// Returns whether this decision rejected the presentation.
    #[must_use]
    pub const fn is_rejected(self) -> bool {
        matches!(self, Self::Rejected { .. })
    }

    /// Returns the charged amount for an approved decision.
    #[must_use]
    pub const fn charged_amount(self) -> Option<Money> {
        match self {
            Self::Approved { charged_amount, .. } => Some(charged_amount),
            Self::Rejected { .. } => None,
        }
    }
}

/// The lifecycle state of a fare transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum FareTransactionStatus {
    /// The transaction exists but has no fare decision.
    PendingDecision,

    /// A deterministic fare decision has been recorded.
    Decided,

    /// Backend reconciliation has completed.
    Reconciled,
}

impl fmt::Display for FareTransactionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            Self::PendingDecision => "pending decision",
            Self::Decided => "decided",
            Self::Reconciled => "reconciled",
        };

        formatter.write_str(status)
    }
}

/// Errors produced by fare-transaction lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FareTransactionError {
    /// A different decision was already recorded.
    #[error("fare transaction decision conflict: existing {existing:?}, requested {requested:?}")]
    DecisionConflict {
        /// Existing authoritative decision.
        existing: FareDecision,

        /// Conflicting requested decision.
        requested: FareDecision,
    },

    /// Reconciliation requires a completed fare decision.
    #[error("fare transaction cannot be reconciled while its status is {status}")]
    CannotReconcileUndecided {
        /// Current transaction status.
        status: FareTransactionStatus,
    },
}

/// The authoritative domain record produced from a simulated tap.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FareTransaction {
    id: FareTransactionId,
    transit_account_id: TransitAccountId,
    fare_credential_id: FareCredentialId,
    reader_id: ReaderId,
    local_sequence_number: LocalSequenceNumber,
    fare_policy_version: FarePolicyVersion,
    event_time: EventTime,
    processing_mode: FareProcessingMode,
    status: FareTransactionStatus,
    decision: Option<FareDecision>,
}

impl FareTransaction {
    /// Creates a transaction awaiting its fare decision.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new_pending(
        id: FareTransactionId,
        transit_account_id: TransitAccountId,
        fare_credential_id: FareCredentialId,
        reader_id: ReaderId,
        local_sequence_number: LocalSequenceNumber,
        fare_policy_version: FarePolicyVersion,
        event_time: EventTime,
        processing_mode: FareProcessingMode,
    ) -> Self {
        Self {
            id,
            transit_account_id,
            fare_credential_id,
            reader_id,
            local_sequence_number,
            fare_policy_version,
            event_time,
            processing_mode,
            status: FareTransactionStatus::PendingDecision,
            decision: None,
        }
    }

    /// Returns the transaction identifier.
    #[must_use]
    pub const fn id(&self) -> FareTransactionId {
        self.id
    }

    /// Returns the transit-account identifier.
    #[must_use]
    pub const fn transit_account_id(&self) -> TransitAccountId {
        self.transit_account_id
    }

    /// Returns the fare-credential identifier.
    #[must_use]
    pub const fn fare_credential_id(&self) -> FareCredentialId {
        self.fare_credential_id
    }

    /// Returns the reader identifier.
    #[must_use]
    pub const fn reader_id(&self) -> ReaderId {
        self.reader_id
    }

    /// Returns the reader-local sequence number.
    #[must_use]
    pub const fn local_sequence_number(&self) -> LocalSequenceNumber {
        self.local_sequence_number
    }

    /// Returns the fare-policy version used for processing.
    #[must_use]
    pub const fn fare_policy_version(&self) -> FarePolicyVersion {
        self.fare_policy_version
    }

    /// Returns the reader-reported event time.
    #[must_use]
    pub const fn event_time(&self) -> EventTime {
        self.event_time
    }

    /// Returns the processing mode.
    #[must_use]
    pub const fn processing_mode(&self) -> FareProcessingMode {
        self.processing_mode
    }

    /// Returns the transaction lifecycle status.
    #[must_use]
    pub const fn status(&self) -> FareTransactionStatus {
        self.status
    }

    /// Returns the authoritative fare decision.
    #[must_use]
    pub const fn decision(&self) -> Option<FareDecision> {
        self.decision
    }

    /// Returns the approved charged amount.
    #[must_use]
    pub const fn charged_amount(&self) -> Option<Money> {
        match self.decision {
            Some(decision) => decision.charged_amount(),
            None => None,
        }
    }

    /// Records the authoritative fare decision.
    ///
    /// Repeating the same decision is idempotent. A different decision is
    /// rejected so duplicate processing cannot silently change the result.
    pub fn record_decision(&mut self, decision: FareDecision) -> Result<(), FareTransactionError> {
        match self.decision {
            Some(existing) if existing == decision => Ok(()),
            Some(existing) => Err(FareTransactionError::DecisionConflict {
                existing,
                requested: decision,
            }),
            None => {
                self.decision = Some(decision);
                self.status = FareTransactionStatus::Decided;
                Ok(())
            }
        }
    }

    /// Marks a decided transaction as reconciled.
    ///
    /// Repeating reconciliation is idempotent.
    pub fn mark_reconciled(&mut self) -> Result<(), FareTransactionError> {
        match self.status {
            FareTransactionStatus::PendingDecision => {
                Err(FareTransactionError::CannotReconcileUndecided {
                    status: self.status,
                })
            }
            FareTransactionStatus::Decided => {
                self.status = FareTransactionStatus::Reconciled;
                Ok(())
            }
            FareTransactionStatus::Reconciled => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Currency, FareCredentialId, FareTransactionId, Money, ReaderId, TransitAccountId};

    use super::{
        EventTime, FareApprovalReason, FareDecision, FareDecisionError, FarePolicyVersion,
        FareProcessingMode, FareRejectionReason, FareTransaction, FareTransactionError,
        FareTransactionStatus, FareTransactionValueError, LocalSequenceNumber,
    };

    fn valid_sequence() -> LocalSequenceNumber {
        match LocalSequenceNumber::new(1) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid sequence failed: {error}")
            }
        }
    }

    fn valid_policy_version() -> FarePolicyVersion {
        match FarePolicyVersion::new(1) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid policy version failed: {error}")
            }
        }
    }

    fn valid_event_time() -> EventTime {
        match EventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid event time failed: {error}")
            }
        }
    }

    fn pending_transaction(mode: FareProcessingMode) -> FareTransaction {
        FareTransaction::new_pending(
            FareTransactionId::generate(),
            TransitAccountId::generate(),
            FareCredentialId::generate(),
            ReaderId::generate(),
            valid_sequence(),
            valid_policy_version(),
            valid_event_time(),
            mode,
        )
    }

    fn approved_decision(minor_units: i64) -> FareDecision {
        let result = FareDecision::approved(
            Money::from_minor_units(minor_units, Currency::Usd),
            FareApprovalReason::StandardFare,
        );

        match result {
            Ok(decision) => decision,
            Err(error) => {
                panic!("valid approval failed: {error}")
            }
        }
    }

    #[test]
    fn local_sequence_number_must_be_nonzero() {
        let result = LocalSequenceNumber::new(0);

        assert_eq!(
            result,
            Err(FareTransactionValueError::ZeroLocalSequenceNumber)
        );
    }

    #[test]
    fn fare_policy_version_must_be_nonzero() {
        let result = FarePolicyVersion::new(0);

        assert_eq!(
            result,
            Err(FareTransactionValueError::ZeroFarePolicyVersion)
        );
    }

    #[test]
    fn negative_event_time_is_rejected() {
        let result = EventTime::from_unix_milliseconds(-1);

        assert_eq!(
            result,
            Err(FareTransactionValueError::NegativeEventTime {
                unix_milliseconds: -1
            })
        );
    }

    #[test]
    fn new_transaction_is_pending_decision() {
        let transaction = pending_transaction(FareProcessingMode::Online);

        assert_eq!(transaction.status(), FareTransactionStatus::PendingDecision);
        assert_eq!(transaction.decision(), None);
        assert_eq!(transaction.charged_amount(), None);
    }

    #[test]
    fn negative_approved_amount_is_rejected() {
        let result = FareDecision::approved(
            Money::from_minor_units(-1, Currency::Usd),
            FareApprovalReason::StandardFare,
        );

        assert!(matches!(
            result,
            Err(FareDecisionError::NegativeApprovedAmount { .. })
        ));
    }

    #[test]
    fn zero_value_approval_is_valid() {
        let result =
            FareDecision::approved(Money::zero(Currency::Usd), FareApprovalReason::Transfer);

        assert!(matches!(
            result,
            Ok(decision)
                if decision.is_approved()
                    && decision.charged_amount()
                        == Some(Money::zero(Currency::Usd))
        ));
    }

    #[test]
    fn approved_decision_moves_transaction_to_decided() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);
        let decision = approved_decision(250);

        let result = transaction.record_decision(decision);

        assert!(result.is_ok());
        assert_eq!(transaction.status(), FareTransactionStatus::Decided);
        assert_eq!(transaction.decision(), Some(decision));
        assert_eq!(
            transaction.charged_amount(),
            Some(Money::from_minor_units(250, Currency::Usd))
        );
    }

    #[test]
    fn rejected_decision_records_no_charge() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);
        let decision = FareDecision::rejected(FareRejectionReason::CredentialRevoked);

        let result = transaction.record_decision(decision);

        assert!(result.is_ok());
        assert!(decision.is_rejected());
        assert_eq!(transaction.decision(), Some(decision));
        assert_eq!(transaction.charged_amount(), None);
    }

    #[test]
    fn repeated_same_decision_is_idempotent() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);
        let decision = approved_decision(250);

        assert!(transaction.record_decision(decision).is_ok());

        let result = transaction.record_decision(decision);

        assert!(result.is_ok());
        assert_eq!(transaction.decision(), Some(decision));
    }

    #[test]
    fn conflicting_decision_is_rejected() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);
        let existing = approved_decision(250);
        let requested = FareDecision::rejected(FareRejectionReason::ProductInvalid);

        assert!(transaction.record_decision(existing).is_ok());

        let result = transaction.record_decision(requested);

        assert_eq!(
            result,
            Err(FareTransactionError::DecisionConflict {
                existing,
                requested,
            })
        );
        assert_eq!(transaction.decision(), Some(existing));
    }

    #[test]
    fn pending_transaction_cannot_be_reconciled() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);

        let result = transaction.mark_reconciled();

        assert_eq!(
            result,
            Err(FareTransactionError::CannotReconcileUndecided {
                status: FareTransactionStatus::PendingDecision
            })
        );
    }

    #[test]
    fn decided_transaction_can_be_reconciled() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);

        assert!(transaction.record_decision(approved_decision(250)).is_ok());

        let result = transaction.mark_reconciled();

        assert!(result.is_ok());
        assert_eq!(transaction.status(), FareTransactionStatus::Reconciled);
    }

    #[test]
    fn reconciliation_is_idempotent() {
        let mut transaction = pending_transaction(FareProcessingMode::Online);

        assert!(transaction.record_decision(approved_decision(250)).is_ok());
        assert!(transaction.mark_reconciled().is_ok());

        let result = transaction.mark_reconciled();

        assert!(result.is_ok());
        assert_eq!(transaction.status(), FareTransactionStatus::Reconciled);
    }

    #[test]
    fn offline_processing_mode_is_preserved() {
        let transaction = pending_transaction(FareProcessingMode::Offline);

        assert_eq!(transaction.processing_mode(), FareProcessingMode::Offline);
    }
}
