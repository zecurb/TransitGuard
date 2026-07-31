use transitguard_domain::{
    AggregateVersion, DomainEvent, DomainEventPayload, FareCredential, FareCredentialId,
    FareCredentialKind, FareCredentialStatus,
};

use crate::{
    ApplicationError, ApplicationTransaction, Clock, DomainEventIdGenerator,
    FareCredentialIdGenerator, SaveCondition, TransactionManager,
};

/// Input for replacing a project-owned fare credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplaceFareCredentialCommand {
    /// Existing credential that should be superseded.
    pub credential_id: FareCredentialId,

    /// Representation used by the new replacement credential.
    pub replacement_kind: FareCredentialKind,
}

/// Stable application result returned after replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplacedFareCredential {
    original_credential_id: FareCredentialId,
    replacement_credential_id: FareCredentialId,
    previous_status: FareCredentialStatus,
    current_status: FareCredentialStatus,
    replacement_status: FareCredentialStatus,
    changed: bool,
}

impl ReplacedFareCredential {
    /// Returns the credential that was superseded.
    #[must_use]
    pub const fn original_credential_id(self) -> FareCredentialId {
        self.original_credential_id
    }

    /// Returns the newly created or previously recorded replacement.
    #[must_use]
    pub const fn replacement_credential_id(self) -> FareCredentialId {
        self.replacement_credential_id
    }

    /// Returns the original credential's status before replacement.
    #[must_use]
    pub const fn previous_status(self) -> FareCredentialStatus {
        self.previous_status
    }

    /// Returns the original credential's authoritative status.
    #[must_use]
    pub const fn current_status(self) -> FareCredentialStatus {
        self.current_status
    }

    /// Returns the initial status of a newly created replacement.
    #[must_use]
    pub const fn replacement_status(self) -> FareCredentialStatus {
        self.replacement_status
    }

    /// Returns whether this invocation created a replacement.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }
}

/// Coordinates atomic fare-credential replacement.
///
/// The existing credential is permanently marked as replaced while a new
/// pending credential is created for the same transit account.
pub struct ReplaceFareCredentialService<'a> {
    transaction_manager: &'a dyn TransactionManager,
    clock: &'a dyn Clock,
    credential_id_generator: &'a dyn FareCredentialIdGenerator,
    event_id_generator: &'a dyn DomainEventIdGenerator,
}

impl<'a> ReplaceFareCredentialService<'a> {
    /// Creates the replacement service from application ports.
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

