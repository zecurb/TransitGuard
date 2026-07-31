use super::support::rollback_with;

use transitguard_domain::{
    AggregateVersion, DomainEvent, DomainEventPayload, FareCredential, FareCredentialId,
    FareCredentialKind, FareCredentialStatus, TransitAccountId,
};

use crate::{
    ApplicationError, Clock, DomainEventIdGenerator, FareCredentialIdGenerator, SaveCondition,
    TransactionManager,
};

/// Input for issuing a project-owned fare credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IssueFareCredentialCommand {
    /// Account that will own the credential.
    pub transit_account_id: TransitAccountId,

    /// Credential representation to issue.
    pub kind: FareCredentialKind,
}

/// Stable application result returned after successful issuance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IssuedFareCredential {
    credential_id: FareCredentialId,
    transit_account_id: TransitAccountId,
    kind: FareCredentialKind,
    status: FareCredentialStatus,
}

impl IssuedFareCredential {
    /// Returns the newly issued credential identifier.
    #[must_use]
    pub const fn credential_id(self) -> FareCredentialId {
        self.credential_id
    }

    /// Returns the owning transit-account identifier.
    #[must_use]
    pub const fn transit_account_id(self) -> TransitAccountId {
        self.transit_account_id
    }

    /// Returns the credential representation.
    #[must_use]
    pub const fn kind(self) -> FareCredentialKind {
        self.kind
    }

    /// Returns the initial credential status.
    #[must_use]
    pub const fn status(self) -> FareCredentialStatus {
        self.status
    }
}

/// Coordinates fare-credential issuance.
///
/// Credential persistence and event persistence occur inside the same
/// application transaction.
pub struct IssueFareCredentialService<'a> {
    transaction_manager: &'a dyn TransactionManager,
    clock: &'a dyn Clock,
    credential_id_generator: &'a dyn FareCredentialIdGenerator,
    event_id_generator: &'a dyn DomainEventIdGenerator,
}

impl<'a> IssueFareCredentialService<'a> {
    /// Creates the issuance service from application ports.
    #[must_use]
    pub const fn new(
        transaction_manager: &'a dyn TransactionManager,
        clock: &'a dyn Clock,
        credential_id_generator: &'a dyn FareCredentialIdGenerator,
        event_id_generator: &'a dyn DomainEventIdGenerator,
    ) -> Self {
        Self {
            transaction_manager,
            clock,
            credential_id_generator,
            event_id_generator,
        }
    }

