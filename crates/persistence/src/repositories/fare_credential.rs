use sqlx::{FromRow, PgPool};
use transitguard_application::{
    FareCredentialRepository, RepositoryError, RepositoryFuture, SaveCondition, VersionedAggregate,
};
use transitguard_domain::{
    AggregateVersion, FareCredential, FareCredentialId, FareCredentialStatus, RevocationReason,
    TransitAccountId,
};
use uuid::Uuid;

use crate::{PersistenceError, PostgresValueCodec};

const ENTITY: &str = "fare credential";
const FIND_OPERATION: &str = "find by identifier";
const SAVE_OPERATION: &str = "save";

const FIND_CREDENTIAL_SQL: &str = r#"
SELECT
    id,
    transit_account_id,
    kind,
    status,
    revocation_reason,
    replacement_id,
    aggregate_version
FROM fare_credentials
WHERE id = $1
"#;

const INSERT_CREDENTIAL_SQL: &str = r#"
INSERT INTO fare_credentials (
    id,
    transit_account_id,
    kind,
    status,
    revocation_reason,
    replacement_id,
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

const UPDATE_CREDENTIAL_SQL: &str = r#"
UPDATE fare_credentials
SET
    transit_account_id = $2,
    kind = $3,
    status = $4,
    revocation_reason = $5,
    replacement_id = $6,
    aggregate_version = $7,
    updated_at = CURRENT_TIMESTAMP
WHERE
    id = $1
    AND aggregate_version = $8
"#;

/// PostgreSQL implementation of the fare-credential repository port.
#[derive(Clone, Debug)]
pub struct PostgresFareCredentialRepository {
    pool: PgPool,
}

impl PostgresFareCredentialRepository {
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

    async fn find_credential(
        &self,
        credential_id: FareCredentialId,
    ) -> Result<Option<VersionedAggregate<FareCredential>>, PersistenceError> {
        let row = sqlx::query_as::<_, FareCredentialRow>(FIND_CREDENTIAL_SQL)
            .bind(credential_id.into_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("find fare credential", source))?;

        row.map(FareCredentialRow::into_versioned).transpose()
    }

    async fn save_credential(
        &self,
        credential: &FareCredential,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => self.insert_credential(credential).await,

            SaveCondition::IfVersion(expected_version) => {
                self.update_credential(credential, expected_version).await
            }
        }
    }

    async fn insert_credential(&self, credential: &FareCredential) -> Result<(), PersistenceError> {
        let row = FareCredentialRow::from_domain(credential, 1);

        sqlx::query(INSERT_CREDENTIAL_SQL)
            .bind(row.id)
            .bind(row.transit_account_id)
            .bind(row.kind)
            .bind(row.status)
            .bind(row.revocation_reason)
            .bind(row.replacement_id)
            .bind(row.aggregate_version)
            .execute(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("insert fare credential", source))?;

        Ok(())
    }

    async fn update_credential(
        &self,
        credential: &FareCredential,
        expected_version: AggregateVersion,
    ) -> Result<(), PersistenceError> {
        let expected_version = PostgresValueCodec::encode_aggregate_version(expected_version)?;

        let next_version =
            expected_version
                .checked_add(1)
                .ok_or(PersistenceError::NumericValueOutOfRange {
                    field: "fare_credentials.aggregate_version",
                })?;

        let row = FareCredentialRow::from_domain(credential, next_version);

        let result = sqlx::query(UPDATE_CREDENTIAL_SQL)
            .bind(row.id)
            .bind(row.transit_account_id)
            .bind(row.kind)
            .bind(row.status)
            .bind(row.revocation_reason)
            .bind(row.replacement_id)
            .bind(row.aggregate_version)
            .bind(expected_version)
            .execute(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("update fare credential", source))?;

        if result.rows_affected() != 1 {
            return Err(PersistenceError::WriteConditionFailed { entity: ENTITY });
        }

        Ok(())
    }
}

impl FareCredentialRepository for PostgresFareCredentialRepository {
    fn find_by_id(
        &self,
        credential_id: FareCredentialId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<FareCredential>>> {
        Box::pin(async move {
            self.find_credential(credential_id)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, FIND_OPERATION, error))
        })
    }

    fn save<'a>(
        &'a self,
        credential: &'a FareCredential,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_credential(credential, condition)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, SAVE_OPERATION, error))
        })
    }
}

#[derive(Debug, FromRow)]
struct FareCredentialRow {
    id: Uuid,
    transit_account_id: Uuid,
    kind: String,
    status: String,
    revocation_reason: Option<String>,
    replacement_id: Option<Uuid>,
    aggregate_version: i64,
}

