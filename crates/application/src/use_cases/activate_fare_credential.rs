use transitguard_domain::{
    DomainEvent, DomainEventPayload, FareCredentialId, FareCredentialStatus,
};

use crate::{
    ApplicationError, ApplicationTransaction, Clock, DomainEventIdGenerator, SaveCondition,
    TransactionManager,
};

/// Input for activating a project-owned fare credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivateFareCredentialCommand {
    /// Credential that should become usable.
    pub credential_id: FareCredentialId,
}

/// Stable application result returned after credential activation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivatedFareCredential {
    credential_id: FareCredentialId,
    previous_status: FareCredentialStatus,
    current_status: FareCredentialStatus,
    changed: bool,
}

impl ActivatedFareCredential {
    /// Returns the affected credential identifier.
    #[must_use]
    pub const fn credential_id(self) -> FareCredentialId {
        self.credential_id
    }

    /// Returns the status observed before activation.
    #[must_use]
    pub const fn previous_status(self) -> FareCredentialStatus {
        self.previous_status
    }

    /// Returns the authoritative status after activation.
    #[must_use]
    pub const fn current_status(self) -> FareCredentialStatus {
        self.current_status
    }

    /// Returns whether this invocation changed persistent state.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }
}

/// Coordinates fare-credential activation.
///
/// The updated credential and its immutable status-change event are persisted
/// atomically under an optimistic-concurrency condition.
pub struct ActivateFareCredentialService<'a> {
    transaction_manager: &'a dyn TransactionManager,
    clock: &'a dyn Clock,
    event_id_generator: &'a dyn DomainEventIdGenerator,
}

impl<'a> ActivateFareCredentialService<'a> {
    /// Creates the activation service from application ports.
    #[must_use]
    pub const fn new(
        transaction_manager: &'a dyn TransactionManager,
        clock: &'a dyn Clock,
        event_id_generator: &'a dyn DomainEventIdGenerator,
    ) -> Self {
        Self {
            transaction_manager,
            clock,
            event_id_generator,
        }
    }

