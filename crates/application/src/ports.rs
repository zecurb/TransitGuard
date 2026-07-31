use core::{
    future::Future,
    pin::Pin,
};

use transitguard_domain::{
    DomainEvent,
    DomainEventId,
    FareCredential,
    FareCredentialId,
    ReaderEquipment,
    ReaderId,
    TransitAccount,
    TransitAccountId,
};

use crate::RepositoryError;

/// A boxed asynchronous repository operation.
///
/// Boxing keeps repository ports object-safe so application services can use
/// runtime-selected PostgreSQL, in-memory, or test implementations.
pub type RepositoryFuture<'a, T> = Pin<
    Box<
        dyn Future<
                Output = Result<T, RepositoryError>,
            > + Send
            + 'a,
    >,
>;

/// Application-facing persistence operations for transit accounts.
pub trait TransitAccountRepository: Send + Sync {
    /// Finds a transit account by its strongly typed identifier.
    fn find_by_id(
        &self,
        account_id: TransitAccountId,
    ) -> RepositoryFuture<'_, Option<TransitAccount>>;

    /// Persists the complete current account state.
    fn save<'a>(
        &'a self,
        account: &'a TransitAccount,
    ) -> RepositoryFuture<'a, ()>;
}

/// Application-facing persistence operations for fare credentials.
pub trait FareCredentialRepository: Send + Sync {
    /// Finds a fare credential by its strongly typed identifier.
    fn find_by_id(
        &self,
        credential_id: FareCredentialId,
    ) -> RepositoryFuture<'_, Option<FareCredential>>;

    /// Persists the complete current credential state.
    fn save<'a>(
        &'a self,
        credential: &'a FareCredential,
    ) -> RepositoryFuture<'a, ()>;
}

/// Application-facing persistence operations for reader equipment.
pub trait ReaderEquipmentRepository: Send + Sync {
    /// Finds reader equipment by its strongly typed identifier.
    fn find_by_id(
        &self,
        reader_id: ReaderId,
    ) -> RepositoryFuture<'_, Option<ReaderEquipment>>;

    /// Persists the complete current reader state.
    fn save<'a>(
        &'a self,
        reader: &'a ReaderEquipment,
    ) -> RepositoryFuture<'a, ()>;
}

/// Persistence operations for immutable domain events.
pub trait DomainEventRepository: Send + Sync {
    /// Appends an immutable domain event.
    fn append<'a>(
        &'a self,
        event: &'a DomainEvent,
    ) -> RepositoryFuture<'a, ()>;
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
