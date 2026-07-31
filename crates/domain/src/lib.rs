//! TransitGuard core domain model.
//!
//! This crate contains business concepts and invariants that remain
//! independent from transport protocols, databases, application
//! configuration, and concrete infrastructure.

pub mod account;
pub mod credential;
pub mod equipment;
pub mod event;
pub mod identifier;
pub mod money;
pub mod transaction;

pub use account::{
    EligibilityClassification, StoredValueBalance, StoredValueError, TransitAccount,
    TransitAccountError, TransitAccountStatus,
};

pub use credential::{
    FareCredential, FareCredentialError, FareCredentialKind, FareCredentialStatus, RevocationReason,
};

pub use equipment::{
    EquipmentIdentity, ReaderDisablementReason, ReaderEquipment, ReaderEquipmentError,
    ReaderEquipmentStatus, ReaderRevocationReason,
};

pub use identifier::{
    DomainEventId, EquipmentKeyId, FareCredentialId, FareTransactionId, IdentifierError, JourneyId,
    ReaderId, RiderId, SynchronizationBatchId, TransitAccountId, TransitProductId,
    TransitProductInstanceId,
};

pub use money::{Currency, Money, MoneyError, MoneyOperation};

pub use transaction::{
    EventTime, FareApprovalReason, FareDecision, FareDecisionError, FarePolicyVersion,
    FareProcessingMode, FareRejectionReason, FareTransaction, FareTransactionError,
    FareTransactionStatus, FareTransactionValueError, LocalSequenceNumber,
};

pub use event::{
    AggregateVersion, DomainAggregateId, DomainEvent, DomainEventError, DomainEventPayload,
    DomainEventTime, DomainEventValueError, StoredValueChangeReason,
};