    /// Issues a pending fare credential for an active account.
    pub async fn execute(
        &self,
        command: IssueFareCredentialCommand,
    ) -> Result<IssuedFareCredential, ApplicationError> {
        let occurred_at = self.clock.now()?;
        let mut transaction = self.transaction_manager.begin().await?;

        let account = match transaction
            .find_transit_account(command.transit_account_id)
            .await
        {
            Ok(Some(account)) => account,

            Ok(None) => {
                let error = ApplicationError::not_found(
                    "transit account",
                    command.transit_account_id.to_string(),
                );

                return rollback_with(transaction, error).await;
            }

            Err(error) => {
                return rollback_with(transaction, ApplicationError::from(error)).await;
            }
        };

        if !account.aggregate().is_active() {
            let error = ApplicationError::conflict(
                "issue fare credential",
                "transit account is not active",
            );

            return rollback_with(transaction, error).await;
        }

        let credential_id = self.credential_id_generator.generate();

        let credential =
            FareCredential::new_pending(credential_id, command.transit_account_id, command.kind);

        let aggregate_version = match AggregateVersion::new(1) {
            Ok(version) => version,
            Err(_) => {
                let error = ApplicationError::conflict(
                    "issue fare credential",
                    "initial aggregate version is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        let payload = DomainEventPayload::FareCredentialIssued {
            credential_id,
            account_id: command.transit_account_id,
            kind: command.kind,
        };

        let event = match DomainEvent::new(
            self.event_id_generator.generate(),
            aggregate_version,
            occurred_at,
            payload,
        ) {
            Ok(event) => event,

            Err(_) => {
                let error = ApplicationError::conflict(
                    "issue fare credential",
                    "credential issuance event is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        if let Err(error) = transaction
            .save_fare_credential(&credential, SaveCondition::MustNotExist)
            .await
        {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        if let Err(error) = transaction.append_domain_event(&event).await {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        transaction.commit().await?;

        Ok(IssuedFareCredential {
            credential_id,
            transit_account_id: command.transit_account_id,
            kind: command.kind,
            status: credential.status(),
        })
    }
}

#[cfg(test)]
mod tests {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    use std::{
        sync::{Arc, Mutex, MutexGuard},
        thread,
    };

    use thiserror::Error;
    use transitguard_domain::{
        AggregateVersion, Currency, DomainEvent, DomainEventId, DomainEventPayload,
        DomainEventTime, EligibilityClassification, FareCredential, FareCredentialId,
        FareCredentialKind, FareCredentialStatus, Money, ReaderEquipment, ReaderId, RiderId,
        TransitAccount, TransitAccountId,
    };

    use crate::{
        ApplicationError, ApplicationTransaction, Clock, DomainEventIdGenerator,
        FareCredentialIdGenerator, RepositoryError, RepositoryFuture, SaveCondition,
        TransactionManager, VersionedAggregate,
    };

    use super::{IssueFareCredentialCommand, IssueFareCredentialService};

    #[derive(Default)]
    struct FakeState {
        account: Option<TransitAccount>,
        saved_credential: Option<FareCredential>,
        events: Vec<DomainEvent>,
        commits: u32,
        rollbacks: u32,
        fail_credential_save: bool,
    }

    struct FakeTransaction {
        state: Arc<Mutex<FakeState>>,
    }

    struct FakeTransactionManager {
        state: Arc<Mutex<FakeState>>,
    }

    #[derive(Debug, Error)]
    #[error("fake storage operation failed")]
    struct FakeStorageError;

    fn repository_error(operation: &'static str) -> RepositoryError {
        RepositoryError::new("fake application transaction", operation, FakeStorageError)
    }

    impl ApplicationTransaction for FakeTransaction {
        fn find_transit_account(
            &mut self,
            account_id: TransitAccountId,
        ) -> RepositoryFuture<'_, Option<VersionedAggregate<TransitAccount>>> {
            let result = match self.state.lock() {
                Ok(state) => Ok(state
                    .account
                    .as_ref()
                    .filter(|account| account.id() == account_id)
                    .cloned()
                    .map(|account| VersionedAggregate::new(account, initial_version()))),

                Err(_) => Err(repository_error("find transit account")),
            };

            Box::pin(async move { result })
        }

        fn save_transit_account<'a>(
            &'a mut self,
            account: &'a TransitAccount,
            _condition: SaveCondition,
        ) -> RepositoryFuture<'a, ()> {
            let account = account.clone();
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        state.account = Some(account);
                        Ok(())
                    }

                    Err(_) => Err(repository_error("save transit account")),
                }
            })
        }

        fn find_fare_credential(
            &mut self,
            credential_id: FareCredentialId,
        ) -> RepositoryFuture<'_, Option<VersionedAggregate<FareCredential>>> {
            let result = match self.state.lock() {
                Ok(state) => Ok(state
                    .saved_credential
                    .as_ref()
                    .filter(|credential| credential.id() == credential_id)
                    .cloned()
                    .map(|credential| VersionedAggregate::new(credential, initial_version()))),

                Err(_) => Err(repository_error("find fare credential")),
            };

            Box::pin(async move { result })
        }

