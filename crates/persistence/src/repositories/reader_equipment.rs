use sqlx::{FromRow, PgPool};
use transitguard_application::{
    ReaderEquipmentRepository, RepositoryError, RepositoryFuture, SaveCondition, VersionedAggregate,
};
use transitguard_domain::{
    AggregateVersion, EquipmentKeyId, ReaderDisablementReason, ReaderEquipment,
    ReaderEquipmentStatus, ReaderId, ReaderRevocationReason,
};
use uuid::Uuid;

use crate::{PersistenceError, PostgresValueCodec};

const ENTITY: &str = "reader equipment";
const FIND_OPERATION: &str = "find by identifier";
const SAVE_OPERATION: &str = "save";

pub(crate) const FIND_READER_SQL: &str = r#"
SELECT
    id,
    equipment_key_id,
    status,
    disablement_reason,
    revocation_reason,
    aggregate_version
FROM reader_equipment
WHERE id = $1
"#;

pub(crate) const INSERT_READER_SQL: &str = r#"
INSERT INTO reader_equipment (
    id,
    equipment_key_id,
    status,
    disablement_reason,
    revocation_reason,
    aggregate_version
)
VALUES (
    $1,
    $2,
    $3,
    $4,
    $5,
    $6
)
"#;

pub(crate) const UPDATE_READER_SQL: &str = r#"
UPDATE reader_equipment
SET
    equipment_key_id = $2,
    status = $3,
    disablement_reason = $4,
    revocation_reason = $5,
    aggregate_version = $6,
    updated_at = CURRENT_TIMESTAMP
WHERE
    id = $1
    AND aggregate_version = $7
"#;

/// PostgreSQL implementation of the reader-equipment repository port.
#[derive(Clone, Debug)]
pub struct PostgresReaderEquipmentRepository {
    pool: PgPool,
}

impl PostgresReaderEquipmentRepository {
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

    async fn find_reader(
        &self,
        reader_id: ReaderId,
    ) -> Result<Option<VersionedAggregate<ReaderEquipment>>, PersistenceError> {
        let row = sqlx::query_as::<_, ReaderEquipmentRow>(FIND_READER_SQL)
            .bind(reader_id.into_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(|source| PersistenceError::database("find reader equipment", source))?;

        row.map(ReaderEquipmentRow::into_versioned).transpose()
    }

    async fn save_reader(
        &self,
        reader: &ReaderEquipment,
        condition: SaveCondition,
    ) -> Result<(), PersistenceError> {
        match condition {
            SaveCondition::MustNotExist => self.insert_reader(reader).await,

            SaveCondition::IfVersion(expected_version) => {
                self.update_reader(reader, expected_version).await
            }
        }
    }

    async fn insert_reader(&self, reader: &ReaderEquipment) -> Result<(), PersistenceError> {
        let row = ReaderEquipmentRow::from_domain(reader, 1);

        sqlx::query(INSERT_READER_SQL)
            .bind(row.id)
            .bind(row.equipment_key_id)
            .bind(row.status)
            .bind(row.disablement_reason)
            .bind(row.revocation_reason)
            .bind(row.aggregate_version)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                PersistenceError::write("insert reader equipment", "reader equipment", source)
            })?;

        Ok(())
    }

    async fn update_reader(
        &self,
        reader: &ReaderEquipment,
        expected_version: AggregateVersion,
    ) -> Result<(), PersistenceError> {
        let expected_version = PostgresValueCodec::encode_aggregate_version(expected_version)?;

        let next_version =
            expected_version
                .checked_add(1)
                .ok_or(PersistenceError::NumericValueOutOfRange {
                    field: "reader_equipment.aggregate_version",
                })?;

        let row = ReaderEquipmentRow::from_domain(reader, next_version);

        let result = sqlx::query(UPDATE_READER_SQL)
            .bind(row.id)
            .bind(row.equipment_key_id)
            .bind(row.status)
            .bind(row.disablement_reason)
            .bind(row.revocation_reason)
            .bind(row.aggregate_version)
            .bind(expected_version)
            .execute(&self.pool)
            .await
            .map_err(|source| {
                PersistenceError::write("update reader equipment", "reader equipment", source)
            })?;

        if result.rows_affected() != 1 {
            return Err(PersistenceError::WriteConditionFailed { entity: ENTITY });
        }

        Ok(())
    }
}

impl ReaderEquipmentRepository for PostgresReaderEquipmentRepository {
    fn find_by_id(
        &self,
        reader_id: ReaderId,
    ) -> RepositoryFuture<'_, Option<VersionedAggregate<ReaderEquipment>>> {
        Box::pin(async move {
            self.find_reader(reader_id)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, FIND_OPERATION, error))
        })
    }

    fn save<'a>(
        &'a self,
        reader: &'a ReaderEquipment,
        condition: SaveCondition,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            self.save_reader(reader, condition)
                .await
                .map_err(|error| RepositoryError::new(ENTITY, SAVE_OPERATION, error))
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct ReaderEquipmentRow {
    pub(crate) id: Uuid,
    pub(crate) equipment_key_id: Uuid,
    pub(crate) status: String,
    pub(crate) disablement_reason: Option<String>,
    pub(crate) revocation_reason: Option<String>,
    pub(crate) aggregate_version: i64,
}

