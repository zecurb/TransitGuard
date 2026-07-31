use core::{future::Future, pin::Pin};

use transitguard_domain::{
    DomainEvent, DomainEventId, FareCredential, FareCredentialId, ReaderEquipment, ReaderId,
    TransitAccount, TransitAccountId,
};

use crate::{RepositoryError, SaveCondition, VersionedAggregate};

/// A boxed asynchronous repository operation.
pub type RepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

/// Application-facing persistence operations for transit accounts.
pub trait TransitAccountRepository: Send + Sync {
    /// Finds an account together with its persistence version.
    fn find_by_id(
        &self,
        account_id: TransitAccountId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<TransitAccount>>>;

    /// Saves an account while atomically enforcing the condition.
    fn save<'a>(
        &'a self,
        account: &'a TransitAccount,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()>;
}

/// Application-facing persistence operations for fare credentials.
pub trait FareCredentialRepository: Send + Sync {
    /// Finds a credential together with its persistence version.
    fn find_by_id(
        &self,
        credential_id: FareCredentialId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<FareCredential>>>;

    /// Saves a credential while atomically enforcing the condition.
    fn save<'a>(
        &'a self,
        credential: &'a FareCredential,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()>;
}

/// Application-facing persistence operations for reader equipment.
pub trait ReaderEquipmentRepository: Send + Sync {
    /// Finds reader equipment together with its persistence version.
    fn find_by_id(
        &self,
        reader_id: ReaderId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<ReaderEquipment>>>;

    /// Saves reader equipment while atomically enforcing the condition.
    fn save<'a>(
        &'a self,
        reader: &'a ReaderEquipment,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()>;
}

/// Persistence operations for immutable domain events.
pub trait DomainEventRepository: Send + Sync {
    /// Appends an immutable domain event.
    fn append<'a>(&'a self, event: &'a DomainEvent) -> RepositoryFuture<'a, ()>;
}

/// Generates fare-credential identifiers.
pub trait FareCredentialIdGenerator: Send + Sync {
    /// Generates the next fare-credential identifier.
    fn generate(&self) -> FareCredentialId;
}

/// Generates immutable domain-event identifiers.
pub trait DomainEventIdGenerator: Send + Sync {
    /// Generates the next domain-event identifier.
    fn generate(&self) -> DomainEventId;
}
