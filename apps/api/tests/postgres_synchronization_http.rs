use std::env;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
    response::Response,
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use transitguard_api::{ApiState, SynchronizationService, build_router};
use transitguard_device_protocol::{
    CanonicalTransactionEnvelope, DeviceProtocolVersion, IDEMPOTENCY_KEY_HEADER,
    PROTOCOL_VERSION_HEADER, ProtocolEnvironmentId, ReaderSoftwareVersion,
    SYNCHRONIZATION_BATCH_ENDPOINT, SynchronizationBatchAcknowledgement,
    SynchronizationBatchRequest, SynchronizationBatchRequestDefinition,
    SynchronizationFailureCategory, SynchronizationRequestEntry,
};
use transitguard_domain::{
    EquipmentKeyId, FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
};
use transitguard_persistence::{
    PostgresConfig, PostgresSynchronizationIngestRepository, connect_postgres,
    run_postgres_migrations,
};

const SUBMITTED_AT: i64 = 1_700_000_000_000;
const RESPONSE_LIMIT: usize = 1_048_576;

async fn database_pool() -> PgPool {
    let database_url = match env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(error) => panic!("DATABASE_URL is required: {error}"),
    };

    let config = match PostgresConfig::new(database_url) {
        Ok(value) => value,
        Err(error) => panic!("database configuration failed: {error}"),
    };

    let pool = match connect_postgres(&config).await {
        Ok(value) => value,
        Err(error) => panic!("database connection failed: {error}"),
    };

    if let Err(error) = run_postgres_migrations(&pool).await {
        pool.close().await;
        panic!("database migrations failed: {error}");
    }

    pool
}

async fn register_reader(pool: &PgPool, reader_id: ReaderId, status: &str) {
    let result = sqlx::query(
        r#"
        INSERT INTO reader_equipment (
            id,
            equipment_key_id,
            status,
            aggregate_version
        )
        VALUES ($1, $2, $3, 1)
        "#,
    )
    .bind(reader_id.into_uuid())
    .bind(EquipmentKeyId::generate().into_uuid())
    .bind(status)
    .execute(pool)
    .await;

    if let Err(error) = result {
        panic!("reader registration failed: {error}");
    }
}

fn environment() -> ProtocolEnvironmentId {
    match ProtocolEnvironmentId::new("development") {
        Ok(value) => value,
        Err(error) => panic!("environment creation failed: {error}"),
    }
}

fn software_version() -> ReaderSoftwareVersion {
    match ReaderSoftwareVersion::new("0.1.0") {
        Ok(value) => value,
        Err(error) => panic!("software-version creation failed: {error}"),
    }
}

fn sequence(value: u64) -> LocalSequenceNumber {
    match LocalSequenceNumber::new(value) {
        Ok(value) => value,
        Err(error) => panic!("sequence creation failed: {error}"),
    }
}

fn synchronization_request(
    reader_id: ReaderId,
    batch_id: SynchronizationBatchId,
    transaction_id: FareTransactionId,
    marker: &str,
) -> SynchronizationBatchRequest {
    let envelope_json = format!(r#"{{"marker":"{marker}","schema_version":1}}"#);

    let envelope = match CanonicalTransactionEnvelope::from_json(&envelope_json) {
        Ok(value) => value,
        Err(error) => panic!("transaction envelope failed: {error}"),
    };

    let local_sequence_number = sequence(1);

    let entry = SynchronizationRequestEntry::new(transaction_id, local_sequence_number, envelope);

    match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
        protocol_version: DeviceProtocolVersion::CURRENT,
        environment_id: environment(),
        reader_id,
        reader_software_version: software_version(),
        batch_id,
        first_local_sequence_number: local_sequence_number,
        last_local_sequence_number: local_sequence_number,
        submitted_at_unix_milliseconds: SUBMITTED_AT,
        entries: vec![entry],
    }) {
        Ok(value) => value,
        Err(error) => panic!("synchronization request failed: {error}"),
    }
}

fn api_state(pool: &PgPool) -> ApiState {
    let repository = PostgresSynchronizationIngestRepository::new(pool.clone());

    let service = SynchronizationService::new(repository.clone(), environment());

    ApiState::new(repository, service)
}

