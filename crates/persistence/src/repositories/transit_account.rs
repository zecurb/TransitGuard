use sqlx::{FromRow, PgPool};
use transitguard_application::{
    RepositoryError, RepositoryFuture, SaveCondition, TransitAccountRepository, VersionedAggregate,
};
use transitguard_domain::{Money, RiderId, TransitAccount, TransitAccountId, TransitAccountStatus};
use uuid::Uuid;

use crate::{PersistenceError, PostgresValueCodec};

const ENTITY: &str = "transit account";
const FIND_OPERATION: &str = "find by identifier";
const SAVE_OPERATION: &str = "save";

const FIND_ACCOUNT_SQL: &str = r#"
SELECT
    id,
    rider_id,
    status,
    eligibility,
    stored_value_minor_units,
    stored_value_currency,
    aggregate_version
FROM transit_accounts
WHERE id = $1
"#;

const INSERT_ACCOUNT_SQL: &str = r#"
INSERT INTO transit_accounts (
    id,
    rider_id,
    status,
    eligibility,
    stored_value_minor_units,
    stored_value_currency,
    aggregate_version
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

const UPDATE_ACCOUNT_SQL: &str = r#"
UPDATE transit_accounts
SET
    rider_id = $2,
    status = $3,
    eligibility = $4,
    stored_value_minor_units = $5,
    stored_value_currency = $6,
    aggregate_version = $7,
    updated_at = CURRENT_TIMESTAMP
WHERE
    id = $1
    AND aggregate_version = $8
"#;

/// PostgreSQL implementation of the transit-account repository port.
#[derive(Clone, Debug)]
pub struct PostgresTransitAccountRepository {
    pool: PgPool,
}

impl PostgresTransitAccountRepository {
    /// Creates a repository backed by the supplied PostgreSQL pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying connection pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn find_account(
        &self,
        account_id: TransitAccountId,
    ) -> Result<Option<VersionedAggregate<TransitAccount>>, PersistenceError> {
        let row = sqlx::query_as::<_, TransitAccountRow>(FIND_ACCOUNT_SQL)
            .bind(account_id.into_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("find transit account", source))?;

        row.map(TransitAccountRow::into_versioned).transpose()
    }

    async fn save_account(
        &self,
        account: &TransitAccount,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => self.insert_account(account).await,

            SaveCondition::IfVersion(expected_version) => {
                self.update_account(account, expected_version).await
            }
        }
    }

    async fn insert_account(&self, account: &TransitAccount) -> Result<(), PersistenceError> {
        let row = TransitAccountRow::from_domain(account, 1);

        sqlx::query(INSERT_ACCOUNT_SQL)
            .bind(row.id)
            .bind(row.rider_id)
            .bind(row.status)
            .bind(row.eligibility)
            .bind(row.stored_value_minor_units)
            .bind(row.stored_value_currency)
            .bind(row.aggregate_version)
            .execute(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("insert transit account", source))?;

        Ok(())
    }

    async fn update_account(
        &self,
        account: &TransitAccount,
        expected_version: transitguard_domain::AggregateVersion,
    ) -> Result<(), PersistenceError> {
        let expected_version = PostgresValueCodec::encode_aggregate_version(expected_version)?;

        let next_version =
            expected_version
                .checked_add(1)
                .ok_or(PersistenceError::NumericValueOutOfRange {
                    field: "transit_accounts.\
                             aggregate_version",
                })?;

        let row = TransitAccountRow::from_domain(account, next_version);

        let result = sqlx::query(UPDATE_ACCOUNT_SQL)
            .bind(row.id)
            .bind(row.rider_id)
            .bind(row.status)
            .bind(row.eligibility)
            .bind(row.stored_value_minor_units)
            .bind(row.stored_value_currency)
            .bind(row.aggregate_version)
            .bind(expected_version)
            .execute(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("update transit account", source))?;

        if result.rows_affected() != 1 {
            return Err(PersistenceError::WriteConditionFailed { entity: ENTITY });
        }

        Ok(())
    }
}

impl TransitAccountRepository for PostgresTransitAccountRepository {
    fn find_by_id(
        &self,
        account_id: TransitAccountId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<TransitAccount>>> {
        Box::pin(async move {
            self.find_account(account_id)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, FIND_OPERATION, error))
        })
    }

    fn save<'a>(
        &'a self,
        account: &'a TransitAccount,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_account(account, condition)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, SAVE_OPERATION, error))
        })
    }
}

#[derive(Debug, FromRow)]
struct TransitAccountRow {
    id: Uuid,
    rider_id: Uuid,
    status: String,
    eligibility: String,
    stored_value_minor_units: i64,
    stored_value_currency: String,
    aggregate_version: i64,
}

impl TransitAccountRow {
    fn from_domain(account: &TransitAccount, aggregate_version: i64) -> Self {
        let stored_value = account.stored_value().amount();

        Self {
            id: account.id().into_uuid(),
            rider_id: account.rider_id().into_uuid(),

            status: String::from(PostgresValueCodec::encode_account_status(account.status())),

            eligibility: String::from(PostgresValueCodec::encode_eligibility(
                account.eligibility(),
            )),

            stored_value_minor_units: stored_value.minor_units(),

            stored_value_currency: String::from(PostgresValueCodec::encode_currency(
                stored_value.currency(),
            )),

            aggregate_version,
        }
    }

