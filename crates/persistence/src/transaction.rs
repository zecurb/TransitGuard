use sqlx::{PgPool, Postgres, Transaction};
use transitguard_application::{
    ApplicationTransaction, RepositoryError, RepositoryFuture, SaveCondition, TransactionManager,
    VersionedAggregate,
};
use transitguard_domain::{
    AggregateVersion, DomainEvent, FareCredential, FareCredentialId, ReaderEquipment, ReaderId,
    TransitAccount, TransitAccountId,
};

use crate::repositories::domain_event::{DomainEventRecord, INSERT_EVENT_SQL};
use crate::repositories::fare_credential::{
    FIND_CREDENTIAL_SQL, FareCredentialRow, INSERT_CREDENTIAL_SQL, UPDATE_CREDENTIAL_SQL,
};
use crate::repositories::reader_equipment::{
    FIND_READER_SQL, INSERT_READER_SQL, ReaderEquipmentRow, UPDATE_READER_SQL,
};
use crate::repositories::transit_account::{
    FIND_ACCOUNT_SQL, INSERT_ACCOUNT_SQL, TransitAccountRow, UPDATE_ACCOUNT_SQL,
};
use crate::{PersistenceError, PostgresValueCodec};

const TRANSACTION_ENTITY: &str = "application transaction";
const ACCOUNT_ENTITY: &str = "transit account";
const CREDENTIAL_ENTITY: &str = "fare credential";
const READER_ENTITY: &str = "reader equipment";
const EVENT_ENTITY: &str = "domain event";

/// Starts PostgreSQL-backed application transactions.
#[derive(Clone, Debug)]
pub struct PostgresTransactionManager {
    pool: PgPool,
}

impl PostgresTransactionManager {
    /// Creates a transaction manager using the supplied PostgreSQL pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying PostgreSQL connection pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}

struct PostgresApplicationTransaction {
    transaction: Transaction<'static, Postgres>,
}

impl PostgresApplicationTransaction {
    async fn find_account(
        &mut self,
        account_id: TransitAccountId,
    ) -> Result<Option<VersionedAggregate<TransitAccount>>, PersistenceError> {
        let row = sqlx::query_as::<_, TransitAccountRow>(FIND_ACCOUNT_SQL)
            .bind(account_id.into_uuid())
            .fetch_optional(&mut *self.transaction)
            .await
            .map_err(|source| {
                PersistenceError::database("find transit account in transaction", source)
            })?;

        row.map(TransitAccountRow::into_versioned).transpose()
    }

    async fn save_account(
        &mut self,
        account: &TransitAccount,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => {
                let row = TransitAccountRow::from_domain(account, 1);

                sqlx::query(INSERT_ACCOUNT_SQL)
                    .bind(row.id)
                    .bind(row.rider_id)
                    .bind(row.status)
                    .bind(row.eligibility)
                    .bind(row.stored_value_minor_units)
                    .bind(row.stored_value_currency)
                    .bind(row.aggregate_version)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "insert transit account in transaction",
                            "transit account",
                            source,
                        )
                    })?;

