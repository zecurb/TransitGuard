use serde_json::Value;
use sqlx::PgPool;
use transitguard_application::{DomainEventRepository, RepositoryError, RepositoryFuture};
use transitguard_domain::DomainEvent;
use uuid::Uuid;

use crate::{PersistenceError, PostgresValueCodec};

const ENTITY: &str = "domain event";
const APPEND_OPERATION: &str = "append";

const INSERT_EVENT_SQL: &str = r#"
INSERT INTO domain_events (
    id,
    aggregate_kind,
    aggregate_id,
    aggregate_version,
    event_name,
    occurred_at_unix_ms,
    payload
)
VALUES (
    $1,
    $2,
    $3,
    $4,
    $5,
    $6,
    $7
)
"#;

/// PostgreSQL implementation of the immutable domain-event repository.
#[derive(Clone, Debug)]
pub struct PostgresDomainEventRepository {
    pool: PgPool,
}

impl PostgresDomainEventRepository {
    /// Creates a repository backed by the supplied PostgreSQL pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying PostgreSQL connection pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn append_event(&self, event: &DomainEvent) -> Result<(), PersistenceError> {
        let record = DomainEventRecord::from_domain(event)?;

        sqlx::query(INSERT_EVENT_SQL)
            .bind(record.id)
            .bind(record.aggregate_kind)
            .bind(record.aggregate_id)
            .bind(record.aggregate_version)
            .bind(record.event_name)
            .bind(record.occurred_at_unix_ms)
            .bind(record.payload)
            .execute(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("insert domain event", source))?;

        Ok(())
    }
}

impl DomainEventRepository for PostgresDomainEventRepository {
    fn append<'a>(&'a self, event: &'a DomainEvent) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.append_event(event)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, APPEND_OPERATION, error))
        })
    }
}

#[derive(Debug)]
struct DomainEventRecord {
    id: Uuid,
    aggregate_kind: &'static str,
    aggregate_id: Uuid,
    aggregate_version: i64,
    event_name: &'static str,
    occurred_at_unix_ms: i64,
    payload: Value,
}

impl DomainEventRecord {
    fn from_domain(event: &DomainEvent) -> Result<Self, PersistenceError> {
        let event = *event;

        let (aggregate_kind, aggregate_id) =
            PostgresValueCodec::encode_aggregate_id(event.aggregate_id());

        let aggregate_version =
            PostgresValueCodec::encode_aggregate_version(event.aggregate_version())?;

        let payload = serde_json::to_value(event.payload()).map_err(|source| {
            PersistenceError::serialization("serialize domain event payload", source)
        })?;

        Ok(Self {
            id: event.id().into_uuid(),
            aggregate_kind,
            aggregate_id,
            aggregate_version,
            event_name: event.event_name(),

            occurred_at_unix_ms: PostgresValueCodec::encode_event_time(event.occurred_at()),

            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        AggregateVersion, DomainEvent, DomainEventId, DomainEventPayload, DomainEventTime,
        FareCredentialId, FareCredentialKind, TransitAccountId,
    };

    use super::DomainEventRecord;

    fn event() -> DomainEvent {
        let version = match AggregateVersion::new(3) {
            Ok(version) => version,

            Err(error) => {
                panic!(
                    "valid version failed: \
                         {error}"
                )
            }
        };

        let occurred_at = match DomainEventTime::from_unix_milliseconds(1_700_000_000_000) {
            Ok(occurred_at) => occurred_at,

            Err(error) => {
                panic!(
                    "valid event time failed: \
                         {error}"
                )
            }
        };

        let payload = DomainEventPayload::FareCredentialIssued {
            credential_id: FareCredentialId::generate(),

            account_id: TransitAccountId::generate(),

            kind: FareCredentialKind::Mobile,
        };

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

    #[test]
    fn record_preserves_event_envelope() {
        let event = event();

        let record = match DomainEventRecord::from_domain(&event) {
            Ok(record) => record,

            Err(error) => {
                panic!(
                    "event mapping failed: \
                         {error}"
                )
            }
        };

        assert_eq!(record.id, event.id().into_uuid());

        assert_eq!(record.aggregate_kind, "fare_credential");

        assert_eq!(record.aggregate_version, 3);

        assert_eq!(record.event_name, "fare_credential.issued");

        assert_eq!(record.occurred_at_unix_ms, 1_700_000_000_000);
    }

    #[test]
    fn serialized_payload_is_a_json_object() {
        let event = event();

        let record = match DomainEventRecord::from_domain(&event) {
            Ok(record) => record,

            Err(error) => {
                panic!(
                    "event mapping failed: \
                         {error}"
                )
            }
        };

        assert!(record.payload.is_object());
    }
}
