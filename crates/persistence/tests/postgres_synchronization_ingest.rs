use std::env;

use sqlx::PgPool;
use transitguard_application::{ReaderEquipmentRepository, SaveCondition};
use transitguard_device_protocol::{
    CanonicalTransactionEnvelope, DeviceProtocolVersion, ProtocolEnvironmentId,
    ReaderSoftwareVersion, SynchronizationAcknowledgementEntry,
    SynchronizationBatchAcknowledgement, SynchronizationBatchAcknowledgementDefinition,
    SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
    SynchronizationEntryOutcome, SynchronizationFailureCategory, SynchronizationRequestEntry,
};
use transitguard_domain::{
    EquipmentKeyId, FareTransactionId, LocalSequenceNumber, ReaderEquipment, ReaderId,
    SynchronizationBatchId,
};
use transitguard_persistence::{
    PostgresConfig, PostgresReaderEquipmentRepository, PostgresSynchronizationIngestRepository,
    PreparedSynchronizationIngest, SynchronizationIngestDisposition,
    SynchronizationIngestPersistenceError, connect_postgres, run_postgres_migrations,
};

const SUBMITTED_AT: i64 = 1_700_000_000_000;
const RECEIVED_AT: i64 = 1_700_000_000_500;

async fn database_pool() -> PgPool {
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
        pool.close().await;
        panic!("database migrations failed: {error}");
    }

    pool
}

async fn register_reader(pool: &PgPool, reader_id: ReaderId) {
    let repository = PostgresReaderEquipmentRepository::new(pool.clone());

    let reader = ReaderEquipment::new_pending(reader_id, EquipmentKeyId::generate());

    let result = repository.save(&reader, SaveCondition::MustNotExist).await;

    if let Err(error) = result {
        panic!("reader registration failed: {error}");
    }
}

fn sequence(value: u64) -> LocalSequenceNumber {
    match LocalSequenceNumber::new(value) {
        Ok(sequence) => sequence,

        Err(error) => {
            panic!("valid sequence failed: {error}")
        }
    }
}

fn environment() -> ProtocolEnvironmentId {
    match ProtocolEnvironmentId::new("development") {
        Ok(environment) => environment,

        Err(error) => {
            panic!("valid environment failed: {error}")
        }
    }
}

fn software_version() -> ReaderSoftwareVersion {
    match ReaderSoftwareVersion::new("0.1.0") {
        Ok(version) => version,

        Err(error) => {
            panic!("valid software version failed: {error}")
        }
    }
}

fn envelope(local_sequence_number: u64, marker: &str) -> CanonicalTransactionEnvelope {
    let json = format!(
        concat!(
            "{{",
            "\"schema_version\":1,",
            "\"local_sequence_number\":{},",
            "\"marker\":\"{}\"",
            "}}"
        ),
        local_sequence_number, marker,
    );

    match CanonicalTransactionEnvelope::from_json(&json) {
        Ok(envelope) => envelope,

        Err(error) => {
            panic!("valid envelope failed: {error}")
        }
    }
}

fn request(
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    transaction_ids: [FareTransactionId; 2],
    sequences: [u64; 2],
    marker: &str,
) -> SynchronizationBatchRequest {
    let entries = vec![
        SynchronizationRequestEntry::new(
            transaction_ids[0],
            sequence(sequences[0]),
            envelope(sequences[0], marker),
        ),
        SynchronizationRequestEntry::new(
            transaction_ids[1],
            sequence(sequences[1]),
            envelope(sequences[1], marker),
        ),
    ];

    match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
        protocol_version: DeviceProtocolVersion::CURRENT,
        environment_id: environment(),
        reader_id,
        reader_software_version: software_version(),
        batch_id,
        first_local_sequence_number: sequence(sequences[0]),
        last_local_sequence_number: sequence(sequences[1]),
        submitted_at_unix_milliseconds: SUBMITTED_AT,
        entries,
    }) {
        Ok(request) => request,

        Err(error) => {
            panic!("valid request failed: {error}")
        }
    }
}

fn acknowledgement(
    request: &SynchronizationBatchRequest,
    received_at_unix_milliseconds: i64,
) -> SynchronizationBatchAcknowledgement {
    let accepted = match SynchronizationAcknowledgementEntry::new(
        request.entries()[0].transaction_id(),
        request.entries()[0].local_sequence_number(),
        SynchronizationEntryOutcome::Acknowledged,
        None,
        None,
    ) {
        Ok(entry) => entry,

        Err(error) => {
            panic!("valid acknowledgement entry failed: {error}")
        }
    };

    let retryable = match SynchronizationAcknowledgementEntry::new(
        request.entries()[1].transaction_id(),
        request.entries()[1].local_sequence_number(),
        SynchronizationEntryOutcome::RetryableFailure,
        Some(SynchronizationFailureCategory::BackendTemporarilyUnavailable),
        Some(received_at_unix_milliseconds + 1_000),
    ) {
        Ok(entry) => entry,

        Err(error) => {
            panic!("valid acknowledgement entry failed: {error}")
        }
    };

    match SynchronizationBatchAcknowledgement::new(SynchronizationBatchAcknowledgementDefinition {
        protocol_version: request.protocol_version(),
        environment_id: request.environment_id().clone(),
        reader_id: request.reader_id(),
        batch_id: request.batch_id(),
        first_local_sequence_number: request.first_local_sequence_number(),
        last_local_sequence_number: request.last_local_sequence_number(),
        received_at_unix_milliseconds,
        replayed: false,
        entries: vec![accepted, retryable],
    }) {
        Ok(acknowledgement) => acknowledgement,

        Err(error) => {
            panic!("valid acknowledgement failed: {error}")
        }
    }
}