                Ok(())
            }

            SaveCondition::IfVersion(expected_version) => {
                let expected_version =
                    encode_next_version(expected_version, "transit_accounts.aggregate_version")?;

                let row = TransitAccountRow::from_domain(account, expected_version.next);

                let result = sqlx::query(UPDATE_ACCOUNT_SQL)
                    .bind(row.id)
                    .bind(row.rider_id)
                    .bind(row.status)
                    .bind(row.eligibility)
                    .bind(row.stored_value_minor_units)
                    .bind(row.stored_value_currency)
                    .bind(row.aggregate_version)
                    .bind(expected_version.current)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "update transit account in transaction",
                            "transit account",
                            source,
                        )
                    })?;

                enforce_single_write(result.rows_affected(), ACCOUNT_ENTITY)
            }
        }
    }

    async fn find_credential(
        &mut self,
        credential_id: FareCredentialId,
    ) -> Result<Option<VersionedAggregate<FareCredential>>, PersistenceError> {
        let row = sqlx::query_as::<_, FareCredentialRow>(FIND_CREDENTIAL_SQL)
            .bind(credential_id.into_uuid())
            .fetch_optional(&mut *self.transaction)
            .await
            .map_err(|source| {
                PersistenceError::database("find fare credential in transaction", source)
            })?;

        row.map(FareCredentialRow::into_versioned).transpose()
    }

    async fn save_credential(
        &mut self,
        credential: &FareCredential,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => {
                let row = FareCredentialRow::from_domain(credential, 1);

                sqlx::query(INSERT_CREDENTIAL_SQL)
                    .bind(row.id)
                    .bind(row.transit_account_id)
                    .bind(row.kind)
                    .bind(row.status)
                    .bind(row.revocation_reason)
                    .bind(row.replacement_id)
                    .bind(row.aggregate_version)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "insert fare credential in transaction",
                            "fare credential",
                            source,
                        )
                    })?;

                Ok(())
            }

            SaveCondition::IfVersion(expected_version) => {
                let expected_version =
                    encode_next_version(expected_version, "fare_credentials.aggregate_version")?;

                let row = FareCredentialRow::from_domain(credential, expected_version.next);

                let result = sqlx::query(UPDATE_CREDENTIAL_SQL)
                    .bind(row.id)
                    .bind(row.transit_account_id)
                    .bind(row.kind)
                    .bind(row.status)
                    .bind(row.revocation_reason)
                    .bind(row.replacement_id)
                    .bind(row.aggregate_version)
                    .bind(expected_version.current)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "update fare credential in transaction",
                            "fare credential",
                            source,
                        )
                    })?;

                enforce_single_write(result.rows_affected(), CREDENTIAL_ENTITY)
            }
        }
    }

    async fn find_reader(
        &mut self,
        reader_id: ReaderId,
    ) -> Result<Option<VersionedAggregate<ReaderEquipment>>, PersistenceError> {
        let row = sqlx::query_as::<_, ReaderEquipmentRow>(FIND_READER_SQL)
            .bind(reader_id.into_uuid())
            .fetch_optional(&mut *self.transaction)
            .await
            .map_err(|source| {
                PersistenceError::database("find reader equipment in transaction", source)
            })?;

        row.map(ReaderEquipmentRow::into_versioned).transpose()
    }

    async fn save_reader(
        &mut self,
        reader: &ReaderEquipment,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => {
                let row = ReaderEquipmentRow::from_domain(reader, 1);

                sqlx::query(INSERT_READER_SQL)
                    .bind(row.id)
                    .bind(row.equipment_key_id)
                    .bind(row.status)
                    .bind(row.disablement_reason)
                    .bind(row.revocation_reason)
                    .bind(row.aggregate_version)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "insert reader equipment in transaction",
                            "reader equipment",
                            source,
                        )
                    })?;

                Ok(())
            }

            SaveCondition::IfVersion(expected_version) => {
                let expected_version =
                    encode_next_version(expected_version, "reader_equipment.aggregate_version")?;

                let row = ReaderEquipmentRow::from_domain(reader, expected_version.next);

                let result = sqlx::query(UPDATE_READER_SQL)
                    .bind(row.id)
                    .bind(row.equipment_key_id)
                    .bind(row.status)
                    .bind(row.disablement_reason)
                    .bind(row.revocation_reason)
                    .bind(row.aggregate_version)
                    .bind(expected_version.current)
                    .execute(&mut *self.transaction)
                    .await
                    .map_err(|source| {
                        PersistenceError::write(
                            "update reader equipment in transaction",
                            "reader equipment",
                            source,
                        )
                    })?;

                enforce_single_write(result.rows_affected(), READER_ENTITY)
            }
        }
    }

    async fn append_event(&mut self, event: &DomainEvent) -> Result<(), PersistenceError> {
        let record = DomainEventRecord::from_domain(event)?;

        sqlx::query(INSERT_EVENT_SQL)
            .bind(record.id)
            .bind(record.aggregate_kind)
            .bind(record.aggregate_id)
            .bind(record.aggregate_version)
            .bind(record.event_name)
            .bind(record.occurred_at_unix_ms)
            .bind(record.payload)
            .execute(&mut *self.transaction)
            .await
            .map_err(|source| {
                PersistenceError::write(
                    "insert domain event in transaction",
                    "domain event",
                    source,
                )
            })?;

        Ok(())
    }
}

