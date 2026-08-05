use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use sqlx::{PgPool, SqlitePool};
use tokio::{net::TcpListener, task::JoinHandle};
use transitguard_api::{ApiState, SynchronizationService, build_router};
use transitguard_device_protocol::{DeviceProtocolVersion, ProtocolEnvironmentId};
use transitguard_domain::{
    Currency, EquipmentKeyId, EventTime, FareApprovalReason, FareCredentialId, FareDecision,
    FarePolicyVersion, FareTransactionId, Money, ReaderId, SynchronizationBatchId,
};
use transitguard_persistence::{
    OfflineQueueState, OfflineTransactionDraft, PostgresConfig,
    PostgresSynchronizationIngestRepository, ReaderDatabaseIdentity, ReaderSqliteConfig,
    SynchronizationBatchState, bind_reader_database, connect_postgres, connect_reader_sqlite,
    create_synchronization_batch, enqueue_offline_transaction, load_offline_queue,
    load_synchronization_batch, mark_synchronization_batch_in_flight, run_postgres_migrations,
    run_reader_sqlite_migrations,
};
use transitguard_reader_simulator::{
    SynchronizationHttpClient, SynchronizationSubmissionResult,
    submit_in_flight_synchronization_batch,
};

const TEST_TIME: i64 = 1_700_000_000_000;

async fn postgres_pool() -> PgPool {
    let database_url = match env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(error) => {
            panic!("DATABASE_URL is required: {error}")
        }
    };

    let config = match PostgresConfig::new(database_url) {
        Ok(value) => value,
        Err(error) => {
            panic!("PostgreSQL configuration failed: {error}")
        }
    };

    let pool = match connect_postgres(&config).await {
        Ok(value) => value,
        Err(error) => {
            panic!("PostgreSQL connection failed: {error}")
        }
    };

    if let Err(error) = run_postgres_migrations(&pool).await {
        pool.close().await;

        panic!("PostgreSQL migrations failed: {error}");
    }

    pool
}

async fn register_reader(pool: &PgPool, reader_id: ReaderId) {
    let result = sqlx::query(
        r#"
        INSERT INTO reader_equipment (
            id,
            equipment_key_id,
            status,
            aggregate_version
        )
        VALUES ($1, $2, 'active', 1)
        "#,
    )
    .bind(reader_id.into_uuid())
    .bind(EquipmentKeyId::generate().into_uuid())
    .execute(pool)
    .await;

    if let Err(error) = result {
        panic!("reader registration failed: {error}");
    }
}

fn environment() -> ProtocolEnvironmentId {
    match ProtocolEnvironmentId::new("development") {
        Ok(value) => value,
        Err(error) => {
            panic!("environment creation failed: {error}")
        }
    }
}

async fn spawn_api(pool: &PgPool) -> (String, JoinHandle<()>) {
    let repository = PostgresSynchronizationIngestRepository::new(pool.clone());

    let service = SynchronizationService::new(repository.clone(), environment());

    let application = build_router(ApiState::new(repository, service));

    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(value) => value,
        Err(error) => {
            panic!("API listener creation failed: {error}")
        }
    };

    let address = match listener.local_addr() {
        Ok(value) => value,
        Err(error) => {
            panic!("API listener address failed: {error}")
        }
    };

    let server = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, application).await {
            panic!("TransitGuard API server failed: {error}");
        }
    });

    (format!("http://{address}"), server)
}

fn database_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "transitguard-round-trip-{name}-{}.sqlite3",
        SynchronizationBatchId::generate()
    ))
}

fn related_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);

    PathBuf::from(value)
}

fn remove_database(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(related_path(path, "-wal"));
    let _ = std::fs::remove_file(related_path(path, "-shm"));
}

fn reader_identity(reader_id: ReaderId) -> ReaderDatabaseIdentity {
    match ReaderDatabaseIdentity::new(
        reader_id,
        "development",
        "0.1.0",
        DeviceProtocolVersion::CURRENT,
        TEST_TIME,
    ) {
        Ok(value) => value,
        Err(error) => {
            panic!("reader identity creation failed: {error}")
        }
    }
}

async fn open_reader_database(reader_id: ReaderId) -> (PathBuf, SqlitePool) {
    let path = database_path("success");

    let config = match ReaderSqliteConfig::new(path.clone()) {
        Ok(value) => value,
        Err(error) => {
            panic!("SQLite configuration failed: {error}")
        }
    };

    let pool = match connect_reader_sqlite(&config).await {
        Ok(value) => value,
        Err(error) => {
            remove_database(&path);

            panic!("SQLite connection failed: {error}")
        }
    };

    if let Err(error) = run_reader_sqlite_migrations(&pool).await {
        pool.close().await;
        remove_database(&path);

        panic!("SQLite migrations failed: {error}");
    }

    if let Err(error) = bind_reader_database(&pool, &reader_identity(reader_id)).await {
        pool.close().await;
        remove_database(&path);

        panic!("reader identity binding failed: {error}");
    }

    (path, pool)
}

