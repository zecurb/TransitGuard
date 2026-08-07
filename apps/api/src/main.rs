use std::{env, error::Error};

use transitguard_api::{ApiState, SynchronizationService, build_router};
use transitguard_device_protocol::ProtocolEnvironmentId;
use transitguard_persistence::{
    PostgresConfig, PostgresSynchronizationIngestRepository, connect_postgres,
    run_postgres_migrations,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = env::var("DATABASE_URL")?;

    let config = PostgresConfig::new(database_url)?;

    let pool = connect_postgres(&config).await?;

    run_postgres_migrations(&pool).await?;

    let ingest_repository = PostgresSynchronizationIngestRepository::new(pool.clone());

    let environment =
        env::var("TRANSITGUARD_ENVIRONMENT").unwrap_or_else(|_| String::from("development"));

    let environment_id = ProtocolEnvironmentId::new(environment)?;

    let synchronization_service =
        SynchronizationService::new(ingest_repository.clone(), environment_id);

    let state = ApiState::new(ingest_repository, synchronization_service);

    let bind_address =
        env::var("TRANSITGUARD_API_BIND").unwrap_or_else(|_| String::from("127.0.0.1:8080"));

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;

    println!("TransitGuard API listening on {bind_address}");

    axum::serve(listener, build_router(state)).await?;

    Ok(())
}
