pub mod synchronization_service;

pub use synchronization_service::{SynchronizationService, SynchronizationServiceError};

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use transitguard_device_protocol::{
    DeviceProtocolVersion, IDEMPOTENCY_KEY_HEADER, MAX_SYNCHRONIZATION_REQUEST_BYTES,
    PROTOCOL_VERSION_HEADER, SYNCHRONIZATION_BATCH_ENDPOINT, SynchronizationBatchRequest,
    SynchronizationFailureCategory,
};
use transitguard_persistence::PostgresSynchronizationIngestRepository;

/// Liveness endpoint for the TransitGuard API process.
pub const LIVENESS_PATH: &str = "/health/live";

/// Readiness endpoint for PostgreSQL-backed API traffic.
pub const READINESS_PATH: &str = "/health/ready";

/// Shared state available to API handlers.
#[derive(Clone, Debug)]
pub struct ApiState {
    synchronization_ingest_repository: PostgresSynchronizationIngestRepository,
    synchronization_service: SynchronizationService,
}

impl ApiState {
    /// Creates shared API state.
    #[must_use]
    pub const fn new(
        synchronization_ingest_repository: PostgresSynchronizationIngestRepository,
        synchronization_service: SynchronizationService,
    ) -> Self {
        Self {
            synchronization_ingest_repository,
            synchronization_service,
        }
    }

    /// Returns the synchronization-ingest repository.
    #[must_use]
    pub const fn synchronization_ingest_repository(
        &self,
    ) -> &PostgresSynchronizationIngestRepository {
        &self.synchronization_ingest_repository
    }

    /// Returns the synchronization application service.
    #[must_use]
    pub const fn synchronization_service(&self) -> &SynchronizationService {
        &self.synchronization_service
    }
}

/// Builds the TransitGuard API router.
pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route(LIVENESS_PATH, get(liveness))
        .route(READINESS_PATH, get(readiness))
        .route(
            SYNCHRONIZATION_BATCH_ENDPOINT,
            post(submit_synchronization_batch),
        )
        .layer(DefaultBodyLimit::max(MAX_SYNCHRONIZATION_REQUEST_BYTES))
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ApiErrorResponse {
    service: &'static str,
    error: &'static str,
    category: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransportValidationError {
    MissingIdempotencyKey,
    InvalidIdempotencyKey,
    MissingProtocolVersion,
    InvalidProtocolVersion,
    UnsupportedProtocolVersion,
}

impl TransportValidationError {
    const fn message(self) -> &'static str {
        match self {
            Self::MissingIdempotencyKey => "idempotency key header is required",

            Self::InvalidIdempotencyKey => "idempotency key does not match the batch",

            Self::MissingProtocolVersion => "protocol version header is required",

            Self::InvalidProtocolVersion => "protocol version header does not match the body",

            Self::UnsupportedProtocolVersion => "protocol version is unsupported",
        }
    }

    const fn failure_category(self) -> SynchronizationFailureCategory {
        match self {
            Self::UnsupportedProtocolVersion => SynchronizationFailureCategory::UnsupportedProtocol,

            Self::MissingIdempotencyKey
            | Self::InvalidIdempotencyKey
            | Self::MissingProtocolVersion
            | Self::InvalidProtocolVersion => {
                SynchronizationFailureCategory::BackendValidationFailure
            }
        }
    }

    fn into_response(self) -> Response {
        api_error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            self.failure_category().as_str(),
            self.message(),
        )
    }
}

async fn liveness() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse {
            service: "transitguard-api",
            status: "alive",
        }),
    )
}

async fn readiness(State(state): State<ApiState>) -> Response {
    let database_check = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(state.synchronization_ingest_repository().pool())
        .await;

    match database_check {
        Ok(1) => (
            StatusCode::OK,
            Json(HealthResponse {
                service: "transitguard-api",
                status: "ready",
            }),
        )
            .into_response(),

        Ok(_) | Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                service: "transitguard-api",
                status: "database_unavailable",
            }),
        )
            .into_response(),
    }
}

async fn submit_synchronization_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<SynchronizationBatchRequest>,
) -> Response {
    if let Err(error) = validate_transport_headers(&headers, &request) {
        return error.into_response();
    }

    let received_at_unix_milliseconds = match current_unix_milliseconds() {
        Some(timestamp) => timestamp,

        None => {
            return api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                SynchronizationFailureCategory::BackendTemporarilyUnavailable.as_str(),
                "backend time source is unavailable",
            );
        }
    };

    match state
        .synchronization_service()
        .process(&request, received_at_unix_milliseconds)
        .await
    {
        Ok(acknowledgement) => (StatusCode::OK, Json(acknowledgement)).into_response(),

        Err(error) => service_error_response(error),
    }
}

