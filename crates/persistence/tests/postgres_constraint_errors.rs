use std::{env, error::Error as StandardError};

use transitguard_application::{
    DomainEventRepository, FareCredentialRepository, RepositoryError, SaveCondition,
    TransitAccountRepository,
};
use transitguard_domain::{
    AggregateVersion, Currency, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
    EligibilityClassification, FareCredential, FareCredentialId, FareCredentialKind, Money,
    RiderId, TransitAccount, TransitAccountId,
};
use transitguard_persistence::{
    PersistenceError, PostgresConfig, PostgresDomainEventRepository,
    PostgresFareCredentialRepository, PostgresTransitAccountRepository, connect_postgres,
    run_postgres_migrations,
};

fn persistence_source(error: &RepositoryError) -> &PersistenceError {
    match error
        .source()
        .and_then(|source| source.downcast_ref::<PersistenceError>())
    {
        Some(error) => error,

        None => {
            panic!(
                "repository error did not preserve \
                 PersistenceError as its source"
            )
        }
    }
}

fn account(account_id: TransitAccountId, rider_id: RiderId) -> TransitAccount {
    match TransitAccount::new(
        account_id,
        rider_id,
        EligibilityClassification::Standard,
        Money::from_minor_units(2_500, Currency::Usd),
    ) {
        Ok(account) => account,

        Err(error) => {
            panic!(
                "account construction failed: \
                 {error}"
            )
        }
    }
}

fn version(value: u64) -> AggregateVersion {
    match AggregateVersion::new(value) {
        Ok(version) => version,

        Err(error) => {
            panic!(
                "aggregate-version construction failed: \
                 {error}"
            )
        }
    }
}

fn event_time(value: i64) -> DomainEventTime {
    match DomainEventTime::from_unix_milliseconds(value) {
        Ok(event_time) => event_time,

        Err(error) => {
            panic!(
                "event-time construction failed: \
                 {error}"
            )
        }
    }
}

fn account_created_event(account: &TransitAccount) -> DomainEvent {
    let payload = DomainEventPayload::TransitAccountCreated {
        account_id: account.id(),
        rider_id: account.rider_id(),
        initial_balance: account.stored_value(),
    };

    match DomainEvent::new(
        DomainEventId::generate(),
        version(1),
        event_time(1_700_000_000_000),
        payload,
    ) {
        Ok(event) => event,

        Err(error) => {
            panic!(
                "domain-event construction failed: \
                 {error}"
            )
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn write_constraints_have_stable_error_categories() {
    let database_url = match env::var("DATABASE_URL") {
        Ok(database_url) => database_url,

        Err(error) => {
            panic!(
                "DATABASE_URL is required: \
                     {error}"
            )
        }
    };

    let config = match PostgresConfig::new(database_url) {
        Ok(config) => config,

        Err(error) => {
            panic!(
                "database configuration failed: \
                     {error}"
            )
        }
    };

    let pool = match connect_postgres(&config).await {
        Ok(pool) => pool,

        Err(error) => {
            panic!(
                "database connection failed: \
                     {error}"
            )
        }
    };

    if let Err(error) = run_postgres_migrations(&pool).await {
        panic!(
            "database migrations failed: \
             {error}"
        );
    }

    let account_repository = PostgresTransitAccountRepository::new(pool.clone());

    let credential_repository = PostgresFareCredentialRepository::new(pool.clone());

    let event_repository = PostgresDomainEventRepository::new(pool);

    let account_id = TransitAccountId::generate();

    let rider_id = RiderId::generate();

    let account = account(account_id, rider_id);

    assert!(
        account_repository
            .save(&account, SaveCondition::MustNotExist,)
            .await
            .is_ok()
    );

    let duplicate_account = match account_repository
        .save(&account, SaveCondition::MustNotExist)
        .await
    {
        Ok(()) => {
            panic!("duplicate account insert succeeded")
        }

        Err(error) => error,
    };

    assert!(matches!(
        persistence_source(&duplicate_account),
        PersistenceError::WriteConditionFailed {
            entity: "transit account"
        }
    ));

    let orphaned_credential = FareCredential::new_pending(
        FareCredentialId::generate(),
        TransitAccountId::generate(),
        FareCredentialKind::Card,
    );

    let foreign_key_error = match credential_repository
        .save(&orphaned_credential, SaveCondition::MustNotExist)
        .await
    {
        Ok(()) => {
            panic!("orphaned credential insert succeeded")
        }

        Err(error) => error,
    };

    assert!(matches!(
        persistence_source(&foreign_key_error),
        PersistenceError::ConstraintViolation {
            entity: "fare credential",
            kind: "foreign-key",
            ..
        }
    ));

    let event = account_created_event(&account);

    assert!(event_repository.append(&event).await.is_ok());

    let duplicate_event = match event_repository.append(&event).await {
        Ok(()) => {
            panic!("duplicate domain event insert succeeded")
        }

        Err(error) => error,
    };

    assert!(matches!(
        persistence_source(&duplicate_event),
        PersistenceError::WriteConditionFailed {
            entity: "domain event"
        }
    ));
}
