pub mod synchronization_service;

pub use synchronization_service::{SynchronizationService, SynchronizationServiceError};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use transitguard_persistence::PostgresSynchronizationIngestRepository;

/// Liveness endpoint for the TransitGuard API process.
pub const LIVENESS_PATH: &str = "/health/live";

/// Readiness endpoint for PostgreSQL-backed API traffic.
pub const READINESS_PATH: &str = "/health/ready";

/// Shared state available to API handlers.
#[derive(Clone, Debug)]
pub struct ApiState {
    synchronization_ingest_repository: PostgresSynchronizationIngestRepository,
}

impl ApiState {
    /// Creates API state from the synchronization repository.
    #[must_use]
    pub const fn new(
        synchronization_ingest_repository: PostgresSynchronizationIngestRepository,
    ) -> Self {
        Self {
            synchronization_ingest_repository,
        }
    }

    /// Returns the synchronization-ingest repository.
    #[must_use]
    pub const fn synchronization_ingest_repository(
        &self,
    ) -> &PostgresSynchronizationIngestRepository {
        &self.synchronization_ingest_repository
    }
}

/// Builds the TransitGuard API router.
pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route(LIVENESS_PATH, get(liveness))
        .route(READINESS_PATH, get(readiness))
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
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

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, Uri},
    };
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;
    use transitguard_persistence::PostgresSynchronizationIngestRepository;

    use super::{ApiState, LIVENESS_PATH, build_router};

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

        ApiState::new(PostgresSynchronizationIngestRepository::new(pool))
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
                panic!("unknown-route request failed: {error}")
            }
        };

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