fn validate_transport_headers(
    headers: &HeaderMap,
    request: &SynchronizationBatchRequest,
) -> Result<(), TransportValidationError> {
    let idempotency_key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .ok_or(TransportValidationError::MissingIdempotencyKey)?
        .to_str()
        .map_err(|_| TransportValidationError::InvalidIdempotencyKey)?;

    if idempotency_key != request.batch_id().to_string() {
        return Err(TransportValidationError::InvalidIdempotencyKey);
    }

    let protocol_version = headers
        .get(PROTOCOL_VERSION_HEADER)
        .ok_or(TransportValidationError::MissingProtocolVersion)?
        .to_str()
        .map_err(|_| TransportValidationError::InvalidProtocolVersion)?
        .parse::<u16>()
        .map_err(|_| TransportValidationError::InvalidProtocolVersion)?;

    if protocol_version != request.protocol_version().value() {
        return Err(TransportValidationError::InvalidProtocolVersion);
    }

    if protocol_version != DeviceProtocolVersion::CURRENT.value() {
        return Err(TransportValidationError::UnsupportedProtocolVersion);
    }

    Ok(())
}

fn current_unix_milliseconds() -> Option<i64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;

    i64::try_from(elapsed.as_millis()).ok()
}

fn service_error_response(error: SynchronizationServiceError) -> Response {
    let category = error.failure_category().as_str();

    let (status, message) = match error {
        SynchronizationServiceError::BatchIdentityConflict => (
            StatusCode::CONFLICT,
            "synchronization batch identity conflicts with stored content",
        ),

        SynchronizationServiceError::TransactionIdentityConflict => (
            StatusCode::CONFLICT,
            "synchronization transaction identity conflicts with stored content",
        ),

        SynchronizationServiceError::BackendTemporarilyUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "backend synchronization dependency is unavailable",
        ),

        SynchronizationServiceError::UnsupportedProtocol => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "protocol version is unsupported",
        ),

        SynchronizationServiceError::EnvironmentMismatch => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "reader environment does not match the backend",
        ),

        SynchronizationServiceError::ReaderNotRegistered => {
            (StatusCode::UNPROCESSABLE_ENTITY, "reader is not registered")
        }

        SynchronizationServiceError::ReaderNotOperational => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "reader is not operational",
        ),

        SynchronizationServiceError::IngestRecord | SynchronizationServiceError::Protocol(_) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "synchronization request failed validation",
        ),
    };

    api_error_response(status, category, message)
}

