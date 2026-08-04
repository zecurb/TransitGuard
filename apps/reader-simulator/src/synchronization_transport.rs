//! Reader-side HTTP transport for durable synchronization batches.
//!
//! The transport submits only project-owned TransitGuard protocol messages.
//! It does not contain real transit credentials, proprietary device
//! protocols, or authentication material.

use std::time::Duration;

use reqwest::{
    Client, StatusCode, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap},
};
use serde::Deserialize;
use thiserror::Error;
use transitguard_device_protocol::{
    IDEMPOTENCY_KEY_HEADER, MAX_SYNCHRONIZATION_ACKNOWLEDGEMENT_BYTES,
    MAX_SYNCHRONIZATION_REQUEST_BYTES, PROTOCOL_VERSION_HEADER, SYNCHRONIZATION_BATCH_ENDPOINT,
    SynchronizationBatchAcknowledgement, SynchronizationBatchRequest,
    SynchronizationFailureCategory, SynchronizationProtocolError,
};

const JSON_MEDIA_TYPE: &str = "application/json";
const TRANSITGUARD_API_SERVICE: &str = "transitguard-api";

/// Reader-side client for the versioned synchronization endpoint.
#[derive(Clone, Debug)]
pub struct SynchronizationHttpClient {
    client: Client,
    endpoint: Url,
    maximum_acknowledgement_bytes: usize,
}

/// Failures produced by the reader synchronization transport.
#[derive(Debug, Error)]
pub enum SynchronizationHttpClientError {
    /// The configured backend URL was not an acceptable HTTP URL.
    #[error("synchronization backend URL is invalid")]
    InvalidEndpoint,

    /// A positive request timeout is required.
    #[error("synchronization request timeout must be positive")]
    InvalidTimeout,

    /// The underlying HTTP client could not be configured.
    #[error("synchronization HTTP client configuration failed")]
    ClientConfiguration {
        /// Original HTTP-client failure.
        #[source]
        source: reqwest::Error,
    },

    /// The validated request could not be encoded.
    #[error("synchronization request encoding failed")]
    RequestEncoding {
        /// Original JSON encoding failure.
        #[source]
        source: serde_json::Error,
    },

    /// The encoded request exceeded the protocol limit.
    #[error(
        "synchronization request exceeds {maximum_bytes} bytes: \
         {actual_bytes}"
    )]
    RequestTooLarge {
        /// Protocol limit.
        maximum_bytes: usize,

        /// Encoded request size.
        actual_bytes: usize,
    },

    /// The HTTP request failed before a response was available.
    #[error(
        "synchronization HTTP request failed with category \
         {category:?}"
    )]
    Request {
        /// Stable operational category.
        category: SynchronizationFailureCategory,

        /// Original HTTP-client failure.
        #[source]
        source: reqwest::Error,
    },

    /// The backend returned a non-success status.
    #[error(
        "synchronization backend returned HTTP {status} with \
         category {category:?}"
    )]
    HttpStatus {
        /// Numeric HTTP status.
        status: u16,

        /// Stable protocol failure category.
        category: SynchronizationFailureCategory,
    },

    /// A successful response did not declare JSON.
    #[error(
        "synchronization acknowledgement did not use \
         application/json"
    )]
    UnexpectedContentType,

    /// Reading the response body failed.
    #[error(
        "synchronization response read failed with category \
         {category:?}"
    )]
    ResponseRead {
        /// Stable operational category.
        category: SynchronizationFailureCategory,

        /// Original HTTP-client failure.
        #[source]
        source: reqwest::Error,
    },

    /// The response exceeded the acknowledgement limit.
    #[error(
        "synchronization response exceeds {maximum_bytes} bytes: \
         {actual_bytes}"
    )]
    ResponseTooLarge {
        /// Protocol limit.
        maximum_bytes: usize,

        /// Observed response size.
        actual_bytes: usize,
    },

    /// The successful response was not a valid acknowledgement.
    #[error("synchronization acknowledgement JSON is invalid")]
    ResponseDecode {
        /// Original JSON decoding failure.
        #[source]
        source: serde_json::Error,
    },

    /// The acknowledgement did not correspond to the request.
    #[error(
        "synchronization acknowledgement does not match \
         the submitted request"
    )]
    InvalidAcknowledgement {
        /// Protocol validation failure.
        #[source]
        source: SynchronizationProtocolError,
    },
}