fn event_time() -> EventTime {
    match EventTime::from_unix_milliseconds(TEST_TIME) {
        Ok(value) => value,
        Err(error) => {
            panic!("event time creation failed: {error}")
        }
    }
}

fn policy_version() -> FarePolicyVersion {
    match FarePolicyVersion::new(1) {
        Ok(value) => value,
        Err(error) => {
            panic!("fare policy version failed: {error}")
        }
    }
}

fn provisional_decision() -> FareDecision {
    match FareDecision::approved(
        Money::from_minor_units(250, Currency::Usd),
        FareApprovalReason::OfflineProvisional,
    ) {
        Ok(value) => value,
        Err(error) => {
            panic!("fare decision creation failed: {error}")
        }
    }
}

fn offline_transaction() -> OfflineTransactionDraft {
    match OfflineTransactionDraft::new(
        FareTransactionId::generate(),
        FareCredentialId::generate(),
        event_time(),
        policy_version(),
        provisional_decision(),
        serde_json::json!({
            "schema_version": 1,
            "kind": "offline_fare_transaction"
        }),
        TEST_TIME + 100,
    ) {
        Ok(value) => value,
        Err(error) => {
            panic!("offline transaction creation failed: {error}")
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn reader_http_postgres_round_trip_is_durable() {
    let postgres = postgres_pool().await;

    let reader_id = ReaderId::generate();

    register_reader(&postgres, reader_id).await;

    let (reader_path, reader) = open_reader_database(reader_id).await;

    for _ in 0..2 {
        if let Err(error) =
            enqueue_offline_transaction(&reader, reader_id, &offline_transaction()).await
        {
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("offline queue insertion failed: {error}");
        }
    }

    let prepared = match create_synchronization_batch(
        &reader,
        reader_id,
        DeviceProtocolVersion::CURRENT,
        TEST_TIME + 200,
        2,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("batch creation failed: {error}")
        }
    };

    let submitted = match mark_synchronization_batch_in_flight(
        &reader,
        reader_id,
        prepared.batch_id(),
        TEST_TIME + 300,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("batch submission failed: {error}")
        }
    };

    let (base_url, server) = spawn_api(&postgres).await;

    let client = match SynchronizationHttpClient::new(&base_url, Duration::from_secs(5)) {
        Ok(value) => value,
        Err(error) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("HTTP client creation failed: {error}")
        }
    };

    let result = submit_in_flight_synchronization_batch(
        &reader,
        &client,
        reader_id,
        submitted.batch_id(),
        TEST_TIME + 400,
        TEST_TIME + 600,
    )
    .await;

    let application = match result {
        Ok(SynchronizationSubmissionResult::Applied {
            acknowledgement_replayed,
            application,
        }) => {
            assert!(!acknowledgement_replayed);
            application
        }

        Ok(other) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("unexpected synchronization result: {other:?}")
        }

        Err(error) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("round-trip submission failed: {error}")
        }
    };

    assert!(application.applied_now());
    assert_eq!(application.acknowledged_entries(), 2);
    assert_eq!(application.retryable_failure_entries(), 0);
    assert_eq!(application.permanent_failure_entries(), 0);
    assert_eq!(application.manual_review_entries(), 0);
    assert_eq!(application.last_acknowledged_sequence(), 2);

    let stored_batch =
        match load_synchronization_batch(&reader, reader_id, submitted.batch_id()).await {
            Ok(value) => value,
            Err(error) => {
                server.abort();
                reader.close().await;
                remove_database(&reader_path);
                postgres.close().await;

                panic!("stored batch reload failed: {error}")
            }
        };

    assert_eq!(
        stored_batch.state(),
        SynchronizationBatchState::Acknowledged
    );

    let queue = match load_offline_queue(&reader, reader_id).await {
        Ok(value) => value,
        Err(error) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("offline queue reload failed: {error}")
        }
    };

    assert_eq!(queue.len(), 2);

    assert!(
        queue
            .iter()
            .all(|entry| { entry.queue_state() == OfflineQueueState::Acknowledged })
    );

    let backend_batches = match sqlx::query_scalar::<_, i64>(
        r#"
            SELECT COUNT(*)
            FROM synchronization_ingest_batches
            WHERE batch_id = $1
            "#,
    )
    .bind(submitted.batch_id().into_uuid())
    .fetch_one(&postgres)
    .await
    {
        Ok(value) => value,
        Err(error) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("backend batch-count query failed: {error}")
        }
    };

    assert_eq!(backend_batches, 1);

    let backend_entries = match sqlx::query_scalar::<_, i64>(
        r#"
            SELECT COUNT(*)
            FROM synchronization_ingest_entries
            WHERE batch_id = $1
            "#,
    )
    .bind(submitted.batch_id().into_uuid())
    .fetch_one(&postgres)
    .await
    {
        Ok(value) => value,
        Err(error) => {
            server.abort();
            reader.close().await;
            remove_database(&reader_path);
            postgres.close().await;

            panic!("backend entry-count query failed: {error}")
        }
    };

    assert_eq!(backend_entries, 2);

    server.abort();
    let _ = server.await;

    reader.close().await;
    remove_database(&reader_path);

    postgres.close().await;
}
