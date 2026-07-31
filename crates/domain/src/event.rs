use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::{
    Currency, DomainEventId, EquipmentKeyId, FareCredentialId, FareCredentialKind,
    FareCredentialStatus, FareDecision, FareTransactionId, ReaderEquipmentStatus, ReaderId,
    RiderId, StoredValueBalance, TransitAccountId, TransitAccountStatus,
};

/// Errors produced while constructing domain-event value objects.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DomainEventValueError {
    /// Aggregate versions begin at one.
    #[error("aggregate version must be greater than zero")]
    ZeroAggregateVersion,

    /// Domain-event time cannot be before the Unix epoch.
    #[error("domain-event time cannot be negative: {unix_milliseconds}")]
    NegativeDomainEventTime {
        /// Invalid Unix timestamp in milliseconds.
        unix_milliseconds: i64,
    },
}

/// The ordered version of an aggregate after a domain change.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AggregateVersion(u64);

impl AggregateVersion {
    /// Creates a validated aggregate version.
    pub const fn new(value: u64) -> Result<Self, DomainEventValueError> {
        if value == 0 {
            return Err(DomainEventValueError::ZeroAggregateVersion);
        }

        Ok(Self(value))
    }

    /// Returns the numeric aggregate version.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for AggregateVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// The time at which a domain change became authoritative.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DomainEventTime(i64);

impl DomainEventTime {
    /// Creates a domain-event time from Unix milliseconds.
    pub const fn from_unix_milliseconds(
        unix_milliseconds: i64,
    ) -> Result<Self, DomainEventValueError> {
        if unix_milliseconds < 0 {
            return Err(DomainEventValueError::NegativeDomainEventTime { unix_milliseconds });
        }

        Ok(Self(unix_milliseconds))
    }

    /// Returns Unix milliseconds.
    #[must_use]
    pub const fn unix_milliseconds(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DomainEventTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;

        Self::from_unix_milliseconds(value).map_err(serde::de::Error::custom)
    }
}

/// The aggregate that owns a domain event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DomainAggregateId {
    /// A transit-account aggregate.
    TransitAccount(TransitAccountId),

    /// A fare-credential aggregate.
    FareCredential(FareCredentialId),

    /// A reader-equipment aggregate.
    ReaderEquipment(ReaderId),

    /// A fare-transaction aggregate.
    FareTransaction(FareTransactionId),
}

impl fmt::Display for DomainAggregateId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransitAccount(id) => {
                write!(formatter, "transit-account:{id}")
            }
            Self::FareCredential(id) => {
                write!(formatter, "fare-credential:{id}")
            }
            Self::ReaderEquipment(id) => {
                write!(formatter, "reader-equipment:{id}")
            }
            Self::FareTransaction(id) => {
                write!(formatter, "fare-transaction:{id}")
            }
        }
    }
}

/// The business reason stored value changed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum StoredValueChangeReason {
    /// Project-owned simulated funds were credited.
    SimulatedFundsCredit,

    /// A fare charge was debited.
    FareDebit,

    /// An authorized administrative adjustment was applied.
    AdministrativeAdjustment,

    /// A reconciliation correction was applied.
    ReconciliationAdjustment,
}