    /// Replaces an active or suspended fare credential.
    ///
    /// Repeating replacement for an already replaced credential returns the
    /// previously recorded successor without creating duplicate state.
    pub async fn execute(
        &self,
        command: ReplaceFareCredentialCommand,
    ) -> Result<ReplacedFareCredential, ApplicationError> {
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

        if previous_status == FareCredentialStatus::Replaced {
            let replacement_credential_id = match loaded.aggregate().replacement_id() {
                Some(identifier) => identifier,

                None => {
                    let error = ApplicationError::conflict(
                        "replace fare credential",
                        "replaced credential has no successor",
                    );

                    return rollback_with(transaction, error).await;
                }
            };

            let original_account_id = loaded.aggregate().transit_account_id();

            let recorded_replacement = match transaction
                .find_fare_credential(replacement_credential_id)
                .await
            {
                Ok(Some(credential)) => credential,

                Ok(None) => {
                    let error = ApplicationError::conflict(
                        "replace fare credential",
                        "recorded replacement credential is missing",
                    );

                    return rollback_with(transaction, error).await;
                }

                Err(error) => {
                    return rollback_with(transaction, ApplicationError::from(error)).await;
                }
            };

            if recorded_replacement.aggregate().transit_account_id() != original_account_id {
                let error = ApplicationError::conflict(
                    "replace fare credential",
                    "recorded replacement belongs to another account",
                );

                return rollback_with(transaction, error).await;
            }

            let replacement_status = recorded_replacement.aggregate().status();

            transaction.commit().await?;

            return Ok(ReplacedFareCredential {
                original_credential_id: command.credential_id,
                replacement_credential_id,
                previous_status,
                current_status: FareCredentialStatus::Replaced,
                replacement_status,
                changed: false,
            });
        }

        let expected_version = loaded.version();

        let next_original_version = match loaded.next_version() {
            Some(version) => version,

            None => {
                let error = ApplicationError::conflict(
                    "replace fare credential",
                    "aggregate version is exhausted",
                );

                return rollback_with(transaction, error).await;
            }
        };

        let replacement_version = match AggregateVersion::new(1) {
            Ok(version) => version,

            Err(_) => {
                let error = ApplicationError::conflict(
                    "replace fare credential",
                    "replacement aggregate version is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        let transit_account_id = loaded.aggregate().transit_account_id();

        let replacement_credential_id = self.credential_id_generator.generate();

        let replacement = FareCredential::new_pending(
            replacement_credential_id,
            transit_account_id,
            command.replacement_kind,
        );

        let mut original = loaded.into_aggregate();

        if let Err(error) = original.replace_with(replacement_credential_id) {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        let occurred_at = match self.clock.now() {
            Ok(time) => time,

            Err(error) => {
                return rollback_with(transaction, ApplicationError::from(error)).await;
            }
        };

        let original_event_payload = DomainEventPayload::FareCredentialStatusChanged {
            credential_id: command.credential_id,
            previous_status,
            current_status: FareCredentialStatus::Replaced,
        };

        let original_event = match DomainEvent::new(
            self.event_id_generator.generate(),
            next_original_version,
            occurred_at,
            original_event_payload,
        ) {
            Ok(event) => event,

            Err(_) => {
                let error = ApplicationError::conflict(
                    "replace fare credential",
                    "original credential event is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        let replacement_event_payload = DomainEventPayload::FareCredentialIssued {
            credential_id: replacement_credential_id,
            account_id: transit_account_id,
            kind: command.replacement_kind,
        };

        let replacement_event = match DomainEvent::new(
            self.event_id_generator.generate(),
            replacement_version,
            occurred_at,
            replacement_event_payload,
        ) {
            Ok(event) => event,

            Err(_) => {
                let error = ApplicationError::conflict(
                    "replace fare credential",
                    "replacement issuance event is invalid",
                );

                return rollback_with(transaction, error).await;
            }
        };

        if let Err(error) = transaction
            .save_fare_credential(&original, SaveCondition::IfVersion(expected_version))
            .await
        {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        if let Err(error) = transaction
            .save_fare_credential(&replacement, SaveCondition::MustNotExist)
            .await
        {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        if let Err(error) = transaction.append_domain_event(&original_event).await {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        if let Err(error) = transaction.append_domain_event(&replacement_event).await {
            return rollback_with(transaction, ApplicationError::from(error)).await;
        }

        transaction.commit().await?;

        Ok(ReplacedFareCredential {
            original_credential_id: command.credential_id,
            replacement_credential_id,
            previous_status,
            current_status: original.status(),
            replacement_status: replacement.status(),
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
        collections::{HashMap, VecDeque},
        sync::{Arc, Mutex, MutexGuard},
        thread,
    };

    use thiserror::Error;
    use transitguard_domain::{
        AggregateVersion, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
        FareCredential, FareCredentialError, FareCredentialId, FareCredentialKind,
        FareCredentialStatus, ReaderEquipment, ReaderId, TransitAccount, TransitAccountId,
    };

    use crate::{
        ApplicationError, ApplicationTransaction, Clock, DomainEventIdGenerator,
        FareCredentialIdGenerator, RepositoryError, RepositoryFuture, SaveCondition,
        TransactionManager, VersionedAggregate,
    };

    use super::{
        ReplaceFareCredentialCommand, ReplaceFareCredentialService, ReplacedFareCredential,
    };

    #[derive(Default)]
    struct FakeState {
        credentials: HashMap<FareCredentialId, FareCredential>,
        versions: HashMap<FareCredentialId, AggregateVersion>,
        staged_credentials: Vec<FareCredential>,
        observed_conditions: Vec<(FareCredentialId, SaveCondition)>,
        staged_events: Vec<DomainEvent>,
        committed_events: Vec<DomainEvent>,
        commits: u32,
        rollbacks: u32,
        save_calls: usize,
        append_calls: usize,
        fail_save_on_call: Option<usize>,
        fail_append_on_call: Option<usize>,
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
                Ok(state) => match state.credentials.get(&credential_id).cloned() {
                    Some(credential) => match state.versions.get(&credential_id).copied() {
                        Some(version) => Ok(Some(VersionedAggregate::new(credential, version))),

                        None => Err(repository_error("load aggregate version")),
                    },

                    None => Ok(None),
                },

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
                        state.save_calls += 1;

                        state.observed_conditions.push((credential.id(), condition));

                        if state.fail_save_on_call == Some(state.save_calls) {
                            return Err(repository_error("save fare credential"));
                        }

                        let condition_satisfied = match condition {
                            SaveCondition::MustNotExist => {
                                !state.credentials.contains_key(&credential.id())
                            }

                            SaveCondition::IfVersion(expected) => {
                                state.versions.get(&credential.id()).copied() == Some(expected)
                            }
                        };

                        if !condition_satisfied {
                            return Err(repository_error("conditional save conflict"));
                        }

                        state.staged_credentials.push(credential);

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
                        state.append_calls += 1;

                        if state.fail_append_on_call == Some(state.append_calls) {
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
                        let staged_credentials = core::mem::take(&mut state.staged_credentials);

                        for credential in staged_credentials {
                            state.credentials.insert(credential.id(), credential);
                        }

                        let events = core::mem::take(&mut state.staged_events);

                        for event in &events {
                            let credential_id = match event.payload() {
                                DomainEventPayload::FareCredentialIssued {
                                    credential_id, ..
                                }
                                | DomainEventPayload::FareCredentialStatusChanged {
                                    credential_id,
                                    ..
                                } => Some(credential_id),

                                _ => None,
                            };

                            if let Some(credential_id) = credential_id {
                                state
                                    .versions
                                    .insert(credential_id, event.aggregate_version());
                            }
                        }

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
                        state.staged_credentials.clear();
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

    struct FixedCredentialIdGenerator {
        id: FareCredentialId,
    }

    impl FareCredentialIdGenerator for FixedCredentialIdGenerator {
        fn generate(&self) -> FareCredentialId {
            self.id
        }
    }

    struct SequenceEventIdGenerator {
        ids: Mutex<VecDeque<DomainEventId>>,
    }

    impl DomainEventIdGenerator for SequenceEventIdGenerator {
        fn generate(&self) -> DomainEventId {
            match self.ids.lock() {
                Ok(mut ids) => match ids.pop_front() {
                    Some(id) => id,

                    None => {
                        panic!(
                            "event identifier sequence \
                                 was exhausted"
                        )
                    }
                },

                Err(_) => {
                    panic!(
                        "event identifier sequence lock \
                         was poisoned"
                    )
                }
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

    fn pending_credential(
        credential_id: FareCredentialId,
        account_id: TransitAccountId,
    ) -> FareCredential {
        FareCredential::new_pending(credential_id, account_id, FareCredentialKind::Card)
    }

    fn active_credential(
        credential_id: FareCredentialId,
        account_id: TransitAccountId,
    ) -> FareCredential {
        let mut credential = pending_credential(credential_id, account_id);

        if let Err(error) = credential.activate() {
            panic!(
                "valid credential activation failed: \
                 {error}"
            );
        }

        credential
    }

    fn suspended_credential(
        credential_id: FareCredentialId,
        account_id: TransitAccountId,
    ) -> FareCredential {
        let mut credential = active_credential(credential_id, account_id);

        if let Err(error) = credential.suspend() {
            panic!(
                "valid credential suspension failed: \
                 {error}"
            );
        }

        credential
    }

    fn replaced_credential(
        credential_id: FareCredentialId,
        account_id: TransitAccountId,
        replacement_id: FareCredentialId,
    ) -> FareCredential {
        let mut credential = active_credential(credential_id, account_id);

        if let Err(error) = credential.replace_with(replacement_id) {
            panic!(
                "valid credential replacement failed: \
                 {error}"
            );
        }

        credential
    }

    fn state_with_credential(
        credential: FareCredential,
        aggregate_version: AggregateVersion,
    ) -> Arc<Mutex<FakeState>> {
        let mut credentials = HashMap::new();
        let mut versions = HashMap::new();

        credentials.insert(credential.id(), credential.clone());

        versions.insert(credential.id(), aggregate_version);

        Arc::new(Mutex::new(FakeState {
            credentials,
            versions,
            ..FakeState::default()
        }))
    }

    fn execute(
        state: Arc<Mutex<FakeState>>,
        command: ReplaceFareCredentialCommand,
        replacement_id: FareCredentialId,
        event_ids: Vec<DomainEventId>,
    ) -> Result<ReplacedFareCredential, ApplicationError> {
        let manager = FakeTransactionManager { state };

        let clock = FixedClock { time: event_time() };

        let credential_ids = FixedCredentialIdGenerator { id: replacement_id };

        let event_ids = SequenceEventIdGenerator {
            ids: Mutex::new(event_ids.into()),
        };

        let service =
            ReplaceFareCredentialService::new(&manager, &clock, &credential_ids, &event_ids);

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
    fn active_credential_is_replaced_atomically() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let replacement_id = FareCredentialId::generate();

        let original_event_id = DomainEventId::generate();

        let replacement_event_id = DomainEventId::generate();

        let stored_version = version(4);

        let state =
            state_with_credential(active_credential(original_id, account_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            replacement_id,
            vec![original_event_id, replacement_event_id],
        );

        assert!(matches!(
            result,
            Ok(replaced)
                if replaced
                    .original_credential_id()
                    == original_id
                    && replaced
                        .replacement_credential_id()
                        == replacement_id
                    && replaced.previous_status()
                        == FareCredentialStatus::Active
                    && replaced.current_status()
                        == FareCredentialStatus::Replaced
                    && replaced.replacement_status()
                        == FareCredentialStatus::Pending
                    && replaced.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);

        assert_eq!(
            state.observed_conditions,
            vec![
                (original_id, SaveCondition::IfVersion(stored_version)),
                (replacement_id, SaveCondition::MustNotExist),
            ]
        );

        assert_eq!(state.versions.get(&original_id), Some(&version(5)));

        assert_eq!(state.versions.get(&replacement_id), Some(&version(1)));

        assert!(matches!(
            state.credentials.get(&original_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Replaced
                    && credential.replacement_id()
                        == Some(replacement_id)
        ));

        assert!(matches!(
            state.credentials.get(&replacement_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Pending
                    && credential.transit_account_id()
                        == account_id
                    && credential.kind()
                        == FareCredentialKind::Mobile
        ));

        assert_eq!(state.committed_events.len(), 2);

        let original_event = state.committed_events[0];

        let replacement_event = state.committed_events[1];

        assert_eq!(original_event.id(), original_event_id);

        assert_eq!(original_event.aggregate_version(), version(5));

        assert!(matches!(
            original_event.payload(),
            DomainEventPayload::
                FareCredentialStatusChanged {
                    credential_id,
                    previous_status:
                        FareCredentialStatus::Active,
                    current_status:
                        FareCredentialStatus::Replaced,
                } if credential_id == original_id
        ));

        assert_eq!(replacement_event.id(), replacement_event_id);

        assert_eq!(replacement_event.aggregate_version(), version(1));

        assert!(matches!(
            replacement_event.payload(),
            DomainEventPayload::
                FareCredentialIssued {
                    credential_id,
                    account_id:
                        recorded_account_id,
                    kind:
                        FareCredentialKind::Mobile,
                } if credential_id
                    == replacement_id
                    && recorded_account_id
                        == account_id
        ));
    }

    #[test]
    fn suspended_credential_can_be_replaced() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let replacement_id = FareCredentialId::generate();

        let state =
            state_with_credential(suspended_credential(original_id, account_id), version(2));

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Card,
            },
            replacement_id,
            vec![DomainEventId::generate(), DomainEventId::generate()],
        );

        assert!(matches!(
            result,
            Ok(replaced)
                if replaced.previous_status()
                    == FareCredentialStatus::Suspended
                    && replaced.current_status()
                        == FareCredentialStatus::Replaced
                    && replaced.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);

        assert_eq!(state.versions.get(&original_id), Some(&version(3)));

        assert!(state.credentials.contains_key(&replacement_id));
    }

    #[test]
    fn repeated_replacement_returns_authoritative_successor_status() {
        let account_id = TransitAccountId::generate();
        let original_id = FareCredentialId::generate();
        let recorded_replacement_id = FareCredentialId::generate();
        let generated_replacement_id = FareCredentialId::generate();
        let stored_version = version(7);
        let replacement_version = version(2);

        let state = state_with_credential(
            replaced_credential(original_id, account_id, recorded_replacement_id),
            stored_version,
        );

        {
            let mut state = lock_state(&state);

            state.credentials.insert(
                recorded_replacement_id,
                active_credential(recorded_replacement_id, account_id),
            );

            state
                .versions
                .insert(recorded_replacement_id, replacement_version);
        }

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            generated_replacement_id,
            Vec::new(),
        );

        assert!(matches!(
            result,
            Ok(replaced)
                if replaced
                    .replacement_credential_id()
                    == recorded_replacement_id
                    && replaced.previous_status()
                        == FareCredentialStatus::Replaced
                    && replaced.current_status()
                        == FareCredentialStatus::Replaced
                    && replaced.replacement_status()
                        == FareCredentialStatus::Active
                    && !replaced.changed()
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 1);
        assert_eq!(state.rollbacks, 0);
        assert!(state.observed_conditions.is_empty());
        assert!(state.committed_events.is_empty());

        assert_eq!(state.versions.get(&original_id), Some(&stored_version));

        assert_eq!(
            state.versions.get(&recorded_replacement_id),
            Some(&replacement_version)
        );

        assert!(!state.credentials.contains_key(&generated_replacement_id));
    }

    #[test]
    fn missing_recorded_replacement_rolls_back() {
        let account_id = TransitAccountId::generate();
        let original_id = FareCredentialId::generate();
        let recorded_replacement_id = FareCredentialId::generate();
        let stored_version = version(5);

        let state = state_with_credential(
            replaced_credential(original_id, account_id, recorded_replacement_id),
            stored_version,
        );

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Card,
            },
            FareCredentialId::generate(),
            Vec::new(),
        );

        assert!(matches!(
            result,
            Err(ApplicationError::Conflict {
                operation: "replace fare credential",
                reason: "recorded replacement credential is missing",
            })
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.observed_conditions.is_empty());
        assert!(state.committed_events.is_empty());

        assert_eq!(state.versions.get(&original_id), Some(&stored_version));
    }

    #[test]
    fn missing_credential_rolls_back() {
        let original_id = FareCredentialId::generate();

        let state = Arc::new(Mutex::new(FakeState::default()));

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Card,
            },
            FareCredentialId::generate(),
            Vec::new(),
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
        assert!(state.observed_conditions.is_empty());
        assert!(state.committed_events.is_empty());
    }

    #[test]
    fn pending_credential_cannot_be_replaced() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let stored_version = version(3);

        let state =
            state_with_credential(pending_credential(original_id, account_id), stored_version);

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            FareCredentialId::generate(),
            Vec::new(),
        );

        assert!(matches!(
            result,
            Err(ApplicationError::FareCredential(
                FareCredentialError::InvalidStatusTransition {
                    from: FareCredentialStatus::Pending,
                    to: FareCredentialStatus::Replaced,
                }
            ))
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.observed_conditions.is_empty());

        assert!(matches!(
            state.credentials.get(&original_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Pending
        ));

        assert_eq!(state.versions.get(&original_id), Some(&stored_version));
    }

    #[test]
    fn exhausted_original_version_rolls_back() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let state = state_with_credential(
            active_credential(original_id, account_id),
            version(u64::MAX),
        );

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            FareCredentialId::generate(),
            Vec::new(),
        );

        assert!(matches!(
            result,
            Err(ApplicationError::Conflict {
                operation: "replace fare credential",
                reason: "aggregate version is exhausted",
            })
        ));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert!(state.observed_conditions.is_empty());
        assert!(state.committed_events.is_empty());
    }

    #[test]
    fn original_conditional_save_failure_rolls_back() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let replacement_id = FareCredentialId::generate();

        let stored_version = version(5);

        let state =
            state_with_credential(active_credential(original_id, account_id), stored_version);

        {
            let mut state = lock_state(&state);
            state.fail_save_on_call = Some(1);
        }

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            replacement_id,
            vec![DomainEventId::generate(), DomainEventId::generate()],
        );

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);

        assert_eq!(
            state.observed_conditions,
            vec![(original_id, SaveCondition::IfVersion(stored_version))]
        );

        assert!(state.staged_credentials.is_empty());
        assert!(state.staged_events.is_empty());
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credentials.get(&original_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Active
        ));

        assert!(!state.credentials.contains_key(&replacement_id));
    }

    #[test]
    fn replacement_identifier_collision_rolls_back_original() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let replacement_id = FareCredentialId::generate();

        let stored_version = version(4);

        let state =
            state_with_credential(active_credential(original_id, account_id), stored_version);

        {
            let mut state = lock_state(&state);

            let collision = pending_credential(replacement_id, TransitAccountId::generate());

            state.credentials.insert(replacement_id, collision);

            state.versions.insert(replacement_id, version(1));
        }

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            replacement_id,
            vec![DomainEventId::generate(), DomainEventId::generate()],
        );

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);

        assert_eq!(
            state.observed_conditions,
            vec![
                (original_id, SaveCondition::IfVersion(stored_version)),
                (replacement_id, SaveCondition::MustNotExist),
            ]
        );

        assert!(state.staged_credentials.is_empty());
        assert!(state.staged_events.is_empty());
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credentials.get(&original_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Active
                    && credential.replacement_id()
                        == None
        ));

        assert_eq!(state.versions.get(&original_id), Some(&stored_version));
    }

    #[test]
    fn second_event_failure_discards_both_credentials() {
        let account_id = TransitAccountId::generate();

        let original_id = FareCredentialId::generate();

        let replacement_id = FareCredentialId::generate();

        let stored_version = version(6);

        let state =
            state_with_credential(active_credential(original_id, account_id), stored_version);

        {
            let mut state = lock_state(&state);
            state.fail_append_on_call = Some(2);
        }

        let result = execute(
            Arc::clone(&state),
            ReplaceFareCredentialCommand {
                credential_id: original_id,
                replacement_kind: FareCredentialKind::Mobile,
            },
            replacement_id,
            vec![DomainEventId::generate(), DomainEventId::generate()],
        );

        assert!(matches!(result, Err(ApplicationError::Repository(_))));

        let state = lock_state(&state);

        assert_eq!(state.commits, 0);
        assert_eq!(state.rollbacks, 1);
        assert_eq!(state.append_calls, 2);
        assert!(state.staged_credentials.is_empty());
        assert!(state.staged_events.is_empty());
        assert!(state.committed_events.is_empty());

        assert!(matches!(
            state.credentials.get(&original_id),
            Some(credential)
                if credential.status()
                    == FareCredentialStatus::Active
                    && credential.replacement_id()
                        == None
        ));

        assert!(!state.credentials.contains_key(&replacement_id));

        assert_eq!(state.versions.get(&original_id), Some(&stored_version));
    }
}
