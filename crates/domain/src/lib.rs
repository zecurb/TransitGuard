//! TransitGuard core domain model.
//!
//! This crate contains business concepts and invariants that remain
//! independent from transport protocols, databases, application
//! configuration, and concrete infrastructure.

pub mod identifier;
pub mod money;

pub use identifier::{
    DomainEventId, FareCredentialId, FareTransactionId, IdentifierError, JourneyId, ReaderId,
    RiderId, SynchronizationBatchId, TransitAccountId, TransitProductId, TransitProductInstanceId,
};

pub use money::{Currency, Money, MoneyError, MoneyOperation};