/// The immutable business fact carried by a domain event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DomainEventPayload {
    /// A transit account was created.
    TransitAccountCreated {
        /// New transit-account identifier.
        account_id: TransitAccountId,

        /// Rider associated with the account.
        rider_id: RiderId,

        /// Validated initial stored-value balance.
        initial_balance: StoredValueBalance,
    },

    /// A transit-account status changed.
    TransitAccountStatusChanged {
        /// Transit-account identifier.
        account_id: TransitAccountId,

        /// Status before the change.
        previous_status: TransitAccountStatus,

        /// Status after the change.
        current_status: TransitAccountStatus,
    },

    /// A stored-value balance changed.
    StoredValueChanged {
        /// Transit-account identifier.
        account_id: TransitAccountId,

        /// Balance before the change.
        previous_balance: StoredValueBalance,

        /// Balance after the change.
        current_balance: StoredValueBalance,

        /// Business reason for the change.
        reason: StoredValueChangeReason,
    },

    /// A project-owned fare credential was issued.
    FareCredentialIssued {
        /// New fare-credential identifier.
        credential_id: FareCredentialId,

        /// Associated transit-account identifier.
        account_id: TransitAccountId,

        /// Credential representation.
        kind: FareCredentialKind,
    },

    /// A fare-credential status changed.
    FareCredentialStatusChanged {
        /// Fare-credential identifier.
        credential_id: FareCredentialId,

        /// Status before the change.
        previous_status: FareCredentialStatus,

        /// Status after the change.
        current_status: FareCredentialStatus,
    },

    /// Project-owned reader equipment completed registration.
    ReaderEquipmentRegistered {
        /// Registered reader identifier.
        reader_id: ReaderId,

        /// Public equipment-key identifier.
        equipment_key_id: EquipmentKeyId,
    },

    /// Reader-equipment status changed.
    ReaderEquipmentStatusChanged {
        /// Reader identifier.
        reader_id: ReaderId,

        /// Status before the change.
        previous_status: ReaderEquipmentStatus,

        /// Status after the change.
        current_status: ReaderEquipmentStatus,
    },

    /// A reader's public equipment-key identifier changed.
    EquipmentKeyRotated {
        /// Reader identifier.
        reader_id: ReaderId,

        /// Public key identifier before rotation.
        previous_key_id: EquipmentKeyId,

        /// Public key identifier after rotation.
        current_key_id: EquipmentKeyId,
    },

    /// An authoritative fare decision was recorded.
    FareTransactionDecided {
        /// Fare-transaction identifier.
        transaction_id: FareTransactionId,

        /// Authoritative fare decision.
        decision: FareDecision,
    },

    /// Backend reconciliation completed for a fare transaction.
    FareTransactionReconciled {
        /// Fare-transaction identifier.
        transaction_id: FareTransactionId,
    },
}

impl DomainEventPayload {
    /// Returns the stable machine-readable event name.
    #[must_use]
    pub const fn event_name(self) -> &'static str {
        match self {
            Self::TransitAccountCreated { .. } => "transit_account.created",
            Self::TransitAccountStatusChanged { .. } => "transit_account.status_changed",
            Self::StoredValueChanged { .. } => "transit_account.stored_value_changed",
            Self::FareCredentialIssued { .. } => "fare_credential.issued",
            Self::FareCredentialStatusChanged { .. } => "fare_credential.status_changed",
            Self::ReaderEquipmentRegistered { .. } => "reader_equipment.registered",
            Self::ReaderEquipmentStatusChanged { .. } => "reader_equipment.status_changed",
            Self::EquipmentKeyRotated { .. } => "reader_equipment.key_rotated",
            Self::FareTransactionDecided { .. } => "fare_transaction.decided",
            Self::FareTransactionReconciled { .. } => "fare_transaction.reconciled",
        }
    }

    /// Returns the aggregate that owns this event.
    #[must_use]
    pub const fn aggregate_id(self) -> DomainAggregateId {
        match self {
            Self::TransitAccountCreated { account_id, .. }
            | Self::TransitAccountStatusChanged { account_id, .. }
            | Self::StoredValueChanged { account_id, .. } => {
                DomainAggregateId::TransitAccount(account_id)
            }

            Self::FareCredentialIssued { credential_id, .. }
            | Self::FareCredentialStatusChanged { credential_id, .. } => {
                DomainAggregateId::FareCredential(credential_id)
            }

            Self::ReaderEquipmentRegistered { reader_id, .. }
            | Self::ReaderEquipmentStatusChanged { reader_id, .. }
            | Self::EquipmentKeyRotated { reader_id, .. } => {
                DomainAggregateId::ReaderEquipment(reader_id)
            }

            Self::FareTransactionDecided { transaction_id, .. }
            | Self::FareTransactionReconciled { transaction_id } => {
                DomainAggregateId::FareTransaction(transaction_id)
            }
        }
    }
}

