use std::env;

use transitguard_application::{SaveCondition, TransitAccountRepository};
use transitguard_domain::{
    Currency, EligibilityClassification, Money, RiderId, TransitAccount, TransitAccountId,
};
use transitguard_persistence::{
    PostgresConfig, PostgresTransitAccountRepository, connect_postgres, run_postgres_migrations,
};

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn transit_account_repository_enforces_versions() {
    let database_url = match env::var("DATABASE_URL") {
        Ok(database_url) => database_url,

        Err(error) => {
            panic!("DATABASE_URL is required: {error}")
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
        panic!("database migrations failed: {error}");
    }

    let repository = PostgresTransitAccountRepository::new(pool);

    let account_id = TransitAccountId::generate();

    let mut account = match TransitAccount::new(
        account_id,
        RiderId::generate(),
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

    let inserted = repository.save(&account, SaveCondition::MustNotExist).await;

    assert!(inserted.is_ok());

    let loaded = match repository.find_by_id(account_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("inserted account was not found")
        }

        Err(error) => {
            panic!("account lookup failed: {error}")
        }
    };

    assert_eq!(loaded.aggregate(), &account);
    assert_eq!(loaded.version().value(), 1);

    let first_version = loaded.version();

    assert!(account.suspend().is_ok());

    let updated = repository
        .save(&account, SaveCondition::IfVersion(first_version))
        .await;

    assert!(updated.is_ok());

    let loaded = match repository.find_by_id(account_id).await {
        Ok(Some(loaded)) => loaded,

        Ok(None) => {
            panic!("updated account was not found")
        }

        Err(error) => {
            panic!(
                "updated account lookup failed: \
                 {error}"
            )
        }
    };

    assert_eq!(loaded.aggregate(), &account);
    assert_eq!(loaded.version().value(), 2);

    let stale_update = repository
        .save(&account, SaveCondition::IfVersion(first_version))
        .await;

    assert!(stale_update.is_err());
}
