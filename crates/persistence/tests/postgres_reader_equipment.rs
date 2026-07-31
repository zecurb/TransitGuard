use std::env;

use transitguard_application::{ReaderEquipmentRepository, SaveCondition};
use transitguard_domain::{EquipmentKeyId, ReaderEquipment, ReaderId};
use transitguard_persistence::{
    PostgresConfig, PostgresReaderEquipmentRepository, connect_postgres, run_postgres_migrations,
};

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn reader_equipment_repository_enforces_versions() {
    let database_url = match env::var("DATABASE_URL") {
        Ok(database_url) => database_url,

        Err(error) => {
            panic!("DATABASE_URL is required: {error}")
        }
    };

    let config = match PostgresConfig::new(database_url) {
        Ok(config) => config,

        Err(error) => {
            panic!("database configuration failed: {error}")
        }
    };

    let pool = match connect_postgres(&config).await {
        Ok(pool) => pool,

        Err(error) => {
            panic!("database connection failed: {error}")
        }
    };

    if let Err(error) = run_postgres_migrations(&pool).await {
        panic!("database migrations failed: {error}");
    }

    let repository = PostgresReaderEquipmentRepository::new(pool);

    let reader_id = ReaderId::generate();

    let mut reader = ReaderEquipment::new_pending(reader_id, EquipmentKeyId::generate());

    let inserted = repository.save(&reader, SaveCondition::MustNotExist).await;

    assert!(inserted.is_ok());

    let duplicate = repository.save(&reader, SaveCondition::MustNotExist).await;

    assert!(duplicate.is_err());

    let loaded = match repository.find_by_id(reader_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("inserted reader was not found")
        }

        Err(error) => {
            panic!("reader lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate(), &reader);
    assert_eq!(loaded.version().value(), 1);

    let first_version = loaded.version();

    assert!(reader.activate().is_ok());

    let updated = repository
        .save(&reader, SaveCondition::IfVersion(first_version))
        .await;

    assert!(updated.is_ok());

    let loaded = match repository.find_by_id(reader_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("updated reader was not found")
        }

        Err(error) => {
            panic!("updated reader lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate(), &reader);
    assert_eq!(loaded.version().value(), 2);

    let stale_update = repository
        .save(&reader, SaveCondition::IfVersion(first_version))
        .await;

    assert!(stale_update.is_err());

    let second_version = loaded.version();
    let rotated_key = EquipmentKeyId::generate();

    assert!(reader.rotate_equipment_key(rotated_key).is_ok());

    let rotated = repository
        .save(&reader, SaveCondition::IfVersion(second_version))
        .await;

    assert!(rotated.is_ok());

    let loaded = match repository.find_by_id(reader_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("rotated reader was not found")
        }

        Err(error) => {
            panic!("rotated reader lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate().identity().key_id(), rotated_key);

    assert_eq!(loaded.version().value(), 3);
}