impl FareCredentialRow {
    fn from_domain(credential: &FareCredential, aggregate_version: i64) -> Self {
        Self {
            id: credential.id().into_uuid(),
            transit_account_id: credential.transit_account_id().into_uuid(),

            kind: String::from(PostgresValueCodec::encode_credential_kind(
                credential.kind(),
            )),

            status: String::from(PostgresValueCodec::encode_credential_status(
                credential.status(),
            )),

            revocation_reason: credential
                .revocation_reason()
                .map(PostgresValueCodec::encode_credential_revocation_reason)
                .map(String::from),

            replacement_id: credential.replacement_id().map(FareCredentialId::into_uuid),

            aggregate_version,
        }
    }

    fn into_versioned(self) -> Result<VersionedAggregate<FareCredential>, PersistenceError> {
        let credential_id =
            FareCredentialId::try_from(self.id).map_err(|_| invalid("fare_credentials.id"))?;

        let account_id = TransitAccountId::try_from(self.transit_account_id)
            .map_err(|_| invalid("fare_credentials.transit_account_id"))?;

        let kind = PostgresValueCodec::decode_credential_kind(&self.kind)?;

        let status = PostgresValueCodec::decode_credential_status(&self.status)?;

        let revocation_reason = self
            .revocation_reason
            .as_deref()
            .map(PostgresValueCodec::decode_credential_revocation_reason)
            .transpose()?;

        let replacement_id = self
            .replacement_id
            .map(FareCredentialId::try_from)
            .transpose()
            .map_err(|_| invalid("fare_credentials.replacement_id"))?;

        validate_lifecycle_fields(status, revocation_reason, replacement_id)?;

        let mut credential = FareCredential::new_pending(credential_id, account_id, kind);

        match status {
            FareCredentialStatus::Pending => {}

            FareCredentialStatus::Active => {
                credential
                    .activate()
                    .map_err(|_| invalid("fare_credentials.status"))?;
            }

            FareCredentialStatus::Suspended => {
                credential
                    .activate()
                    .map_err(|_| invalid("fare_credentials.status"))?;

                credential
                    .suspend()
                    .map_err(|_| invalid("fare_credentials.status"))?;
            }

            FareCredentialStatus::Revoked => {
                let reason = revocation_reason
                    .ok_or_else(|| invalid("fare_credentials.revocation_reason"))?;

                credential
                    .revoke(reason)
                    .map_err(|_| invalid("fare_credentials.status"))?;
            }

            FareCredentialStatus::Expired => {
                credential
                    .expire()
                    .map_err(|_| invalid("fare_credentials.status"))?;
            }

            FareCredentialStatus::Replaced => {
                let replacement_id =
                    replacement_id.ok_or_else(|| invalid("fare_credentials.replacement_id"))?;

                credential
                    .activate()
                    .map_err(|_| invalid("fare_credentials.status"))?;

                credential
                    .replace_with(replacement_id)
                    .map_err(|_| invalid("fare_credentials.replacement_id"))?;
            }
        }

        let version = PostgresValueCodec::decode_aggregate_version(self.aggregate_version)?;

        Ok(VersionedAggregate::new(credential, version))
    }
}

fn validate_lifecycle_fields(
    status: FareCredentialStatus,
    revocation_reason: Option<RevocationReason>,
    replacement_id: Option<FareCredentialId>,
) -> Result<(), PersistenceError> {
    match status {
        FareCredentialStatus::Revoked => {
            if revocation_reason.is_none() {
                return Err(invalid("fare_credentials.revocation_reason"));
            }

            if replacement_id.is_some() {
                return Err(invalid("fare_credentials.replacement_id"));
            }
        }

        FareCredentialStatus::Replaced => {
            if revocation_reason.is_some() {
                return Err(invalid("fare_credentials.revocation_reason"));
            }

            if replacement_id.is_none() {
                return Err(invalid("fare_credentials.replacement_id"));
            }
        }

        FareCredentialStatus::Pending
        | FareCredentialStatus::Active
        | FareCredentialStatus::Suspended
        | FareCredentialStatus::Expired => {
            if revocation_reason.is_some() {
                return Err(invalid("fare_credentials.revocation_reason"));
            }

            if replacement_id.is_some() {
                return Err(invalid("fare_credentials.replacement_id"));
            }
        }
    }

    Ok(())
}

const fn invalid(field: &'static str) -> PersistenceError {
    PersistenceError::InvalidStoredValue { field }
}

#[cfg(test)]
mod tests {
    use transitguard_domain::{
        AggregateVersion, FareCredential, FareCredentialId, FareCredentialKind,
        FareCredentialStatus, RevocationReason, TransitAccountId,
    };
    use uuid::Uuid;

    use super::FareCredentialRow;
    use crate::PersistenceError;

