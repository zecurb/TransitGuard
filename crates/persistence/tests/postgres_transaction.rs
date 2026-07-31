use std::env;

use transitguard_application::{SaveCondition, TransactionManager};
use transitguard_domain::{
    AggregateVersion, Currency, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
    EligibilityClassification, Money, RiderId, TransitAccount, TransitAccountId,
    TransitAccountStatus,
};
use transitguard_persistence::{
    PostgresConfig, PostgresTransactionManager, connect_postgres, run_postgres_migrations,
};

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

fn event_time(value: i64) -> DomainEventTime {
    match DomainEventTime::from_unix_milliseconds(value) {
        Ok(event_time) => event_time,

        Err(error) => {
            panic!(
                "valid event time failed: \
                 {error}"
            )
        }
    }
}

fn event(
    version: AggregateVersion,
    occurred_at: DomainEventTime,
    payload: DomainEventPayload,
) -> DomainEvent {
    match DomainEvent::new(DomainEventId::generate(), version, occurred_at, payload) {
        Ok(event) => event,

        Err(error) => {
            panic!(
                "valid domain event failed: \
                 {error}"
            )
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn transaction_commits_and_rolls_back_atomically() {
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

    let manager = PostgresTransactionManager::new(pool);

    let account_id = TransitAccountId::generate();

    let rider_id = RiderId::generate();

    let account = match TransitAccount::new(
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
    };

    let created_event = event(
        version(1),
        event_time(1_700_000_000_000),
        DomainEventPayload::TransitAccountCreated {
            account_id,
            rider_id,
            initial_balance: account.stored_value(),
        },
    );

    let mut rolled_back = match manager.begin().await {
        Ok(transaction) => transaction,

        Err(error) => {
            panic!(
                "transaction begin failed: \
                     {error}"
            )
        }
    };

    assert!(
        rolled_back
            .save_transit_account(&account, SaveCondition::MustNotExist,)
            .await
            .is_ok()
    );

    assert!(
        rolled_back
            .append_domain_event(&created_event,)
            .await
            .is_ok()
    );

    assert!(rolled_back.rollback().await.is_ok());

    let account_count = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
             FROM transit_accounts",
    )
    .fetch_one(manager.pool())
    .await
    {
        Ok(count) => count,

        Err(error) => {
            panic!(
                "account count query failed: \
                     {error}"
            )
        }
    };

    let event_count = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
             FROM domain_events",
    )
    .fetch_one(manager.pool())
    .await
    {
        Ok(count) => count,

        Err(error) => {
            panic!(
                "event count query failed: \
                     {error}"
            )
        }
    };

    assert_eq!(account_count, 0);
    assert_eq!(event_count, 0);

    let mut committed = match manager.begin().await {
        Ok(transaction) => transaction,

        Err(error) => {
            panic!(
                "transaction begin failed: \
                     {error}"
            )
        }
    };

    assert!(
        committed
            .save_transit_account(&account, SaveCondition::MustNotExist,)
            .await
            .is_ok()
    );

    assert!(committed.append_domain_event(&created_event,).await.is_ok());

    assert!(committed.commit().await.is_ok());

    let account_count = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
             FROM transit_accounts",
    )
    .fetch_one(manager.pool())
    .await
    {
        Ok(count) => count,

        Err(error) => {
            panic!(
                "account count query failed: \
                     {error}"
            )
        }
    };

    let event_count = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
             FROM domain_events",
    )
    .fetch_one(manager.pool())
    .await
    {
        Ok(count) => count,

        Err(error) => {
            panic!(
                "event count query failed: \
                     {error}"
            )
        }
    };

    assert_eq!(account_count, 1);
    assert_eq!(event_count, 1);

    let mut conflicted = match manager.begin().await {
        Ok(transaction) => transaction,

        Err(error) => {
            panic!(
                "transaction begin failed: \
                     {error}"
            )
        }
    };

    let loaded = match conflicted.find_transit_account(account_id).await {
        Ok(Some(account)) => account,

        Ok(None) => {
            panic!("committed account was not found")
        }

        Err(error) => {
            panic!(
                "account lookup failed: \
                     {error}"
            )
        }
    };

    let first_version = loaded.version();
    let mut suspended = loaded.aggregate().clone();

    assert!(suspended.suspend().is_ok());

    let suspended_event = event(
        version(2),
        event_time(1_700_000_000_001),
        DomainEventPayload::TransitAccountStatusChanged {
            account_id,

            previous_status: TransitAccountStatus::Active,

            current_status: TransitAccountStatus::Suspended,
        },
    );

    assert!(
        conflicted
            .save_transit_account(&suspended, SaveCondition::IfVersion(first_version,),)
            .await
            .is_ok()
    );

    assert!(
        conflicted
            .append_domain_event(&suspended_event,)
            .await
            .is_ok()
    );

    let conflicting_event = event(
        version(2),
        event_time(1_700_000_000_002),
        DomainEventPayload::TransitAccountStatusChanged {
            account_id,

            previous_status: TransitAccountStatus::Active,

            current_status: TransitAccountStatus::Suspended,
        },
    );

    assert!(
        conflicted
            .append_domain_event(&conflicting_event,)
            .await
            .is_err()
    );

    assert!(conflicted.rollback().await.is_ok());

    let stored_status = match sqlx::query_scalar::<_, String>(
        "SELECT status \
             FROM transit_accounts \
             WHERE id = $1",
    )
    .bind(account_id.into_uuid())
    .fetch_one(manager.pool())
    .await
    {
        Ok(status) => status,

        Err(error) => {
            panic!(
                "account status query failed: \
                     {error}"
            )
        }
    };

    let event_count = match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
             FROM domain_events",
    )
    .fetch_one(manager.pool())
    .await
    {
        Ok(count) => count,

        Err(error) => {
            panic!(
                "event count query failed: \
                     {error}"
            )
        }
    };

    assert_eq!(stored_status, "active");
    assert_eq!(event_count, 1);
}