fn prepare(
    request: &SynchronizationBatchRequest,
    received_at_unix_milliseconds: i64,
) -> PreparedSynchronizationIngest {
    let acknowledgement = acknowledgement(request, received_at_unix_milliseconds);

    match PreparedSynchronizationIngest::prepare(request, &acknowledgement) {
        Ok(prepared) => prepared,

        Err(error) => {
            panic!(
                "synchronization ingest preparation failed: \
                 {error}"
            )
        }
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn synchronization_ingest_is_atomic_and_idempotent() {
    let pool = database_pool().await;

    let reader_id = ReaderId::generate();
    register_reader(&pool, reader_id).await;

    let batch_id = SynchronizationBatchId::generate();

    let transaction_ids = [FareTransactionId::generate(), FareTransactionId::generate()];

    let initial_request = request(reader_id, batch_id, transaction_ids, [10, 12], "initial");

    let initial_ingest = prepare(&initial_request, RECEIVED_AT);

    let repository = PostgresSynchronizationIngestRepository::new(pool.clone());

    let first = repository.store(&initial_ingest).await;

    assert!(matches!(
        first,
        Ok(SynchronizationIngestDisposition::Stored)
    ));

    let replay_ingest = prepare(&initial_request, RECEIVED_AT + 10_000);

    let replay = repository.store(&replay_ingest).await;

    assert!(matches!(
        replay,
        Ok(SynchronizationIngestDisposition::Replayed)
    ));

    let stored_acknowledgement = match repository.load_acknowledgement(batch_id).await {
        Ok(Some(acknowledgement)) => acknowledgement,

        Ok(None) => {
            pool.close().await;
            panic!("stored acknowledgement was not found")
        }

        Err(error) => {
            pool.close().await;
            panic!("stored acknowledgement load failed: {error}")
        }
    };

    assert_eq!(
        stored_acknowledgement.received_at_unix_milliseconds(),
        RECEIVED_AT
    );

    assert!(!stored_acknowledgement.replayed());

    let replay_response = stored_acknowledgement.with_replayed(true);

    assert!(replay_response.replayed());

    assert_eq!(replay_response.received_at_unix_milliseconds(), RECEIVED_AT);

    let batch_count = match sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM synchronization_ingest_batches
        WHERE batch_id = $1
        "#,
    )
    .bind(batch_id.into_uuid())
    .fetch_one(&pool)
    .await
    {
        Ok(count) => count,

        Err(error) => {
            pool.close().await;
            panic!("batch count failed: {error}")
        }
    };

    let transaction_count = match sqlx::query_scalar::<_, i64>(
        r#"
            SELECT COUNT(*)
            FROM synchronization_ingest_transactions
            WHERE
                reader_id = $1
                AND local_sequence_number IN (10, 12)
            "#,
    )
    .bind(reader_id.into_uuid())
    .fetch_one(&pool)
    .await
    {
        Ok(count) => count,

        Err(error) => {
            pool.close().await;
            panic!("transaction count failed: {error}")
        }
    };

    let entry_count = match sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM synchronization_ingest_entries
        WHERE batch_id = $1
        "#,
    )
    .bind(batch_id.into_uuid())
    .fetch_one(&pool)
    .await
    {
        Ok(count) => count,

        Err(error) => {
            pool.close().await;
            panic!("entry count failed: {error}")
        }
    };

    assert_eq!(batch_count, 1);
    assert_eq!(transaction_count, 2);
    assert_eq!(entry_count, 2);

    let conflicting_batch_request = request(
        reader_id,
        batch_id,
        [FareTransactionId::generate(), FareTransactionId::generate()],
        [10, 12],
        "conflicting-batch",
    );

    let conflicting_batch = prepare(&conflicting_batch_request, RECEIVED_AT);

    let batch_conflict = repository.store(&conflicting_batch).await;

    assert!(matches!(
        batch_conflict,
        Err(
            SynchronizationIngestPersistenceError::
                BatchIdentityConflict {
                    batch_id: conflicting_id,
                }
        ) if conflicting_id == batch_id
    ));

    let conflict_batch_id = SynchronizationBatchId::generate();

    let transaction_conflict_request = request(
        reader_id,
        conflict_batch_id,
        [transaction_ids[0], FareTransactionId::generate()],
        [20, 22],
        "conflicting-transaction",
    );

    let transaction_conflict = prepare(&transaction_conflict_request, RECEIVED_AT);

    let transaction_result = repository.store(&transaction_conflict).await;

    assert!(matches!(
        transaction_result,
        Err(
            SynchronizationIngestPersistenceError::
                TransactionIdentityConflict {
                    transaction_id,
                }
        ) if transaction_id == transaction_ids[0]
    ));

    let rolled_back_batch_count = match sqlx::query_scalar::<_, i64>(
        r#"
            SELECT COUNT(*)
            FROM synchronization_ingest_batches
            WHERE batch_id = $1
            "#,
    )
    .bind(conflict_batch_id.into_uuid())
    .fetch_one(&pool)
    .await
    {
        Ok(count) => count,

        Err(error) => {
            pool.close().await;
            panic!("rollback verification failed: {error}")
        }
    };

    assert_eq!(rolled_back_batch_count, 0);

    pool.close().await;
}
