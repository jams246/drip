use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::RETRY_AFTER},
    response::{IntoResponse, Response},
};
use drip_core::protocol::{ApiError, ApiErrorCode};

use crate::StoreError;

/// HTTP failure with the shared JSON response shape.
#[derive(Debug)]
pub struct ApiFailure {
    status: StatusCode,
    body: ApiError,
}

#[derive(Clone, Copy)]
struct ApiFailureMarker;

impl ApiFailure {
    pub(crate) fn invalid_input() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidInput,
            "invalid request",
            false,
        )
    }

    pub(crate) fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ApiErrorCode::PayloadTooLarge,
            "control request exceeds 256 KiB",
            false,
        )
    }

    pub(crate) fn request_timeout() -> Self {
        Self::new(
            StatusCode::REQUEST_TIMEOUT,
            ApiErrorCode::RequestTimeout,
            "control request timed out",
            true,
        )
    }

    pub(crate) fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::NotFound,
            "resource not found",
            false,
        )
    }

    pub(crate) fn is_marked(response: &Response) -> bool {
        response.extensions().get::<ApiFailureMarker>().is_some()
    }

    fn new(status: StatusCode, code: ApiErrorCode, message: &str, retryable: bool) -> Self {
        Self {
            status,
            body: ApiError {
                code,
                message: message.to_owned(),
                retryable,
            },
        }
    }
}

impl From<StoreError> for ApiFailure {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::InvalidInput => Self::invalid_input(),
            StoreError::InvalidCredential => Self::new(
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::InvalidCredential,
                "invalid credential",
                false,
            ),
            StoreError::NotFound => Self::new(
                StatusCode::NOT_FOUND,
                ApiErrorCode::NotFound,
                "resource not found",
                false,
            ),
            StoreError::DisallowedNamespace => Self::new(
                StatusCode::FORBIDDEN,
                ApiErrorCode::DisallowedNamespace,
                "selection is not active for this device",
                false,
            ),
            StoreError::Conflict => Self::new(
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                "request conflicts with current state",
                false,
            ),
            StoreError::CorruptContent => Self::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::CorruptContent,
                "uploaded content failed integrity verification",
                false,
            ),
            StoreError::RetryableIo => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::RetryableIo,
                "temporary storage failure; retry shortly",
                true,
            ),
            StoreError::UnsupportedEntry => Self::new(
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                "mirror contains an unsupported entry; resolve it before syncing this selection",
                false,
            ),
            StoreError::UnsupportedExchange => Self::new(
                StatusCode::CONFLICT,
                ApiErrorCode::Conflict,
                "mirror filesystem cannot safely exchange files and directories; use a filesystem with atomic exchange support",
                false,
            ),
            StoreError::Busy => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::CapacityExhausted,
                "server is busy; retry shortly",
                true,
            ),
            StoreError::CapacityExhausted => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorCode::CapacityExhausted,
                "upload capacity is exhausted; retry shortly",
                true,
            ),
            StoreError::InsufficientStagingSpace => Self::new(
                StatusCode::INSUFFICIENT_STORAGE,
                ApiErrorCode::InsufficientStagingSpace,
                "upload staging has insufficient space",
                false,
            ),
            StoreError::Random
            | StoreError::Database(_)
            | StoreError::Filesystem(_)
            | StoreError::Serialization(_)
            | StoreError::Clock => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::InternalError,
                "internal server error",
                false,
            ),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        response.extensions_mut().insert(ApiFailureMarker);
        if self.status == StatusCode::SERVICE_UNAVAILABLE {
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}