impl ApplicationTransaction for PostgresApplicationTransaction {
    fn find_transit_account(
        &mut self,
        account_id: TransitAccountId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<TransitAccount>>> {
        Box::pin(async move {
            self.find_account(account_id)
                .await
                .map_err(|error| repository_error(ACCOUNT_ENTITY, "find by identifier", error))
        })
    }

    fn save_transit_account<'a>(
        &'a mut self,
        account: &'a TransitAccount,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_account(account, condition)
                .await
                .map_err(|error| repository_error(ACCOUNT_ENTITY, "save", error))
        })
    }

    fn find_fare_credential(
        &mut self,
        credential_id: FareCredentialId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<FareCredential>>> {
        Box::pin(async move {
            self.find_credential(credential_id)
                .await
                .map_err(|error| repository_error(CREDENTIAL_ENTITY, "find by identifier", error))
        })
    }

    fn save_fare_credential<'a>(
        &'a mut self,
        credential: &'a FareCredential,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_credential(credential, condition)
                .await
                .map_err(|error| repository_error(CREDENTIAL_ENTITY, "save", error))
        })
    }

    fn find_reader_equipment(
        &mut self,
        reader_id: ReaderId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<ReaderEquipment>>> {
        Box::pin(async move {
            self.find_reader(reader_id)
                .await
                .map_err(|error| repository_error(READER_ENTITY, "find by identifier", error))
        })
    }

    fn save_reader_equipment<'a>(
        &'a mut self,
        reader: &'a ReaderEquipment,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_reader(reader, condition)
                .await
                .map_err(|error| repository_error(READER_ENTITY, "save", error))
        })
    }

    fn append_domain_event<'a>(&'a mut self, event: &'a DomainEvent) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.append_event(event)
                .await
                .map_err(|error| repository_error(EVENT_ENTITY, "append", error))
        })
    }

    fn commit(self: Box<Self>) -> RepositoryFuture<'static, ()> {
        let Self { transaction } = *self;

        Box::pin(async move {
            transaction.commit().await.map_err(|source| {
                repository_error(
                    TRANSACTION_ENTITY,
                    "commit",
                    PersistenceError::database("commit application transaction", source),
                )
            })
        })
    }

    fn rollback(self: Box<Self>) -> RepositoryFuture<'static, ()> {
        let Self { transaction } = *self;

        Box::pin(async move {
            transaction.rollback().await.map_err(|source| {
                repository_error(
                    TRANSACTION_ENTITY,
                    "rollback",
                    PersistenceError::database("rollback application transaction", source),
                )
            })
        })
    }
}

impl TransactionManager for PostgresTransactionManager {
    fn begin(&self) -> RepositoryFuture<'_, Box<dyn ApplicationTransaction>> {
        Box::pin(async move {
            let transaction = self.pool.begin().await.map_err(|source| {
                repository_error(
                    TRANSACTION_ENTITY,
                    "begin",
                    PersistenceError::database("begin application transaction", source),
                )
            })?;

            Ok(Box::new(PostgresApplicationTransaction { transaction })
                as Box<dyn ApplicationTransaction>)
        })
    }
}

struct EncodedVersion {
    current: i64,
    next: i64,
}

fn encode_next_version(
    version: AggregateVersion,
    field: &'static str,
) -> Result<EncodedVersion, PersistenceError> {
    let current = PostgresValueCodec::encode_aggregate_version(version)?;

    let next = current
        .checked_add(1)
        .ok_or(PersistenceError::NumericValueOutOfRange { field })?;

    Ok(EncodedVersion { current, next })
}

fn enforce_single_write(rows_affected: u64, entity: &'static str) -> Result<(), PersistenceError> {
    if rows_affected != 1 {
        return Err(PersistenceError::WriteConditionFailed { entity });
    }

    Ok(())
}

fn repository_error(
    entity: &'static str,
    operation: &'static str,
    error: PersistenceError,
) -> RepositoryError {
    RepositoryError::new(entity, operation, error)
}