/// Errors produced while creating authoritative domain events.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DomainEventError {
    /// A status-change event did not contain an actual change.
    #[error("{event_name} must record a real status change")]
    NoStatusChange {
        /// Stable event name.
        event_name: &'static str,
    },

    /// A balance-change event did not contain an actual change.
    #[error("stored-value event must record a real balance change")]
    NoStoredValueChange,

    /// A balance-change event attempted to change currency.
    #[error("stored-value event cannot change currency from {previous} to {current}")]
    StoredValueCurrencyChanged {
        /// Previous balance currency.
        previous: Currency,

        /// Current balance currency.
        current: Currency,
    },

    /// A key-rotation event did not contain a different key identifier.
    #[error("equipment-key rotation must change the key identifier")]
    NoEquipmentKeyChange,
}

/// An immutable, versioned domain-event envelope.
///
/// The aggregate identifier is derived from the payload and cannot disagree
/// with the business fact contained in the event.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct DomainEvent {
    id: DomainEventId,
    aggregate_id: DomainAggregateId,
    aggregate_version: AggregateVersion,
    occurred_at: DomainEventTime,
    payload: DomainEventPayload,
}

impl DomainEvent {
    /// Creates a validated domain event.
    pub fn new(
        id: DomainEventId,
        aggregate_version: AggregateVersion,
        occurred_at: DomainEventTime,
        payload: DomainEventPayload,
    ) -> Result<Self, DomainEventError> {
        Self::validate_payload(payload)?;

        Ok(Self {
            id,
            aggregate_id: payload.aggregate_id(),
            aggregate_version,
            occurred_at,
            payload,
        })
    }

    /// Returns the event identifier.
    #[must_use]
    pub const fn id(self) -> DomainEventId {
        self.id
    }

    /// Returns the owning aggregate identifier.
    #[must_use]
    pub const fn aggregate_id(self) -> DomainAggregateId {
        self.aggregate_id
    }

    /// Returns the aggregate version after this event.
    #[must_use]
    pub const fn aggregate_version(self) -> AggregateVersion {
        self.aggregate_version
    }

    /// Returns when the event became authoritative.
    #[must_use]
    pub const fn occurred_at(self) -> DomainEventTime {
        self.occurred_at
    }

    /// Returns the immutable business fact.
    #[must_use]
    pub const fn payload(self) -> DomainEventPayload {
        self.payload
    }

