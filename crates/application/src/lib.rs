//! TransitGuard application services and infrastructure ports.
//!
//! This crate coordinates use cases through domain objects and abstract
//! infrastructure interfaces. It contains no HTTP handlers, SQL queries,
//! database-row types, filesystem configuration, or concrete telemetry.

pub mod clock;
pub mod error;
pub mod ports;
pub mod unit_of_work;
pub mod use_cases;
pub mod versioning;

pub use clock::Clock;

pub use error::{ApplicationError, BoxError, ClockError, RepositoryError};

pub use ports::{
    DomainEventIdGenerator, DomainEventRepository, FareCredentialIdGenerator,
    FareCredentialRepository, ReaderEquipmentRepository, RepositoryFuture,
    TransitAccountRepository,
};

pub use unit_of_work::{ApplicationTransaction, TransactionManager};

pub use use_cases::{
    ActivateFareCredentialCommand, ActivateFareCredentialService, ActivatedFareCredential,
    IssueFareCredentialCommand, IssueFareCredentialService, IssuedFareCredential,
    RevokeFareCredentialCommand, RevokeFareCredentialService, RevokedFareCredential,
    SuspendFareCredentialCommand, SuspendFareCredentialService, SuspendedFareCredential,
};

pub use versioning::{SaveCondition, VersionedAggregate};
