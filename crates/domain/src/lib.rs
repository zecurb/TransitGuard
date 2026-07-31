//! TransitGuard core domain model.
//!
//! This crate contains business concepts and invariants that remain
//! independent from transport protocols, databases, application
//! configuration, and concrete infrastructure.

pub mod account;
pub mod credential;
pub mod equipment;
pub mod identifier;
pub mod money;

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