    /// Returns the stable machine-readable event name.
    #[must_use]
    pub const fn event_name(self) -> &'static str {
        self.payload.event_name()
    }

    fn validate_payload(payload: DomainEventPayload) -> Result<(), DomainEventError> {
        match payload {
            DomainEventPayload::TransitAccountStatusChanged {
                previous_status,
                current_status,
                ..
            } if previous_status == current_status => Err(DomainEventError::NoStatusChange {
                event_name: payload.event_name(),
            }),

            DomainEventPayload::FareCredentialStatusChanged {
                previous_status,
                current_status,
                ..
            } if previous_status == current_status => Err(DomainEventError::NoStatusChange {
                event_name: payload.event_name(),
            }),

            DomainEventPayload::ReaderEquipmentStatusChanged {
                previous_status,
                current_status,
                ..
            } if previous_status == current_status => Err(DomainEventError::NoStatusChange {
                event_name: payload.event_name(),
            }),

            DomainEventPayload::StoredValueChanged {
                previous_balance,
                current_balance,
                ..
            } => {
                if previous_balance.currency() != current_balance.currency() {
                    return Err(DomainEventError::StoredValueCurrencyChanged {
                        previous: previous_balance.currency(),
                        current: current_balance.currency(),
                    });
                }

                if previous_balance == current_balance {
                    return Err(DomainEventError::NoStoredValueChange);
                }

                Ok(())
            }

            DomainEventPayload::EquipmentKeyRotated {
                previous_key_id,
                current_key_id,
                ..
            } if previous_key_id == current_key_id => Err(DomainEventError::NoEquipmentKeyChange),

            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Currency, DomainEventId, EquipmentKeyId, FareTransactionId, Money, ReaderEquipmentStatus,
        ReaderId, StoredValueBalance, TransitAccountId, TransitAccountStatus,
    };

    use super::{
        AggregateVersion, DomainAggregateId, DomainEvent, DomainEventError, DomainEventPayload,
        DomainEventTime, DomainEventValueError, StoredValueChangeReason,
    };

    fn version() -> AggregateVersion {
        match AggregateVersion::new(1) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid aggregate version failed: {error}")
            }
        }
    }

    fn event_time() -> DomainEventTime {
        match DomainEventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid domain-event time failed: {error}")
            }
        }
    }

    fn balance(minor_units: i64, currency: Currency) -> StoredValueBalance {
        match StoredValueBalance::new(Money::from_minor_units(minor_units, currency)) {
            Ok(value) => value,
            Err(error) => {
                panic!("valid balance failed: {error}")
            }
        }
    }

    #[test]
    fn aggregate_version_must_be_nonzero() {
        assert_eq!(
            AggregateVersion::new(0),
            Err(DomainEventValueError::ZeroAggregateVersion)
        );
    }

    #[test]
    fn negative_domain_event_time_is_rejected() {
        assert_eq!(
            DomainEventTime::from_unix_milliseconds(-1),
            Err(DomainEventValueError::NegativeDomainEventTime {
                unix_milliseconds: -1
            })
        );
    }

    #[test]
    fn aggregate_identifier_is_derived_from_payload() {
        let account_id = TransitAccountId::generate();
        let payload = DomainEventPayload::TransitAccountStatusChanged {
            account_id,
            previous_status: TransitAccountStatus::Active,
            current_status: TransitAccountStatus::Suspended,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert!(matches!(
            result,
            Ok(event)
                if event.aggregate_id()
                    == DomainAggregateId::TransitAccount(
                        account_id
                    )
        ));
    }

    #[test]
    fn event_name_is_stable() {
        let payload = DomainEventPayload::FareTransactionReconciled {
            transaction_id: FareTransactionId::generate(),
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert!(matches!(
            result,
            Ok(event)
                if event.event_name()
                    == "fare_transaction.reconciled"
        ));
    }

    #[test]
    fn account_status_event_requires_real_change() {
        let payload = DomainEventPayload::TransitAccountStatusChanged {
            account_id: TransitAccountId::generate(),
            previous_status: TransitAccountStatus::Active,
            current_status: TransitAccountStatus::Active,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert!(matches!(
            result,
            Err(DomainEventError::NoStatusChange {
                event_name: "transit_account.status_changed"
            })
        ));
    }

    #[test]
    fn reader_status_event_requires_real_change() {
        let payload = DomainEventPayload::ReaderEquipmentStatusChanged {
            reader_id: ReaderId::generate(),
            previous_status: ReaderEquipmentStatus::Disabled,
            current_status: ReaderEquipmentStatus::Disabled,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert!(matches!(
            result,
            Err(DomainEventError::NoStatusChange {
                event_name: "reader_equipment.status_changed"
            })
        ));
    }

    #[test]
    fn stored_value_event_requires_real_change() {
        let unchanged = balance(1_000, Currency::Usd);
        let payload = DomainEventPayload::StoredValueChanged {
            account_id: TransitAccountId::generate(),
            previous_balance: unchanged,
            current_balance: unchanged,
            reason: StoredValueChangeReason::FareDebit,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert_eq!(result, Err(DomainEventError::NoStoredValueChange));
    }

    #[test]
    fn stored_value_event_cannot_change_currency() {
        let payload = DomainEventPayload::StoredValueChanged {
            account_id: TransitAccountId::generate(),
            previous_balance: balance(1_000, Currency::Usd),
            current_balance: balance(1_000, Currency::Eur),
            reason: StoredValueChangeReason::AdministrativeAdjustment,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert_eq!(
            result,
            Err(DomainEventError::StoredValueCurrencyChanged {
                previous: Currency::Usd,
                current: Currency::Eur,
            })
        );
    }

    #[test]
    fn equipment_key_rotation_requires_new_key() {
        let key_id = EquipmentKeyId::generate();
        let payload = DomainEventPayload::EquipmentKeyRotated {
            reader_id: ReaderId::generate(),
            previous_key_id: key_id,
            current_key_id: key_id,
        };

        let result = DomainEvent::new(DomainEventId::generate(), version(), event_time(), payload);

        assert_eq!(result, Err(DomainEventError::NoEquipmentKeyChange));
    }
}
