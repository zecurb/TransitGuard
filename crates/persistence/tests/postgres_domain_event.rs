use std::env;

use sqlx::PgPool;
use transitguard_application::DomainEventRepository;
use transitguard_domain::{
    AggregateVersion, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
    FareCredentialId, FareCredentialKind, FareCredentialStatus, TransitAccountId,
};
use transitguard_persistence::{
    PostgresConfig, PostgresDomainEventRepository, connect_postgres, run_postgres_migrations,
};
use uuid::Uuid;

fn version(value: u64) -> AggregateVersion {
    match AggregateVersion::new(value) {
        Ok(version) => version,

        Err(error) => {
            panic!("valid version failed: {error}")
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

fn domain_event(
    id: DomainEventId,
    version: AggregateVersion,
    time: DomainEventTime,
    payload: DomainEventPayload,
) -> DomainEvent {
    match DomainEvent::new(id, version, time, payload) {
        Ok(event) => event,

        Err(error) => {
            panic!(
                "valid domain event failed: \
                 {error}"
            )
        }
    }
}

async fn stored_event(
    pool: &PgPool,
    event_id: Uuid,
) -> (String, Uuid, i64, String, i64, serde_json::Value) {
    let query = r#"
SELECT
    aggregate_kind,
    aggregate_id,
    aggregate_version,
    event_name,
    occurred_at_unix_ms,
    payload
FROM domain_events
WHERE id = $1
"#;

    match sqlx::query_as(query).bind(event_id).fetch_one(pool).await {
        Ok(row) => row,

        Err(error) => {
            panic!(
                "stored event query failed: \
                 {error}"
            )
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn domain_event_repository_appends_immutable_events() {
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

    let repository = PostgresDomainEventRepository::new(pool);

    let credential_id = FareCredentialId::generate();

    let account_id = TransitAccountId::generate();

    let event = domain_event(
        DomainEventId::generate(),
        version(1),
        event_time(1_700_000_000_000),
        DomainEventPayload::FareCredentialIssued {
            credential_id,
            account_id,
            kind: FareCredentialKind::Mobile,
        },
    );

    let appended = repository.append(&event).await;

    assert!(appended.is_ok());

    let duplicate = repository.append(&event).await;

    assert!(duplicate.is_err());

    let conflicting_version = domain_event(
        DomainEventId::generate(),
        version(1),
        event_time(1_700_000_000_001),
        DomainEventPayload::FareCredentialStatusChanged {
            credential_id,

            previous_status: FareCredentialStatus::Pending,

            current_status: FareCredentialStatus::Active,
        },
    );

    let conflict = repository.append(&conflicting_version).await;

    assert!(conflict.is_err());

    let stored = stored_event(repository.pool(), event.id().into_uuid()).await;

    assert_eq!(stored.0, "fare_credential");

    assert_eq!(stored.1, credential_id.into_uuid());

    assert_eq!(stored.2, 1);

    assert_eq!(stored.3, "fare_credential.issued");

    assert_eq!(stored.4, 1_700_000_000_000);

    assert!(stored.5.is_object());

    let count = match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM domain_events")
        .fetch_one(repository.pool())
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

    assert_eq!(count, 1);
}