fn api_error_response(status: StatusCode, category: &'static str, error: &'static str) -> Response {
    (
        status,
        Json(ApiErrorResponse {
            service: "transitguard-api",
            error,
            category,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    };
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;
    use transitguard_device_protocol::{
        CanonicalTransactionEnvelope, DeviceProtocolVersion, IDEMPOTENCY_KEY_HEADER,
        PROTOCOL_VERSION_HEADER, ProtocolEnvironmentId, ReaderSoftwareVersion,
        SYNCHRONIZATION_BATCH_ENDPOINT, SynchronizationBatchRequest,
        SynchronizationBatchRequestDefinition, SynchronizationRequestEntry,
    };
    use transitguard_domain::{
        FareTransactionId, LocalSequenceNumber, ReaderId, SynchronizationBatchId,
    };
    use transitguard_persistence::{
        PostgresReaderEquipmentRepository, PostgresSynchronizationIngestRepository,
    };

    use super::{
        ApiState, LIVENESS_PATH, SynchronizationService, TransportValidationError, build_router,
        validate_transport_headers,
    };

    fn test_state() -> ApiState {
        let pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy(concat!(
                "postgresql://transitguard:",
                "transitguard@127.0.0.1:1/",
                "transitguard"
            )) {
            Ok(pool) => pool,

            Err(error) => {
                panic!(
                    "test database configuration failed: \
                     {error}"
                )
            }
        };

        let ingest_repository = PostgresSynchronizationIngestRepository::new(pool.clone());

        let reader_repository = PostgresReaderEquipmentRepository::new(pool);

        let environment_id = match ProtocolEnvironmentId::new("development") {
            Ok(environment_id) => environment_id,

            Err(error) => {
                panic!("test environment failed: {error}")
            }
        };

        let service = SynchronizationService::new(
            reader_repository,
            ingest_repository.clone(),
            environment_id,
        );

        ApiState::new(ingest_repository, service)
    }

    fn sequence(value: u64) -> LocalSequenceNumber {
        match LocalSequenceNumber::new(value) {
            Ok(sequence) => sequence,

            Err(error) => {
                panic!("valid sequence failed: {error}")
            }
        }
    }

    fn protocol_request() -> SynchronizationBatchRequest {
        let environment_id = match ProtocolEnvironmentId::new("development") {
            Ok(environment_id) => environment_id,

            Err(error) => {
                panic!("valid environment failed: {error}")
            }
        };

        let software_version = match ReaderSoftwareVersion::new("0.1.0") {
            Ok(version) => version,

            Err(error) => {
                panic!(
                    "valid software version failed: \
                         {error}"
                )
            }
        };

        let envelope = match CanonicalTransactionEnvelope::from_json(r#"{"schema_version":1}"#) {
            Ok(envelope) => envelope,

            Err(error) => {
                panic!("valid envelope failed: {error}")
            }
        };

        let local_sequence_number = sequence(1);

        let entry = SynchronizationRequestEntry::new(
            FareTransactionId::generate(),
            local_sequence_number,
            envelope,
        );

        match SynchronizationBatchRequest::new(SynchronizationBatchRequestDefinition {
            protocol_version: DeviceProtocolVersion::CURRENT,
            environment_id,
            reader_id: ReaderId::generate(),
            reader_software_version: software_version,
            batch_id: SynchronizationBatchId::generate(),
            first_local_sequence_number: local_sequence_number,
            last_local_sequence_number: local_sequence_number,
            submitted_at_unix_milliseconds: 100,
            entries: vec![entry],
        }) {
            Ok(request) => request,

            Err(error) => {
                panic!("valid request failed: {error}")
            }
        }
    }

    fn idempotency_header(request: &SynchronizationBatchRequest) -> HeaderValue {
        let value = request.batch_id().to_string();

        match HeaderValue::from_bytes(value.as_bytes()) {
            Ok(header) => header,

            Err(error) => {
                panic!(
                    "valid idempotency header failed: \
                     {error}"
                )
            }
        }
    }

    #[tokio::test]
    async fn liveness_returns_stable_json() {
        let mut request = Request::new(Body::empty());

        *request.uri_mut() = Uri::from_static(LIVENESS_PATH);

        let response = match build_router(test_state()).oneshot(request).await {
            Ok(response) => response,

            Err(error) => {
                panic!("liveness request failed: {error}")
            }
        };

        assert_eq!(response.status(), StatusCode::OK);

        let body = match to_bytes(response.into_body(), 4_096).await {
            Ok(body) => body,

            Err(error) => {
                panic!("liveness body read failed: {error}")
            }
        };

        let payload = match serde_json::from_slice::<Value>(&body) {
            Ok(payload) => payload,

            Err(error) => {
                panic!(
                    "liveness JSON decode failed: \
                         {error}"
                )
            }
        };

        assert_eq!(
            payload.get("service"),
            Some(&Value::String(String::from("transitguard-api")))
        );

        assert_eq!(
            payload.get("status"),
            Some(&Value::String(String::from("alive")))
        );
    }

    #[tokio::test]
    async fn unknown_route_returns_not_found() {
        let mut request = Request::new(Body::empty());

        *request.uri_mut() = Uri::from_static("/does-not-exist");

        let response = match build_router(test_state()).oneshot(request).await {
            Ok(response) => response,

            Err(error) => {
                panic!(
                    "unknown-route request failed: \
                     {error}"
                )
            }
        };

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn synchronization_route_requires_headers() {
        let request_payload = protocol_request();

        let body = match serde_json::to_vec(&request_payload) {
            Ok(body) => body,

            Err(error) => {
                panic!("request serialization failed: {error}")
            }
        };

        let request = match Request::builder()
            .method(Method::POST)
            .uri(SYNCHRONIZATION_BATCH_ENDPOINT)
            .header("content-type", "application/json")
            .body(Body::from(body))
        {
            Ok(request) => request,

            Err(error) => {
                panic!("HTTP request creation failed: {error}")
            }
        };

        let response = match build_router(test_state()).oneshot(request).await {
            Ok(response) => response,

            Err(error) => {
                panic!(
                    "synchronization request failed: \
                     {error}"
                )
            }
        };

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn matching_transport_headers_are_accepted() {
        let request = protocol_request();
        let mut headers = HeaderMap::new();

        headers.insert(IDEMPOTENCY_KEY_HEADER, idempotency_header(&request));

        headers.insert(PROTOCOL_VERSION_HEADER, HeaderValue::from_static("1"));

        assert_eq!(validate_transport_headers(&headers, &request,), Ok(()));
    }

    #[test]
    fn missing_idempotency_header_is_rejected() {
        let request = protocol_request();
        let headers = HeaderMap::new();

        assert_eq!(
            validate_transport_headers(&headers, &request,),
            Err(TransportValidationError::MissingIdempotencyKey)
        );
    }

    #[test]
    fn conflicting_idempotency_header_is_rejected() {
        let request = protocol_request();
        let conflicting_request = protocol_request();
        let mut headers = HeaderMap::new();

        headers.insert(
            IDEMPOTENCY_KEY_HEADER,
            idempotency_header(&conflicting_request),
        );

        headers.insert(PROTOCOL_VERSION_HEADER, HeaderValue::from_static("1"));

        assert_eq!(
            validate_transport_headers(&headers, &request,),
            Err(TransportValidationError::InvalidIdempotencyKey)
        );
    }

    #[test]
    fn conflicting_protocol_header_is_rejected() {
        let request = protocol_request();
        let mut headers = HeaderMap::new();

        headers.insert(IDEMPOTENCY_KEY_HEADER, idempotency_header(&request));

        headers.insert(PROTOCOL_VERSION_HEADER, HeaderValue::from_static("2"));

        assert_eq!(
            validate_transport_headers(&headers, &request,),
            Err(TransportValidationError::InvalidProtocolVersion)
        );
    }
}