    /// Activates a pending or suspended fare credential.
    ///
    /// Activating an already active credential is idempotent and creates no
    /// duplicate event.
    pub async fn execute(
        &self,
        command: ActivateFareCredentialCommand,
    ) -> Result<ActivatedFareCredential, ApplicationError> {
        let mut transaction = self.transaction_manager.begin().await?;

        let loaded = match transaction
            .find_fare_credential(command.credential_id)
            .await
        {
            Ok(Some(credential)) => credential,

            Ok(None) => {
                let error = ApplicationError::not_found(
                    "fare credential",
                    command.credential_id.to_string(),
                );

                return rollback_with(transaction, error).await;
            }

            Err(error) => {
                return rollback_with(transaction, ApplicationError::from(error)).await;
            }
        };

        let previous_status = loaded.aggregate().status();

        if previous_status == FareCredentialStatus::Active {
            transaction.commit().await?;

            return Ok(ActivatedFareCredential {
                credential_id: command.credential_id,
                previous_status,
                current_status: FareCredentialStatus::Active,
                changed: false,
            });
        }

        let expected_version = loaded.version();

        let next_version = match loaded.next_version() {
            Some(version) => version,

            None => {
                let error = ApplicationError::conflict(
                    "activate fare credential",
                    "aggregate version is exhausted",
                );

                return rollback_with(transaction, error).await;
            }
        };

        let mut credential = loaded.into_aggregate();

        if let Err(error) = credential.activate() {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        let occurred_at = match self.clock.now() {
            Ok(time) => time,

            Err(error) => {
                return rollback_with(transaction, ApplicationError::from(error)).await;
            }
        };

        let payload = DomainEventPayload::FareCredentialStatusChanged {
            credential_id: command.credential_id,
            previous_status,
            current_status: FareCredentialStatus::Active,
        };

        let event = match DomainEvent::new(
            self.event_id_generator.generate(),
            next_version,
            occurred_at,
            payload,
        ) {
            Ok(event) => event,

            Err(_) => {
                let error = ApplicationError::conflict(
                    "activate fare credential",
                    "credential activation event is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        if let Err(error) = transaction
            .save_fare_credential(&credential, SaveCondition::IfVersion(expected_version))
            .await
        {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        if let Err(error) = transaction.append_domain_event(&event).await {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        transaction.commit().await?;

        Ok(ActivatedFareCredential {
            credential_id: command.credential_id,
            previous_status,
            current_status: credential.status(),
            changed: true,
        })
    }
}

async fn rollback_with<T>(
    transaction: Box<dyn ApplicationTransaction>,
    original_error: ApplicationError,
) -> Result<T, ApplicationError> {
    match transaction.rollback().await {
        Ok(()) => Err(original_error),

        Err(rollback_error) => Err(ApplicationError::from(rollback_error)),
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
        AggregateVersion, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
        FareCredential, FareCredentialError, FareCredentialId, FareCredentialKind,
        FareCredentialStatus, ReaderEquipment, ReaderId, RevocationReason, TransitAccount,
        TransitAccountId,
    };

    use crate::{
        ApplicationError, ApplicationTransaction, Clock, DomainEventIdGenerator, RepositoryError,
        RepositoryFuture, SaveCondition, TransactionManager, VersionedAggregate,
    };

    use super::{
        ActivateFareCredentialCommand, ActivateFareCredentialService, ActivatedFareCredential,
    };

    #[derive(Default)]
    struct FakeState {
        credential: Option<FareCredential>,
        version: Option<AggregateVersion>,
        staged_credential: Option<FareCredential>,
        observed_condition: Option<SaveCondition>,
        staged_events: Vec<DomainEvent>,
        committed_events: Vec<DomainEvent>,
        commits: u32,
        rollbacks: u32,
        fail_save: bool,
        fail_append: bool,
    }

    struct FakeTransaction {
        state: Arc<Mutex<FakeState>>,
    }

    struct FakeTransactionManager {
        state: Arc<Mutex<FakeState>>,
    }

    #[derive(Debug, Error)]
    #[error("fake persistence operation failed")]
    struct FakePersistenceError;

    fn repository_error(operation: &'static str) -> RepositoryError {
        RepositoryError::new("fare credential", operation, FakePersistenceError)
    }

    impl ApplicationTransaction for FakeTransaction {
        fn find_transit_account(
            &mut self,
            _account_id: TransitAccountId,
        ) -> RepositoryFuture<'_, Option<VersionedAggregate<TransitAccount>>> {
            Box::pin(async { Ok(None) })
        }

        fn save_transit_account<'a>(
            &'a mut self,
            _account: &'a TransitAccount,
            _condition: SaveCondition,
        ) -> RepositoryFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn find_fare_credential(
            &mut self,
            credential_id: FareCredentialId,
        ) -> RepositoryFuture<'_, Option<VersionedAggregate<FareCredential>>> {
            let result = match self.state.lock() {
                Ok(state) => {
                    match state
                        .credential
                        .as_ref()
                        .filter(|credential| credential.id() == credential_id)
                        .cloned()
                    {
                        Some(credential) => match state.version {
                            Some(version) => Ok(Some(VersionedAggregate::new(credential, version))),

                            None => Err(repository_error("load aggregate version")),
                        },

                        None => Ok(None),
                    }
                }

                Err(_) => Err(repository_error("find fare credential")),
            };

            Box::pin(async move { result })
        }

        fn save_fare_credential<'a>(
            &'a mut self,
            credential: &'a FareCredential,
            condition: SaveCondition,
        ) -> RepositoryFuture<'a, ()> {
            let credential = credential.clone();
            let state = Arc::clone(&self.state);

            Box::pin(async move {
                match state.lock() {
                    Ok(mut state) => {
                        state.observed_condition = Some(condition);

                        if state.fail_save {
                            return Err(repository_error("save fare credential"));
                        }

                        let condition_satisfied = match condition {
                            SaveCondition::MustNotExist => state.credential.is_none(),

                            SaveCondition::IfVersion(expected) => state.version == Some(expected),
                        };

                        if !condition_satisfied {
                            return Err(repository_error("conditional save conflict"));
                        }

                        state.staged_credential = Some(credential);

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
                        if state.fail_append {
                            return Err(repository_error("append domain event"));
                        }

                        state.staged_events.push(event);

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
                        let next_version = state
                            .staged_events
                            .last()
                            .copied()
                            .map(DomainEvent::aggregate_version);

                        if let Some(credential) = state.staged_credential.take() {
                            state.credential = Some(credential);
                        }

                        if let Some(version) = next_version {
                            state.version = Some(version);
                        }

                        let events = core::mem::take(&mut state.staged_events);

                        state.committed_events.extend(events);

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
                        state.staged_credential = None;
                        state.staged_events.clear();
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

    struct FixedEventIdGenerator {
        id: DomainEventId,
    }

    impl DomainEventIdGenerator for FixedEventIdGenerator {
        fn generate(&self) -> DomainEventId {
            self.id
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

    fn version(value: u64) -> AggregateVersion {
        match AggregateVersion::new(value) {
            Ok(version) => version,

            Err(error) => {
                panic!(
                    "valid aggregate version failed: \
                     {error}"
                )
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

    fn pending_credential(credential_id: FareCredentialId) -> FareCredential {
        FareCredential::new_pending(
            credential_id,
            TransitAccountId::generate(),
            FareCredentialKind::Card,
        )
    }

    fn active_credential(credential_id: FareCredentialId) -> FareCredential {
        let mut credential = pending_credential(credential_id);

        if let Err(error) = credential.activate() {
            panic!(
                "valid credential activation failed: \
                 {error}"
            );
        }

        credential
    }

    fn suspended_credential(credential_id: FareCredentialId) -> FareCredential {
        let mut credential = active_credential(credential_id);

        if let Err(error) = credential.suspend() {
            panic!(
                "valid credential suspension failed: \
                 {error}"
            );
        }

        credential
    }

    fn revoked_credential(credential_id: FareCredentialId) -> FareCredential {
        let mut credential = active_credential(credential_id);

        if let Err(error) = credential.revoke(RevocationReason::SecurityIncident) {
            panic!(
                "valid credential revocation failed: \
                 {error}"
            );
        }

        credential
    }

    fn state_with_credential(
        credential: FareCredential,
        aggregate_version: AggregateVersion,
    ) -> Arc<Mutex<FakeState>> {
        Arc::new(Mutex::new(FakeState {
            credential: Some(credential),
            version: Some(aggregate_version),
            ..FakeState::default()
        }))
    }

    fn execute(
        state: Arc<Mutex<FakeState>>,
        command: ActivateFareCredentialCommand,
        event_id: DomainEventId,
    ) -> Result<ActivatedFareCredential, ApplicationError> {
        let manager = FakeTransactionManager { state };

        let clock = FixedClock { time: event_time() };

        let event_ids = FixedEventIdGenerator { id: event_id };

        let service = ActivateFareCredentialService::new(&manager, &clock, &event_ids);

        run_ready(service.execute(command))
    }

    fn lock_state<'a>(state: &'a Arc<Mutex<FakeState>>) -> MutexGuard<'a, FakeState> {
        match state.lock() {
            Ok(state) => state,

            Err(_) => {
                panic!("fake state lock was poisoned")
            }
        }
    }

    #[test]
    fn pending_credential_is_activated_atomically() {
        let credential_id = FareCredentialId::generate();

        let event_id = DomainEventId::generate();

        let stored_version = version(4);

        let state = state_with_credential(pending_credential(credential_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            event_id,
        );

        assert!(matches!(
            result,
            Ok(activated)
                if activated.credential_id()
                    == credential_id
                    && activated.previous_status()
                        == FareCredentialStatus::Pending
                    && activated.current_status()
                        == FareCredentialStatus::Active
                    && activated.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);

        assert_eq!(
            state.observed_condition,
            Some(SaveCondition::IfVersion(stored_version))
        );

        assert_eq!(state.version, Some(version(5)));

        assert!(matches!(
            state.credential.as_ref(),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Active
                    && credential.is_usable()
        ));

        assert_eq!(state.committed_events.len(), 1);

        let event = state.committed_events[0];

        assert_eq!(event.id(), event_id);

        assert_eq!(event.aggregate_version(), version(5));

        assert_eq!(event.occurred_at(), event_time());

        assert!(matches!(
            event.payload(),
            DomainEventPayload::
                FareCredentialStatusChanged {
                    credential_id:
                        recorded_credential_id,
                    previous_status:
                        FareCredentialStatus::Pending,
                    current_status:
                        FareCredentialStatus::Active,
                } if recorded_credential_id
                    == credential_id
        ));
    }

    #[test]
    fn suspended_credential_is_reactivated() {
        let credential_id = FareCredentialId::generate();

        let stored_version = version(7);

        let state = state_with_credential(suspended_credential(credential_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(
            result,
            Ok(activated)
                if activated.previous_status()
                    == FareCredentialStatus::Suspended
                    && activated.current_status()
                        == FareCredentialStatus::Active
                    && activated.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(
            state.observed_condition,
            Some(SaveCondition::IfVersion(stored_version))
        );

        assert_eq!(state.version, Some(version(8)));

        assert_eq!(state.committed_events.len(), 1);

        assert!(matches!(
            state.credential.as_ref(),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Active
        ));
    }

    #[test]
    fn active_credential_activation_is_idempotent() {
        let credential_id = FareCredentialId::generate();

        let stored_version = version(9);

        let state = state_with_credential(active_credential(credential_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(
            result,
            Ok(activated)
                if activated.previous_status()
                    == FareCredentialStatus::Active
                    && activated.current_status()
                        == FareCredentialStatus::Active
                    && !activated.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);
        assert_eq!(state.version, Some(stored_version));
        assert_eq!(state.observed_condition, None);
        assert!(state.committed_events.is_empty());
    }

    #[test]
    fn missing_credential_rolls_back() {
        let credential_id = FareCredentialId::generate();

        let state = Arc::new(Mutex::new(FakeState::default()));

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(
            result,
            Err(ApplicationError::NotFound {
                entity: "fare credential",
                ..
            })
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert_eq!(state.observed_condition, None);
        assert!(state.committed_events.is_empty());
    }

    #[test]
    fn revoked_credential_cannot_be_activated() {
        let credential_id = FareCredentialId::generate();

        let stored_version = version(3);

        let state = state_with_credential(revoked_credential(credential_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(
            result,
            Err(ApplicationError::FareCredential(
                FareCredentialError::InvalidStatusTransition {
                    from: FareCredentialStatus::Revoked,
                    to: FareCredentialStatus::Active,
                }
            ))
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert_eq!(state.version, Some(stored_version));
        assert_eq!(state.observed_condition, None);
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credential.as_ref(),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Revoked
        ));
    }

    #[test]
    fn conditional_save_failure_rolls_back() {
        let credential_id = FareCredentialId::generate();

        let stored_version = version(2);

        let state = state_with_credential(pending_credential(credential_id), stored_version);

        {
            let mut state_guard = lock_state(&state);

            state_guard.fail_save = true;
        }

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);

        assert_eq!(
            state.observed_condition,
            Some(SaveCondition::IfVersion(stored_version))
        );

        assert_eq!(state.version, Some(stored_version));
        assert!(state.staged_credential.is_none());
        assert!(state.staged_events.is_empty());
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credential.as_ref(),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Pending
        ));
    }

    #[test]
    fn event_append_failure_discards_staged_activation() {
        let credential_id = FareCredentialId::generate();

        let stored_version = version(6);

        let state = state_with_credential(pending_credential(credential_id), stored_version);

        {
            let mut state_guard = lock_state(&state);

            state_guard.fail_append = true;
        }

        let result = execute(
            Arc::clone(&state),
            ActivateFareCredentialCommand { credential_id },
            DomainEventId::generate(),
        );

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);

        assert_eq!(
            state.observed_condition,
            Some(SaveCondition::IfVersion(stored_version))
        );

        assert_eq!(state.version, Some(stored_version));
        assert!(state.staged_credential.is_none());
        assert!(state.staged_events.is_empty());
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credential.as_ref(),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Pending
        ));
    }
}