impl SynchronizationHttpClientError {
    /// Returns a bounded operational failure category.
    #[must_use]
    pub const fn failure_category(&self) -> SynchronizationFailureCategory {
        match self {
            Self::InvalidEndpoint | Self::InvalidTimeout | Self::RequestEncoding { .. } => {
                SynchronizationFailureCategory::BackendValidationFailure
            }

            Self::ClientConfiguration { .. } => SynchronizationFailureCategory::ConnectionFailure,

            Self::RequestTooLarge { .. } | Self::ResponseTooLarge { .. } => {
                SynchronizationFailureCategory::PayloadTooLarge
            }

            Self::Request { category, .. }
            | Self::ResponseRead { category, .. }
            | Self::HttpStatus { category, .. } => *category,

            Self::UnexpectedContentType
            | Self::ResponseDecode { .. }
            | Self::InvalidAcknowledgement { .. } => {
                SynchronizationFailureCategory::ResponseDecodeFailure
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    service: String,
    category: SynchronizationFailureCategory,
}

impl SynchronizationHttpClient {
    /// Builds a client for a TransitGuard backend base URL.
    ///
    /// The supplied URL must use HTTP or HTTPS and must not embed
    /// credentials. An absolute synchronization path is appended.
    pub fn new(
        base_url: &str,
        request_timeout: Duration,
    ) -> Result<Self, SynchronizationHttpClientError> {
        if request_timeout.is_zero() {
            return Err(SynchronizationHttpClientError::InvalidTimeout);
        }

        let base_url =
            Url::parse(base_url).map_err(|_| SynchronizationHttpClientError::InvalidEndpoint)?;

        let valid_scheme = matches!(base_url.scheme(), "http" | "https");

        let contains_credentials = !base_url.username().is_empty() || base_url.password().is_some();

        if !valid_scheme || base_url.host_str().is_none() || contains_credentials {
            return Err(SynchronizationHttpClientError::InvalidEndpoint);
        }

        let endpoint = base_url
            .join(SYNCHRONIZATION_BATCH_ENDPOINT)
            .map_err(|_| SynchronizationHttpClientError::InvalidEndpoint)?;

        let client = Client::builder()
            .timeout(request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| SynchronizationHttpClientError::ClientConfiguration { source })?;

        Ok(Self {
            client,
            endpoint,
            maximum_acknowledgement_bytes: MAX_SYNCHRONIZATION_ACKNOWLEDGEMENT_BYTES,
        })
    }

    /// Returns the resolved synchronization endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Submits one validated durable synchronization request.
    ///
    /// A successful acknowledgement is decoded and validated against
    /// the original request before being returned to the caller.
    pub async fn submit(
        &self,
        request: &SynchronizationBatchRequest,
    ) -> Result<SynchronizationBatchAcknowledgement, SynchronizationHttpClientError> {
        let body = serde_json::to_vec(request)
            .map_err(|source| SynchronizationHttpClientError::RequestEncoding { source })?;

        if body.len() > MAX_SYNCHRONIZATION_REQUEST_BYTES {
            return Err(SynchronizationHttpClientError::RequestTooLarge {
                maximum_bytes: MAX_SYNCHRONIZATION_REQUEST_BYTES,
                actual_bytes: body.len(),
            });
        }

        let response = self
            .client
            .post(self.endpoint.clone())
            .header(ACCEPT, JSON_MEDIA_TYPE)
            .header(CONTENT_TYPE, JSON_MEDIA_TYPE)
            .header(IDEMPOTENCY_KEY_HEADER, request.batch_id().to_string())
            .header(
                PROTOCOL_VERSION_HEADER,
                request.protocol_version().value().to_string(),
            )
            .body(body)
            .send()
            .await
            .map_err(|source| SynchronizationHttpClientError::Request {
                category: request_failure_category(&source),
                source,
            })?;

        let status = response.status();

        let response_is_json = has_json_content_type(response.headers());

        let response_body = read_bounded_body(response, self.maximum_acknowledgement_bytes).await?;

        if status != StatusCode::OK {
            let category = decode_api_error_category(&response_body)
                .unwrap_or_else(|| fallback_status_category(status));

            return Err(SynchronizationHttpClientError::HttpStatus {
                status: status.as_u16(),
                category,
            });
        }

        if !response_is_json {
            return Err(SynchronizationHttpClientError::UnexpectedContentType);
        }

        let acknowledgement =
            serde_json::from_slice::<SynchronizationBatchAcknowledgement>(&response_body)
                .map_err(|source| SynchronizationHttpClientError::ResponseDecode { source })?;

        acknowledgement
            .validate_against_request(request)
            .map_err(|source| SynchronizationHttpClientError::InvalidAcknowledgement { source })?;

        Ok(acknowledgement)
    }
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    maximum_bytes: usize,
) -> Result<Vec<u8>, SynchronizationHttpClientError> {
    let mut body = Vec::new();

    loop {
        let next_chunk = response.chunk().await.map_err(|source| {
            SynchronizationHttpClientError::ResponseRead {
                category: response_failure_category(&source),
                source,
            }
        })?;

        let Some(chunk) = next_chunk else {
            break;
        };

        let Some(total_bytes) = body.len().checked_add(chunk.len()) else {
            return Err(SynchronizationHttpClientError::ResponseTooLarge {
                maximum_bytes,
                actual_bytes: usize::MAX,
            });
        };

        if total_bytes > maximum_bytes {
            return Err(SynchronizationHttpClientError::ResponseTooLarge {
                maximum_bytes,
                actual_bytes: total_bytes,
            });
        }

        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

fn request_failure_category(error: &reqwest::Error) -> SynchronizationFailureCategory {
    if error.is_timeout() {
        SynchronizationFailureCategory::NetworkTimeout
    } else {
        SynchronizationFailureCategory::ConnectionFailure
    }
}

fn response_failure_category(error: &reqwest::Error) -> SynchronizationFailureCategory {
    if error.is_timeout() {
        SynchronizationFailureCategory::NetworkTimeout
    } else {
        SynchronizationFailureCategory::ResponseDecodeFailure
    }
}

fn decode_api_error_category(body: &[u8]) -> Option<SynchronizationFailureCategory> {
    let envelope = serde_json::from_slice::<ApiErrorEnvelope>(body).ok()?;

    if envelope.service != TRANSITGUARD_API_SERVICE {
        return None;
    }

    Some(envelope.category)
}

fn fallback_status_category(status: StatusCode) -> SynchronizationFailureCategory {
    match status {
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
            SynchronizationFailureCategory::NetworkTimeout
        }

        StatusCode::TOO_MANY_REQUESTS
        | StatusCode::BAD_GATEWAY
        | StatusCode::SERVICE_UNAVAILABLE => {
            SynchronizationFailureCategory::BackendTemporarilyUnavailable
        }

        StatusCode::CONFLICT => SynchronizationFailureCategory::BatchIdentityConflict,

        StatusCode::PAYLOAD_TOO_LARGE => SynchronizationFailureCategory::PayloadTooLarge,

        _ if status.is_server_error() => {
            SynchronizationFailureCategory::BackendTemporarilyUnavailable
        }

        _ => SynchronizationFailureCategory::BackendValidationFailure,
    }
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(CONTENT_TYPE) else {
        return false;
    };

    let Ok(value) = value.to_str() else {
        return false;
    };

    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case(JSON_MEDIA_TYPE))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use reqwest::{
        StatusCode,
        header::{CONTENT_TYPE, HeaderMap, HeaderValue},
    };
    use transitguard_device_protocol::SynchronizationFailureCategory;

    use super::{
        SynchronizationHttpClient, SynchronizationHttpClientError, fallback_status_category,
        has_json_content_type,
    };

    #[test]
    fn client_builds_versioned_endpoint() {
        let result =
            SynchronizationHttpClient::new("http://127.0.0.1:8080/base/", Duration::from_secs(5));

        let Ok(client) = result else {
            panic!("valid HTTP client configuration failed");
        };

        assert_eq!(
            client.endpoint().as_str(),
            concat!(
                "http://127.0.0.1:8080",
                "/v1/reader-synchronization/batches"
            )
        );
    }

    #[test]
    fn zero_timeout_is_rejected() {
        let result = SynchronizationHttpClient::new("http://127.0.0.1:8080", Duration::ZERO);

        assert!(matches!(
            result,
            Err(SynchronizationHttpClientError::InvalidTimeout)
        ));
    }

    #[test]
    fn embedded_credentials_are_rejected() {
        let result = SynchronizationHttpClient::new(
            "https://reader:secret@example.test",
            Duration::from_secs(5),
        );

        assert!(matches!(
            result,
            Err(SynchronizationHttpClientError::InvalidEndpoint)
        ));
    }

    #[test]
    fn unsupported_url_scheme_is_rejected() {
        let result =
            SynchronizationHttpClient::new("file:///tmp/transitguard", Duration::from_secs(5));

        assert!(matches!(
            result,
            Err(SynchronizationHttpClientError::InvalidEndpoint)
        ));
    }

    #[test]
    fn http_statuses_have_stable_fallback_categories() {
        assert_eq!(
            fallback_status_category(StatusCode::SERVICE_UNAVAILABLE),
            SynchronizationFailureCategory::BackendTemporarilyUnavailable
        );

        assert_eq!(
            fallback_status_category(StatusCode::CONFLICT),
            SynchronizationFailureCategory::BatchIdentityConflict
        );

        assert_eq!(
            fallback_status_category(StatusCode::PAYLOAD_TOO_LARGE),
            SynchronizationFailureCategory::PayloadTooLarge
        );

        assert_eq!(
            fallback_status_category(StatusCode::UNPROCESSABLE_ENTITY),
            SynchronizationFailureCategory::BackendValidationFailure
        );
    }

    #[test]
    fn json_content_type_accepts_parameters() {
        let mut headers = HeaderMap::new();

        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        assert!(has_json_content_type(&headers));
    }

    #[test]
    fn non_json_content_type_is_rejected() {
        let mut headers = HeaderMap::new();

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));

        assert!(!has_json_content_type(&headers));
    }
}
