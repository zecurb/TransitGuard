use std::env;

use transitguard_application::{FareCredentialRepository, SaveCondition, TransitAccountRepository};
use transitguard_domain::{
    Currency, EligibilityClassification, FareCredential, FareCredentialId, FareCredentialKind,
    Money, RiderId, TransitAccount, TransitAccountId,
};
use transitguard_persistence::{
    PostgresConfig, PostgresFareCredentialRepository, PostgresTransitAccountRepository,
    connect_postgres, run_postgres_migrations,
};

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn fare_credential_repository_enforces_versions_and_account_ownership() {
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

    let account_repository = PostgresTransitAccountRepository::new(pool.clone());

    let credential_repository = PostgresFareCredentialRepository::new(pool);

    let account_id = TransitAccountId::generate();

    let account = match TransitAccount::new(
        account_id,
        RiderId::generate(),
        EligibilityClassification::Standard,
        Money::from_minor_units(2_500, Currency::Usd),
    ) {
        Ok(account) => account,

        Err(error) => {
            panic!("account construction failed: {error}")
        }
    };

    let account_inserted = account_repository
        .save(&account, SaveCondition::MustNotExist)
        .await;

    assert!(account_inserted.is_ok());

    let credential_id = FareCredentialId::generate();

    let mut credential =
        FareCredential::new_pending(credential_id, account_id, FareCredentialKind::Card);

    let inserted = credential_repository
        .save(&credential, SaveCondition::MustNotExist)
        .await;

    assert!(inserted.is_ok());

    let duplicate = credential_repository
        .save(&credential, SaveCondition::MustNotExist)
        .await;

    assert!(duplicate.is_err());

    let loaded = match credential_repository.find_by_id(credential_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("inserted credential was not found")
        }

        Err(error) => {
            panic!("credential lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate(), &credential);
    assert_eq!(loaded.version().value(), 1);

    let first_version = loaded.version();

    assert!(credential.activate().is_ok());

    let updated = credential_repository
        .save(&credential, SaveCondition::IfVersion(first_version))
        .await;

    assert!(updated.is_ok());

    let loaded = match credential_repository.find_by_id(credential_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("updated credential was not found")
        }

        Err(error) => {
            panic!("updated credential lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate(), &credential);
    assert_eq!(loaded.version().value(), 2);

    let stale_update = credential_repository
        .save(&credential, SaveCondition::IfVersion(first_version))
        .await;

    assert!(stale_update.is_err());

    let missing_account_credential = FareCredential::new_pending(
        FareCredentialId::generate(),
        TransitAccountId::generate(),
        FareCredentialKind::Mobile,
    );

    let missing_account_insert = credential_repository
        .save(&missing_account_credential, SaveCondition::MustNotExist)
        .await;

    assert!(missing_account_insert.is_err());
}
