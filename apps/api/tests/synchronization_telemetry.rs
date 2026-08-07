use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use transitguard_api::{
    ApiState, SYNCHRONIZATION_HEALTH_PATH, SynchronizationService, build_router,
};
use transitguard_device_protocol::{ProtocolEnvironmentId, SYNCHRONIZATION_BATCH_ENDPOINT};
use transitguard_persistence::PostgresSynchronizationIngestRepository;

const RESPONSE_LIMIT: usize = 65_536;

fn test_state() -> ApiState {
    let pool = match PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy(concat!(
            "postgresql://transitguard:",
            "transitguard@127.0.0.1:1/",
            "transitguard"
        )) {
        Ok(value) => value,
        Err(error) => {
            panic!("test database configuration failed: {error}")
        }
    };

    let repository = PostgresSynchronizationIngestRepository::new(pool);

    let environment = match ProtocolEnvironmentId::new("development") {
        Ok(value) => value,
        Err(error) => {
            panic!("test environment failed: {error}")
        }
    };

    let service = SynchronizationService::new(repository.clone(), environment);

    ApiState::new(repository, service)
}

async fn read_json(response: axum::response::Response) -> Value {
    let body = match to_bytes(response.into_body(), RESPONSE_LIMIT).await {
        Ok(value) => value,
        Err(error) => {
            panic!("response body read failed: {error}")
        }
    };

    match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            panic!("response JSON decode failed: {error}")
        }
    }
}

async fn health(application: &axum::Router) -> Value {
    let request = match Request::builder()
        .method(Method::GET)
        .uri(SYNCHRONIZATION_HEALTH_PATH)
        .body(Body::empty())
    {
        Ok(value) => value,
        Err(error) => {
            panic!("health request creation failed: {error}")
        }
    };

    let response = match application.clone().oneshot(request).await {
        Ok(value) => value,
        Err(error) => {
            panic!("health request failed: {error}")
        }
    };

    assert_eq!(response.status(), StatusCode::OK);

    read_json(response).await
}

#[tokio::test]
async fn synchronization_health_reports_sanitized_failures() {
    let application = build_router(test_state());

    let initial = health(&application).await;

    assert_eq!(
        initial.get("requests_total").and_then(Value::as_u64),
        Some(0)
    );

    assert_eq!(
        initial
            .get("request_failures_total")
            .and_then(Value::as_u64),
        Some(0)
    );

    let malformed = match Request::builder()
        .method(Method::POST)
        .uri(SYNCHRONIZATION_BATCH_ENDPOINT)
        .header("content-type", "application/json")
        .body(Body::from("{"))
    {
        Ok(value) => value,
        Err(error) => {
            panic!("malformed request creation failed: {error}")
        }
    };

    let response = match application.clone().oneshot(malformed).await {
        Ok(value) => value,
        Err(error) => {
            panic!("malformed request failed: {error}")
        }
    };

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let snapshot = health(&application).await;

    assert_eq!(
        snapshot.get("requests_total").and_then(Value::as_u64),
        Some(1)
    );

    assert_eq!(
        snapshot
            .get("request_failures_total")
            .and_then(Value::as_u64),
        Some(1)
    );

    assert_eq!(
        snapshot
            .get("failures")
            .and_then(|value| { value.get("backend_validation_failure") })
            .and_then(Value::as_u64),
        Some(1)
    );

    assert!(!snapshot.to_string().contains("request body"));
}