        fn save_fare_credential<'a>(
            &'a mut self,
            credential: &'a FareCredential,
            _condition: SaveCondition,
        ) -> RepositoryFuture<'a, ()> {
            let credential = credential.clone();
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        if state.fail_credential_save {
                            return Err(repository_error("save fare credential"));
                        }

                        state.saved_credential = Some(credential);
                        Ok(())
                    }

                    Err(_) => Err(repository_error("save fare credential")),
                }
            })
        }

        fn find_reader_equipment(
            &mut self,
            _reader_id: ReaderId,
        ) -> RepositoryFuture<'_, Option<VersionedAggregate<ReaderEquipment>>> {
            Box::pin(async { Ok(None) })
        }

        fn save_reader_equipment<'a>(
            &'a mut self,
            _reader: &'a ReaderEquipment,
            _condition: SaveCondition,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn append_domain_event<'a>(
            &'a mut self,
            event: &'a DomainEvent,
        ) -> RepositoryFuture<'a, ()> {
            let event = *event;
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        state.events.push(event);
                        Ok(())
                    }

                    Err(_) => Err(repository_error("append domain event")),
                }
            })
        }

        fn commit(self: Box<Self>) -> RepositoryFuture<'static, ()> {
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        state.commits += 1;
                        Ok(())
                    }

                    Err(_) => Err(repository_error("commit")),
                }
            })
        }

        fn rollback(self: Box<Self>) -> RepositoryFuture<'static, ()> {
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        state.saved_credential = None;
                        state.events.clear();
                        state.rollbacks += 1;
                        Ok(())
                    }

                    Err(_) => Err(repository_error("rollback")),
                }
            })
        }
    }

    impl TransactionManager for FakeTransactionManager {
        fn begin(&self) -> RepositoryFuture<'_, Box<dyn ApplicationTransaction>> {
            let transaction = FakeTransaction {
                state: Arc::clone(&self.state),
            };

            Box::pin(async move { Ok(Box::new(transaction) as Box<dyn ApplicationTransaction>) })
        }
    }

    struct FixedClock {
        time: DomainEventTime,
    }

    impl Clock for FixedClock {
        fn now(&self) -> Result<DomainEventTime, crate::ClockError> {
            Ok(self.time)
        }
    }

    struct FixedCredentialIdGenerator {
        id: FareCredentialId,
    }

    impl FareCredentialIdGenerator for FixedCredentialIdGenerator {
        fn generate(&self) -> FareCredentialId {
            self.id
        }
    }

    struct FixedEventIdGenerator {
        id: DomainEventId,
    }

    impl DomainEventIdGenerator for FixedEventIdGenerator {
        fn generate(&self) -> DomainEventId {
            self.id
        }
    }

    fn initial_version() -> AggregateVersion {
        match AggregateVersion::new(1) {
            Ok(version) => version,
            Err(error) => {
                panic!("valid initial aggregate version failed: {error}")
            }
        }
    }

    fn run_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => {
                    return output;
                }

                Poll::Pending => {
                    thread::yield_now();
                }
            }
        }
    }

    fn lock_state<'a>(state: &'a Arc<Mutex<FakeState>>) -> MutexGuard<'a, FakeState> {
        match state.lock() {
            Ok(state) => state,
            Err(_) => {
                panic!("fake state lock was poisoned")
            }
        }
    }

    fn event_time() -> DomainEventTime {
        match DomainEventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(time) => time,
            Err(error) => {
                panic!("valid event time failed: {error}")
            }
        }
    }

    fn active_account(account_id: TransitAccountId) -> TransitAccount {
        match TransitAccount::new(
            account_id,
            RiderId::generate(),
            EligibilityClassification::Standard,
            Money::from_minor_units(2_000, Currency::Usd),
        ) {
            Ok(account) => account,
            Err(error) => {
                panic!("valid account failed: {error}")
            }
        }
    }

    fn service_components(
        state: Arc<Mutex<FakeState>>,
        credential_id: FareCredentialId,
        event_id: DomainEventId,
    ) -> (
        FakeTransactionManager,
        FixedClock,
        FixedCredentialIdGenerator,
        FixedEventIdGenerator,
    ) {
        (
            FakeTransactionManager { state },
            FixedClock { time: event_time() },
            FixedCredentialIdGenerator { id: credential_id },
            FixedEventIdGenerator { id: event_id },
        )
    }

    #[test]
    fn active_account_receives_pending_credential() {
        let account_id = TransitAccountId::generate();
        let credential_id = FareCredentialId::generate();
        let event_id = DomainEventId::generate();

        let state = Arc::new(Mutex::new(FakeState {
            account: Some(active_account(account_id)),
            ..FakeState::default()
        }));

        let (manager, clock, credential_ids, event_ids) =
            service_components(Arc::clone(&state), credential_id, event_id);

        let service =
            IssueFareCredentialService::new(&manager, &clock, &credential_ids, &event_ids);

        let result = run_ready(service.execute(IssueFareCredentialCommand {
            transit_account_id: account_id,
            kind: FareCredentialKind::Card,
        }));

        assert!(matches!(
            result,
            Ok(issued)
                if issued.credential_id()
                    == credential_id
                    && issued.transit_account_id()
                        == account_id
                    && issued.kind()
                        == FareCredentialKind::Card
                    && issued.status()
                        == FareCredentialStatus::Pending
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);

        assert!(matches!(
            state.saved_credential.as_ref(),
            Some(credential)
                if credential.id()
                    == credential_id
                    && credential.status()
                        == FareCredentialStatus::Pending
        ));

        assert_eq!(state.events.len(), 1);

        let event = state.events[0];

        assert_eq!(event.id(), event_id);
        assert_eq!(event.aggregate_version().value(), 1);
        assert_eq!(event.occurred_at(), event_time());

        assert!(matches!(
            event.payload(),
            DomainEventPayload::FareCredentialIssued {
                credential_id: recorded_credential_id,
                account_id: recorded_account_id,
                kind: FareCredentialKind::Card,
            } if recorded_credential_id
                    == credential_id
                && recorded_account_id
                    == account_id
        ));
    }

    #[test]
    fn missing_account_rolls_back() {
        let account_id = TransitAccountId::generate();

        let state = Arc::new(Mutex::new(FakeState::default()));

        let (manager, clock, credential_ids, event_ids) = service_components(
            Arc::clone(&state),
            FareCredentialId::generate(),
            DomainEventId::generate(),
        );

        let service =
            IssueFareCredentialService::new(&manager, &clock, &credential_ids, &event_ids);

        let result = run_ready(service.execute(IssueFareCredentialCommand {
            transit_account_id: account_id,
            kind: FareCredentialKind::Mobile,
        }));

        assert!(matches!(
            result,
            Err(ApplicationError::NotFound {
                entity: "transit account",
                ..
            })
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.saved_credential.is_none());
        assert!(state.events.is_empty());
    }

    #[test]
    fn suspended_account_rolls_back() {
        let account_id = TransitAccountId::generate();
        let mut account = active_account(account_id);

        assert!(account.suspend().is_ok());

        let state = Arc::new(Mutex::new(FakeState {
            account: Some(account),
            ..FakeState::default()
        }));

        let (manager, clock, credential_ids, event_ids) = service_components(
            Arc::clone(&state),
            FareCredentialId::generate(),
            DomainEventId::generate(),
        );

        let service =
            IssueFareCredentialService::new(&manager, &clock, &credential_ids, &event_ids);

        let result = run_ready(service.execute(IssueFareCredentialCommand {
            transit_account_id: account_id,
            kind: FareCredentialKind::DevelopmentTestToken,
        }));

        assert!(matches!(
            result,
            Err(ApplicationError::Conflict {
                operation: "issue fare credential",
                reason: "transit account is not active",
            })
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.saved_credential.is_none());
        assert!(state.events.is_empty());
    }

    #[test]
    fn credential_save_failure_rolls_back() {
        let account_id = TransitAccountId::generate();

        let state = Arc::new(Mutex::new(FakeState {
            account: Some(active_account(account_id)),
            fail_credential_save: true,
            ..FakeState::default()
        }));

        let (manager, clock, credential_ids, event_ids) = service_components(
            Arc::clone(&state),
            FareCredentialId::generate(),
            DomainEventId::generate(),
        );

        let service =
            IssueFareCredentialService::new(&manager, &clock, &credential_ids, &event_ids);

        let result = run_ready(service.execute(IssueFareCredentialCommand {
            transit_account_id: account_id,
            kind: FareCredentialKind::Card,
        }));

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.saved_credential.is_none());
        assert!(state.events.is_empty());
    }
}