    fn into_versioned(self) -> Result<VersionedAggregate<TransitAccount>, PersistenceError> {
        let account_id =
            TransitAccountId::try_from(self.id).map_err(|_| invalid("transit_accounts.id"))?;

        let rider_id =
            RiderId::try_from(self.rider_id).map_err(|_| invalid("transit_accounts.rider_id"))?;

        let status = PostgresValueCodec::decode_account_status(&self.status)?;

        let eligibility = PostgresValueCodec::decode_eligibility(&self.eligibility)?;

        let currency = PostgresValueCodec::decode_currency(self.stored_value_currency.trim())?;

        let initial_balance = Money::from_minor_units(self.stored_value_minor_units, currency);

        let mut account = TransitAccount::new(account_id, rider_id, eligibility, initial_balance)
            .map_err(|_| {
            invalid(
                "transit_accounts.\
                     stored_value_minor_units",
            )
        })?;

        match status {
            TransitAccountStatus::Active => {}

            TransitAccountStatus::Suspended => {
                account
                    .suspend()
                    .map_err(|_| invalid("transit_accounts.status"))?;
            }

            TransitAccountStatus::Closed => {
                account.close();
            }
        }

        let version = PostgresValueCodec::decode_aggregate_version(self.aggregate_version)?;

        Ok(VersionedAggregate::new(account, version))
    }
}

const fn invalid(field: &'static str) -> PersistenceError {
    PersistenceError::InvalidStoredValue { field }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        AggregateVersion, Currency, EligibilityClassification, Money, RiderId, TransitAccount,
        TransitAccountId, TransitAccountStatus,
    };
    use uuid::Uuid;

    use super::TransitAccountRow;
    use crate::PersistenceError;

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

    fn account(eligibility: EligibilityClassification, minor_units: i64) -> TransitAccount {
        match TransitAccount::new(
            TransitAccountId::generate(),
            RiderId::generate(),
            eligibility,
            Money::from_minor_units(minor_units, Currency::Usd),
        ) {
            Ok(account) => account,

            Err(error) => {
                panic!(
                    "valid test account failed: \
                     {error}"
                )
            }
        }
    }

    fn assert_round_trip(account: &TransitAccount, expected_version: AggregateVersion) {
        let row = TransitAccountRow::from_domain(
            account,
            i64::try_from(expected_version.value()).unwrap_or_else(|error| {
                panic!(
                    "test version conversion \
                         failed: {error}"
                )
            }),
        );

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!(
                    "row reconstruction failed: \
                     {error}"
                )
            }
        };

        assert_eq!(loaded.aggregate(), account);

        assert_eq!(loaded.version(), expected_version);
    }

    #[test]
    fn active_account_round_trips() {
        let account = account(EligibilityClassification::Standard, 2_500);

        assert_round_trip(&account, version(1));
    }

    #[test]
    fn suspended_account_round_trips() {
        let mut account = account(EligibilityClassification::ReducedFare, 1_500);

        assert!(account.suspend().is_ok());

        assert_round_trip(&account, version(4));
    }

    #[test]
    fn closed_account_round_trips() {
        let mut account = account(EligibilityClassification::Senior, 500);

        account.close();

        assert_round_trip(&account, version(8));
    }

    #[test]
    fn invalid_account_identifier_is_rejected() {
        let account = account(EligibilityClassification::Youth, 100);

        let mut row = TransitAccountRow::from_domain(&account, 1);

        row.id = Uuid::nil();

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "transit_accounts.id"
            })
        ));
    }

    #[test]
    fn invalid_account_status_is_rejected() {
        let account = account(EligibilityClassification::Standard, 100);

        let mut row = TransitAccountRow::from_domain(&account, 1);

        row.status = String::from("unexpected");

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "transit_accounts.status"
            })
        ));
    }

    #[test]
    fn negative_balance_is_rejected() {
        let account = account(EligibilityClassification::Standard, 100);

        let mut row = TransitAccountRow::from_domain(&account, 1);

        row.stored_value_minor_units = -1;

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "transit_accounts.\
                             stored_value_minor_units"
            })
        ));
    }

    #[test]
    fn invalid_aggregate_version_is_rejected() {
        let account = account(EligibilityClassification::Standard, 100);

        let row = TransitAccountRow::from_domain(&account, 0);

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "aggregate_version"
            })
        ));
    }

    #[test]
    fn stored_status_is_preserved() {
        let mut account = account(EligibilityClassification::EmployeeTestAccount, 750);

        assert!(account.suspend().is_ok());

        let row = TransitAccountRow::from_domain(&account, 3);

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!(
                    "row reconstruction failed: \
                     {error}"
                )
            }
        };

        assert_eq!(loaded.aggregate().status(), TransitAccountStatus::Suspended);

        assert_eq!(
            loaded.aggregate().eligibility(),
            EligibilityClassification::EmployeeTestAccount
        );
    }
}