impl ReaderEquipmentRow {
    pub(crate) fn from_domain(reader: &ReaderEquipment, aggregate_version: i64) -> Self {
        Self {
            id: reader.id().into_uuid(),

            equipment_key_id: reader.identity().key_id().into_uuid(),

            status: String::from(PostgresValueCodec::encode_reader_status(reader.status())),

            disablement_reason: reader
                .disablement_reason()
                .map(PostgresValueCodec::encode_reader_disablement_reason)
                .map(String::from),

            revocation_reason: reader
                .revocation_reason()
                .map(PostgresValueCodec::encode_reader_revocation_reason)
                .map(String::from),

            aggregate_version,
        }
    }

    pub(crate) fn into_versioned(
        self,
    ) -> Result<VersionedAggregate<ReaderEquipment>, PersistenceError> {
        let reader_id = ReaderId::try_from(self.id).map_err(|_| invalid("reader_equipment.id"))?;

        let key_id = EquipmentKeyId::try_from(self.equipment_key_id)
            .map_err(|_| invalid("reader_equipment.equipment_key_id"))?;

        let status = PostgresValueCodec::decode_reader_status(&self.status)?;

        let disablement_reason = self
            .disablement_reason
            .as_deref()
            .map(PostgresValueCodec::decode_reader_disablement_reason)
            .transpose()?;

        let revocation_reason = self
            .revocation_reason
            .as_deref()
            .map(PostgresValueCodec::decode_reader_revocation_reason)
            .transpose()?;

        validate_lifecycle_fields(status, disablement_reason, revocation_reason)?;

        let mut reader = ReaderEquipment::new_pending(reader_id, key_id);

        match status {
            ReaderEquipmentStatus::PendingRegistration => {}

            ReaderEquipmentStatus::Active => {
                reader
                    .activate()
                    .map_err(|_| invalid("reader_equipment.status"))?;
            }

            ReaderEquipmentStatus::Offline => {
                reader
                    .activate()
                    .map_err(|_| invalid("reader_equipment.status"))?;

                reader
                    .mark_offline()
                    .map_err(|_| invalid("reader_equipment.status"))?;
            }

            ReaderEquipmentStatus::Disabled => {
                let reason = disablement_reason
                    .ok_or_else(|| invalid("reader_equipment.disablement_reason"))?;

                reader
                    .disable(reason)
                    .map_err(|_| invalid("reader_equipment.status"))?;
            }

            ReaderEquipmentStatus::Revoked => {
                let reason = revocation_reason
                    .ok_or_else(|| invalid("reader_equipment.revocation_reason"))?;

                reader
                    .revoke(reason)
                    .map_err(|_| invalid("reader_equipment.status"))?;
            }

            ReaderEquipmentStatus::Decommissioned => {
                reader
                    .decommission()
                    .map_err(|_| invalid("reader_equipment.status"))?;
            }
        }

        let version = PostgresValueCodec::decode_aggregate_version(self.aggregate_version)?;

        Ok(VersionedAggregate::new(reader, version))
    }
}