fn http_request(payload: &SynchronizationBatchRequest) -> Request<Body> {
    let body = match serde_json::to_vec(payload) {
        Ok(value) => value,
        Err(error) => panic!("request serialization failed: {error}"),
    };

    match Request::builder()
        .method(Method::POST)
        .uri(SYNCHRONIZATION_BATCH_ENDPOINT)
        .header("content-type", "application/json")
        .header(IDEMPOTENCY_KEY_HEADER, payload.batch_id().to_string())
        .header(
            PROTOCOL_VERSION_HEADER,
            payload.protocol_version().value().to_string(),
        )
        .body(Body::from(body))
    {
        Ok(value) => value,
        Err(error) => panic!("HTTP request creation failed: {error}"),
    }
}

async fn send(application: &Router, payload: &SynchronizationBatchRequest) -> Response {
    match application.clone().oneshot(http_request(payload)).await {
        Ok(value) => value,
        Err(error) => panic!("HTTP request failed: {error}"),
    }
}

async fn decode_acknowledgement(response: Response) -> SynchronizationBatchAcknowledgement {
    let body = match to_bytes(response.into_body(), RESPONSE_LIMIT).await {
        Ok(value) => value,
        Err(error) => panic!("response body read failed: {error}"),
    };

    match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => panic!("acknowledgement decode failed: {error}"),
    }
}

async fn decode_json(response: Response) -> Value {
    let body = match to_bytes(response.into_body(), RESPONSE_LIMIT).await {
        Ok(value) => value,
        Err(error) => panic!("response body read failed: {error}"),
    };

    match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => panic!("response JSON decode failed: {error}"),
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL database"]
async fn synchronization_endpoint_is_atomic_and_idempotent() {
    let pool = database_pool().await;

    let active_reader_id = ReaderId::generate();

    register_reader(&pool, active_reader_id, "active").await;

    let application = build_router(api_state(&pool));

    let batch_id = SynchronizationBatchId::generate();
    let transaction_id = FareTransactionId::generate();

    let initial_request =
        synchronization_request(active_reader_id, batch_id, transaction_id, "initial");

    let initial_response = send(&application, &initial_request).await;

    assert_eq!(initial_response.status(), StatusCode::OK);

    let initial_acknowledgement = decode_acknowledgement(initial_response).await;

    assert!(!initial_acknowledgement.replayed());

    if let Err(error) = initial_acknowledgement.validate_against_request(&initial_request) {
        pool.close().await;
        panic!("initial acknowledgement was invalid: {error}");
    }

    let replay_response = send(&application, &initial_request).await;

    assert_eq!(replay_response.status(), StatusCode::OK);

    let replay_acknowledgement = decode_acknowledgement(replay_response).await;

    assert!(replay_acknowledgement.replayed());

    assert_eq!(
        replay_acknowledgement.received_at_unix_milliseconds(),
        initial_acknowledgement.received_at_unix_milliseconds()
    );

    let conflicting_request = synchronization_request(
        active_reader_id,
        batch_id,
        transaction_id,
        "conflicting-content",
    );

    let conflict_response = send(&application, &conflicting_request).await;

    assert_eq!(conflict_response.status(), StatusCode::CONFLICT);

    let conflict_payload = decode_json(conflict_response).await;

    assert_eq!(
        conflict_payload.get("category").and_then(Value::as_str),
        Some(SynchronizationFailureCategory::BatchIdentityConflict.as_str())
    );

    let unregistered_request = synchronization_request(
        ReaderId::generate(),
        SynchronizationBatchId::generate(),
        FareTransactionId::generate(),
        "unregistered-reader",
    );

    let unregistered_response = send(&application, &unregistered_request).await;

    assert_eq!(
        unregistered_response.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let unregistered_payload = decode_json(unregistered_response).await;

    assert_eq!(
        unregistered_payload.get("category").and_then(Value::as_str),
        Some(SynchronizationFailureCategory::ReaderNotRegistered.as_str())
    );

    let pending_reader_id = ReaderId::generate();

    register_reader(&pool, pending_reader_id, "pending_registration").await;

    let pending_request = synchronization_request(
        pending_reader_id,
        SynchronizationBatchId::generate(),
        FareTransactionId::generate(),
        "pending-reader",
    );

    let pending_response = send(&application, &pending_request).await;

    assert_eq!(pending_response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let pending_payload = decode_json(pending_response).await;

    assert_eq!(
        pending_payload.get("category").and_then(Value::as_str),
        Some(SynchronizationFailureCategory::ReaderNotOperational.as_str())
    );

    let stored_batches =
        match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM synchronization_ingest_batches")
            .fetch_one(&pool)
            .await
        {
            Ok(value) => value,

            Err(error) => {
                pool.close().await;
                panic!("batch-count query failed: {error}")
            }
        };

    assert_eq!(stored_batches, 1);

    pool.close().await;
}
