use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sarmg_error::{ErrorCode, ErrorEnvelope, HttpStatus};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    BadRequest(String),
    #[error("Client pairing protocol is unsupported")]
    UnsupportedClientProtocol { received: u16, supported: u16 },
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("client credential does not belong to the reported host")]
    ClientHostMismatch,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    UnsupportedMediaType(String),
    #[error("login rate limit exceeded")]
    LoginRateLimited { retry_after: u64 },
    #[error("{message}")]
    RateLimited {
        message: &'static str,
        retry_after: u64,
    },
    #[error("{0}")]
    Unavailable(String),
    #[error("{message}")]
    RetryableUnavailable {
        message: &'static str,
        retry_after: u64,
    },
    #[error("database is unavailable")]
    Database(#[source] anyhow::Error),
    #[error("internal server error")]
    Internal(#[source] anyhow::Error),
}

/// Marks responses which have already been serialized with the current
/// Foundation error contract. The outer API middleware uses this marker to
/// replace Axum extractor/route rejections without rewriting product errors.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FoundationErrorEnvelope;

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::BadRequest(_) | Self::UnsupportedClientProtocol { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::ClientHostMismatch => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::LoginRateLimited { .. } | Self::RateLimited { .. } => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::Unavailable(_) | Self::RetryableUnavailable { .. } => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::Database(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let retry_after = match &self {
            Self::LoginRateLimited { retry_after } | Self::RateLimited { retry_after, .. } => {
                Some(*retry_after)
            }
            Self::RetryableUnavailable { retry_after, .. } => Some(*retry_after),
            _ => None,
        };
        let message = self.to_string();
        let mut envelope = match &self {
            Self::BadRequest(_) => ErrorEnvelope::new(HttpStatus::BadRequest, message),
            Self::UnsupportedClientProtocol { .. } => {
                product_envelope("unsupported_client_protocol", message, false)
            }
            Self::Unauthorized => ErrorEnvelope::new(HttpStatus::Unauthorized, message),
            Self::Forbidden => ErrorEnvelope::new(HttpStatus::Forbidden, message),
            Self::ClientHostMismatch => product_envelope("client_host_mismatch", message, false),
            Self::NotFound(_) => ErrorEnvelope::new(HttpStatus::NotFound, message),
            Self::Conflict(_) => ErrorEnvelope::new(HttpStatus::Conflict, message),
            Self::UnsupportedMediaType(_) => {
                product_envelope("unsupported_media_type", message, false)
            }
            Self::LoginRateLimited { .. } | Self::RateLimited { .. } => {
                ErrorEnvelope::new(HttpStatus::TooManyRequests, message)
            }
            Self::Unavailable(_) | Self::RetryableUnavailable { .. } | Self::Database(_) => {
                ErrorEnvelope::new(HttpStatus::ServiceUnavailable, message)
            }
            Self::Internal(_) => ErrorEnvelope::new(HttpStatus::Internal, message),
        };
        if let Some(retry_after) = retry_after {
            envelope = envelope.with_detail("retry_after_seconds", retry_after);
        }
        if let Self::UnsupportedClientProtocol {
            received,
            supported,
        } = self
        {
            envelope = envelope
                .with_detail("received", received)
                .with_detail("supported", [supported]);
        }
        let mut response = (status, Json(envelope)).into_response();
        response.extensions_mut().insert(FoundationErrorEnvelope);
        if let Some(retry_after) = retry_after {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&retry_after.to_string())
                    .expect("an integer Retry-After is a valid header value"),
            );
        }
        response
    }
}

/// Produce a safe current-contract envelope for framework-generated API
/// failures (JSON extraction, body limits, method mismatch and unknown routes).
/// The original framework body is intentionally not reflected to callers.
pub(crate) fn framework_envelope(status: StatusCode) -> ErrorEnvelope {
    match status {
        StatusCode::BAD_REQUEST => ErrorEnvelope::new(HttpStatus::BadRequest, "bad request"),
        StatusCode::UNAUTHORIZED => ErrorEnvelope::new(HttpStatus::Unauthorized, "unauthorized"),
        StatusCode::FORBIDDEN => ErrorEnvelope::new(HttpStatus::Forbidden, "forbidden"),
        StatusCode::NOT_FOUND => ErrorEnvelope::new(HttpStatus::NotFound, "not found"),
        StatusCode::CONFLICT => ErrorEnvelope::new(HttpStatus::Conflict, "conflict"),
        StatusCode::UNPROCESSABLE_ENTITY => {
            ErrorEnvelope::new(HttpStatus::UnprocessableEntity, "unprocessable entity")
        }
        StatusCode::TOO_MANY_REQUESTS => {
            ErrorEnvelope::new(HttpStatus::TooManyRequests, "too many requests")
        }
        StatusCode::SERVICE_UNAVAILABLE => {
            ErrorEnvelope::new(HttpStatus::ServiceUnavailable, "service unavailable")
        }
        StatusCode::METHOD_NOT_ALLOWED => {
            product_envelope("method_not_allowed", "method not allowed", false)
        }
        StatusCode::PAYLOAD_TOO_LARGE => {
            product_envelope("payload_too_large", "request body is too large", false)
        }
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            product_envelope("unsupported_media_type", "unsupported media type", false)
        }
        _ if status.is_server_error() => {
            ErrorEnvelope::new(HttpStatus::Internal, "internal server error")
        }
        _ => product_envelope("http_error", "request failed", false),
    }
}

fn product_envelope(
    code: &'static str,
    message: impl Into<String>,
    retryable: bool,
) -> ErrorEnvelope {
    ErrorEnvelope::with_code(
        ErrorCode::new(code).expect("built-in Host Monitoring error code is valid"),
        message,
    )
    .retryable(retryable)
}

pub fn database(error: impl Into<anyhow::Error>) -> Error {
    let error = error.into();
    tracing::warn!(%error, "host-monitoring SQLite operation failed");
    Error::Database(error)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn errors_use_the_strict_foundation_envelope() {
        let response = Error::ClientHostMismatch.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            response
                .extensions()
                .get::<FoundationErrorEnvelope>()
                .is_some()
        );
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({
                "code": "client_host_mismatch",
                "message": "client credential does not belong to the reported host",
                "retryable": false
            })
        );
        serde_json::from_slice::<ErrorEnvelope>(&body).unwrap();
    }

    #[tokio::test]
    async fn retryable_errors_carry_header_and_machine_details() {
        let response = Error::RateLimited {
            message: "client report rate exceeded",
            retry_after: 3,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "3");
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({
                "code": "too_many_requests",
                "message": "client report rate exceeded",
                "retryable": true,
                "details": {"retry_after_seconds": 3}
            })
        );
    }

    #[tokio::test]
    async fn unsupported_protocol_is_a_structured_non_retryable_error() {
        let response = Error::UnsupportedClientProtocol {
            received: 2,
            supported: 1,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({
                "code": "unsupported_client_protocol",
                "message": "Client pairing protocol is unsupported",
                "retryable": false,
                "details": {"received": 2, "supported": [1]}
            })
        );
    }
}
