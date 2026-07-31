use transitguard_domain::{
    DomainEvent, FareCredential, FareCredentialId, ReaderEquipment, ReaderId, TransitAccount,
    TransitAccountId,
};

use crate::RepositoryFuture;

/// A transaction-scoped set of application persistence operations.
///
/// Implementations must keep changes provisional until `commit` succeeds.
/// Calling `rollback`, or dropping an implementation without committing,
/// must prevent provisional changes from becoming durable.
pub trait ApplicationTransaction: Send {
    /// Finds a transit account inside the current transaction.
    fn find_transit_account(
        &mut self,
        account_id: TransitAccountId,
    ) -> RepositoryFuture<'_, Option<TransitAccount>>;

    /// Saves a transit account inside the current transaction.
    fn save_transit_account<'a>(
        &'a mut self,
        account: &'a TransitAccount,
    ) -> RepositoryFuture<'a, ()>;

    /// Finds a fare credential inside the current transaction.
    fn find_fare_credential(
        &mut self,
        credential_id: FareCredentialId,
    ) -> RepositoryFuture<'_, Option<FareCredential>>;

    /// Saves a fare credential inside the current transaction.
    fn save_fare_credential<'a>(
        &'a mut self,
        credential: &'a FareCredential,
    ) -> RepositoryFuture<'a, ()>;

    /// Finds reader equipment inside the current transaction.
    fn find_reader_equipment(
        &mut self,
        reader_id: ReaderId,
    ) -> RepositoryFuture<'_, Option<ReaderEquipment>>;

    /// Saves reader equipment inside the current transaction.
    fn save_reader_equipment<'a>(
        &'a mut self,
        reader: &'a ReaderEquipment,
    ) -> RepositoryFuture<'a, ()>;

    /// Appends an immutable event inside the current transaction.
    fn append_domain_event<'a>(&'a mut self, event: &'a DomainEvent) -> RepositoryFuture<'a, ()>;

    /// Atomically makes all transaction changes durable.
    fn commit(self: Box<Self>) -> RepositoryFuture<'static, ()>;

    /// Discards all provisional transaction changes.
    fn rollback(self: Box<Self>) -> RepositoryFuture<'static, ()>;
}

/// Starts application persistence transactions.
pub trait TransactionManager: Send + Sync {
    /// Starts a new application transaction.
    fn begin(&self) -> RepositoryFuture<'_, Box<dyn ApplicationTransaction>>;
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        DomainEvent, FareCredential, FareCredentialId, ReaderEquipment, ReaderId, TransitAccount,
        TransitAccountId,
    };

    use crate::RepositoryFuture;

    use super::{ApplicationTransaction, TransactionManager};

    struct NoopTransaction;

    impl ApplicationTransaction for NoopTransaction {
        fn find_transit_account(
            &mut self,
            _account_id: TransitAccountId,
        ) -> RepositoryFuture<'_, Option<TransitAccount>> {
            Box::pin(async { Ok(None) })
        }

        fn save_transit_account<'a>(
            &'a mut self,
            _account: &'a TransitAccount,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn find_fare_credential(
            &mut self,
            _credential_id: FareCredentialId,
        ) -> RepositoryFuture<'_, Option<FareCredential>> {
            Box::pin(async { Ok(None) })
        }

        fn save_fare_credential<'a>(
            &'a mut self,
            _credential: &'a FareCredential,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn find_reader_equipment(
            &mut self,
            _reader_id: ReaderId,
        ) -> RepositoryFuture<'_, Option<ReaderEquipment>> {
            Box::pin(async { Ok(None) })
        }

        fn save_reader_equipment<'a>(
            &'a mut self,
            _reader: &'a ReaderEquipment,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn append_domain_event<'a>(
            &'a mut self,
            _event: &'a DomainEvent,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn commit(self: Box<Self>) -> RepositoryFuture<'static, ()> {
            Box::pin(async { Ok(()) })
        }

        fn rollback(self: Box<Self>) -> RepositoryFuture<'static, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    struct NoopTransactionManager;

    impl TransactionManager for NoopTransactionManager {
        fn begin(&self) -> RepositoryFuture<'_, Box<dyn ApplicationTransaction>> {
            Box::pin(async { Ok(Box::new(NoopTransaction) as Box<dyn ApplicationTransaction>) })
        }
    }

    #[test]
    fn transaction_manager_trait_is_object_safe() {
        let manager = NoopTransactionManager;
        let object: &dyn TransactionManager = &manager;

        drop(object.begin());
    }

    #[test]
    fn application_transaction_trait_is_object_safe() {
        let transaction: Box<dyn ApplicationTransaction> = Box::new(NoopTransaction);

        drop(transaction.rollback());
    }
}