fn validate_lifecycle_fields(
    status: ReaderEquipmentStatus,
    disablement_reason: Option<ReaderDisablementReason>,
    revocation_reason: Option<ReaderRevocationReason>,
) -> Result<(), PersistenceError> {
    match status {
        ReaderEquipmentStatus::Disabled => {
            if disablement_reason.is_none() {
                return Err(invalid("reader_equipment.disablement_reason"));
            }

            if revocation_reason.is_some() {
                return Err(invalid("reader_equipment.revocation_reason"));
            }
        }

        ReaderEquipmentStatus::Revoked => {
            if disablement_reason.is_some() {
                return Err(invalid("reader_equipment.disablement_reason"));
            }

            if revocation_reason.is_none() {
                return Err(invalid("reader_equipment.revocation_reason"));
            }
        }

        ReaderEquipmentStatus::PendingRegistration
        | ReaderEquipmentStatus::Active
        | ReaderEquipmentStatus::Offline
        | ReaderEquipmentStatus::Decommissioned => {
            if disablement_reason.is_some() {
                return Err(invalid("reader_equipment.disablement_reason"));
            }

            if revocation_reason.is_some() {
                return Err(invalid("reader_equipment.revocation_reason"));
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
        AggregateVersion, EquipmentKeyId, ReaderDisablementReason, ReaderEquipment,
        ReaderEquipmentStatus, ReaderId, ReaderRevocationReason,
    };
    use uuid::Uuid;

    use super::ReaderEquipmentRow;
    use crate::PersistenceError;

    fn version(value: u64) -> AggregateVersion {
        match AggregateVersion::new(value) {
            Ok(version) => version,

            Err(error) => {
                panic!("valid aggregate version failed: {error}")
            }
        }
    }

    fn pending_reader() -> ReaderEquipment {
        ReaderEquipment::new_pending(ReaderId::generate(), EquipmentKeyId::generate())
    }

    fn row_version(version: AggregateVersion) -> i64 {
        match i64::try_from(version.value()) {
            Ok(value) => value,

            Err(error) => {
                panic!("test version conversion failed: {error}")
            }
        }
    }

    fn assert_round_trip(reader: &ReaderEquipment, expected_version: AggregateVersion) {
        let row = ReaderEquipmentRow::from_domain(reader, row_version(expected_version));

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!("row reconstruction failed: {error}")
            }
        };

        assert_eq!(loaded.aggregate(), reader);
        assert_eq!(loaded.version(), expected_version);
    }

    #[test]
    fn pending_reader_round_trips() {
        assert_round_trip(&pending_reader(), version(1));
    }

    #[test]
    fn active_reader_round_trips() {
        let mut reader = pending_reader();
        assert!(reader.activate().is_ok());

        assert_round_trip(&reader, version(2));
    }

    #[test]
    fn offline_reader_round_trips() {
        let mut reader = pending_reader();

        assert!(reader.activate().is_ok());
        assert!(reader.mark_offline().is_ok());

        assert_round_trip(&reader, version(3));
    }

    #[test]
    fn disabled_reader_round_trips() {
        let mut reader = pending_reader();

        assert!(
            reader
                .disable(ReaderDisablementReason::InvalidConfiguration)
                .is_ok()
        );

        assert_round_trip(&reader, version(4));
    }

    #[test]
    fn revoked_reader_round_trips() {
        let mut reader = pending_reader();

        assert!(
            reader
                .revoke(ReaderRevocationReason::CredentialExposure)
                .is_ok()
        );

        assert_round_trip(&reader, version(5));
    }

    #[test]
    fn decommissioned_reader_round_trips() {
        let mut reader = pending_reader();

        assert!(reader.decommission().is_ok());

        assert_round_trip(&reader, version(6));
    }

    #[test]
    fn invalid_reader_identifier_is_rejected() {
        let reader = pending_reader();
        let mut row = ReaderEquipmentRow::from_domain(&reader, 1);

        row.id = Uuid::nil();

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.id"
            })
        ));
    }

    #[test]
    fn invalid_equipment_key_identifier_is_rejected() {
        let reader = pending_reader();
        let mut row = ReaderEquipmentRow::from_domain(&reader, 1);

        row.equipment_key_id = Uuid::nil();

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.equipment_key_id"
            })
        ));
    }

    #[test]
    fn disabled_status_requires_a_reason() {
        let mut reader = pending_reader();

        assert!(
            reader
                .disable(ReaderDisablementReason::AdministrativeAction)
                .is_ok()
        );

        let mut row = ReaderEquipmentRow::from_domain(&reader, 2);
        row.disablement_reason = None;

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.disablement_reason"
            })
        ));
    }

    #[test]
    fn revoked_status_requires_a_reason() {
        let mut reader = pending_reader();

        assert!(
            reader
                .revoke(ReaderRevocationReason::SecurityIncident)
                .is_ok()
        );

        let mut row = ReaderEquipmentRow::from_domain(&reader, 2);
        row.revocation_reason = None;

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.revocation_reason"
            })
        ));
    }

    #[test]
    fn active_status_rejects_disablement_reason() {
        let mut reader = pending_reader();
        assert!(reader.activate().is_ok());

        let mut row = ReaderEquipmentRow::from_domain(&reader, 2);
        row.disablement_reason = Some(String::from("administrative_action"));

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.disablement_reason"
            })
        ));
    }

    #[test]
    fn active_status_rejects_revocation_reason() {
        let mut reader = pending_reader();
        assert!(reader.activate().is_ok());

        let mut row = ReaderEquipmentRow::from_domain(&reader, 2);
        row.revocation_reason = Some(String::from("security_incident"));

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "reader_equipment.revocation_reason"
            })
        ));
    }

    #[test]
    fn invalid_aggregate_version_is_rejected() {
        let reader = pending_reader();
        let row = ReaderEquipmentRow::from_domain(&reader, 0);

        let result = row.into_versioned();

        assert!(matches!(
            result,
            Err(PersistenceError::InvalidStoredValue {
                field: "aggregate_version"
            })
        ));
    }

    #[test]
    fn reconstructed_identity_is_preserved() {
        let reader = pending_reader();
        let row = ReaderEquipmentRow::from_domain(&reader, 3);

        let loaded = match row.into_versioned() {
            Ok(loaded) => loaded,

            Err(error) => {
                panic!("row reconstruction failed: {error}")
            }
        };

        assert_eq!(loaded.aggregate().identity(), reader.identity());

        assert_eq!(
            loaded.aggregate().status(),
            ReaderEquipmentStatus::PendingRegistration
        );
    }
}