    fn version(value: u64) -> AggregateVersion {
        match AggregateVersion::new(value) {
            Ok(version) => version,

            Err(error) => {
                panic!("valid aggregate version failed: {error}")
            }
        }
    }

    fn pending_credential() -> FareCredential {
        FareCredential::new_pending(
            FareCredentialId::generate(),
            TransitAccountId::generate(),
            FareCredentialKind::Card,
        )
    }

    fn row_version(version: AggregateVersion) -> i64 {
        match i64::try_from(version.value()) {
            Ok(value) => value,

            Err(error) => {
                panic!("test version conversion failed: {error}")
            }
        }
    }

    fn assert_round_trip(credential: &FareCredential, expected_version: AggregateVersion) {
        let row = FareCredentialRow::from_domain(credential, row_version(expected_version));

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!("row reconstruction failed: {error}")
            }
        };

        assert_eq!(loaded.aggregate(), credential);
        assert_eq!(loaded.version(), expected_version);
    }

    #[test]
    fn pending_credential_round_trips() {
        assert_round_trip(&pending_credential(), version(1));
    }

    #[test]
    fn active_credential_round_trips() {
        let mut credential = pending_credential();
        assert!(credential.activate().is_ok());

        assert_round_trip(&credential, version(2));
    }

    #[test]
    fn suspended_credential_round_trips() {
        let mut credential = pending_credential();

        assert!(credential.activate().is_ok());
        assert!(credential.suspend().is_ok());

        assert_round_trip(&credential, version(3));
    }

    #[test]
    fn revoked_credential_round_trips() {
        let mut credential = pending_credential();

        assert!(credential.revoke(RevocationReason::ReportedLost).is_ok());

        assert_round_trip(&credential, version(4));
    }

    #[test]
    fn expired_credential_round_trips() {
        let mut credential = pending_credential();
        assert!(credential.expire().is_ok());

        assert_round_trip(&credential, version(5));
    }

    #[test]
    fn replaced_credential_round_trips() {
        let mut credential = pending_credential();
        let replacement_id = FareCredentialId::generate();

        assert!(credential.activate().is_ok());
        assert!(credential.replace_with(replacement_id).is_ok());

        assert_round_trip(&credential, version(6));
    }

    #[test]
    fn invalid_credential_identifier_is_rejected() {
        let credential = pending_credential();
        let mut row = FareCredentialRow::from_domain(&credential, 1);

        row.id = Uuid::nil();

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.id"
            })
        ));
    }

    #[test]
    fn invalid_account_identifier_is_rejected() {
        let credential = pending_credential();
        let mut row = FareCredentialRow::from_domain(&credential, 1);

        row.transit_account_id = Uuid::nil();

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.transit_account_id"
            })
        ));
    }

    #[test]
    fn revoked_status_requires_a_reason() {
        let mut credential = pending_credential();

        assert!(
            credential
                .revoke(RevocationReason::SecurityIncident)
                .is_ok()
        );

        let mut row = FareCredentialRow::from_domain(&credential, 2);
        row.revocation_reason = None;

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.revocation_reason"
            })
        ));
    }

    #[test]
    fn active_status_rejects_a_revocation_reason() {
        let mut credential = pending_credential();
        assert!(credential.activate().is_ok());

        let mut row = FareCredentialRow::from_domain(&credential, 2);
        row.revocation_reason = Some(String::from("reported_lost"));

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.revocation_reason"
            })
        ));
    }

    #[test]
    fn replaced_status_requires_a_successor() {
        let mut credential = pending_credential();

        assert!(credential.activate().is_ok());

        assert!(
            credential
                .replace_with(FareCredentialId::generate())
                .is_ok()
        );

        let mut row = FareCredentialRow::from_domain(&credential, 2);
        row.replacement_id = None;

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.replacement_id"
            })
        ));
    }

    #[test]
    fn self_replacement_is_rejected() {
        let credential = pending_credential();
        let mut row = FareCredentialRow::from_domain(&credential, 2);

        row.status = String::from("replaced");
        row.replacement_id = Some(credential.id().into_uuid());

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "fare_credentials.replacement_id"
            })
        ));
    }

    #[test]
    fn invalid_aggregate_version_is_rejected() {
        let credential = pending_credential();
        let row = FareCredentialRow::from_domain(&credential, 0);

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "aggregate_version"
            })
        ));
    }

    #[test]
    fn reconstructed_status_matches_stored_status() {
        let mut credential = pending_credential();

        assert!(credential.activate().is_ok());
        assert!(credential.suspend().is_ok());

        let row = FareCredentialRow::from_domain(&credential, 3);

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!("row reconstruction failed: {error}")
            }
        };

        assert_eq!(loaded.aggregate().status(), FareCredentialStatus::Suspended);
    }
}
