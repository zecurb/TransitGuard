//! TransitGuard application services and infrastructure ports.
//!
//! This crate coordinates use cases through domain objects and abstract
//! infrastructure interfaces. It contains no HTTP handlers, SQL queries,
//! database-row types, filesystem configuration, or concrete telemetry.

pub mod error;
pub mod ports;

pub use error::{
    ApplicationError,
    BoxError,
    RepositoryError,
};

pub use ports::{
    DomainEventIdGenerator,
    DomainEventRepository,
    FareCredentialIdGenerator,
    FareCredentialRepository,
    ReaderEquipmentRepository,
    RepositoryFuture,
    TransitAccountRepository,
};
