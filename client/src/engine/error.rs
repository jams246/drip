use std::io;

use drip_core::protocol::ApiErrorCode;
use reqwest::StatusCode;
use thiserror::Error;

use crate::{credentials::CredentialError, source::SourceError, transport::TransportError};

/// Pairing, validation, or upload failure without credential values.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("local client I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Settings(#[from] crate::settings::SettingsError),
    #[error("could not encode a control message: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("server response exceeds the 4 MiB client limit")]
    ResponseTooLarge,
    #[error("server returned invalid JSON")]
    ResponseJson(#[source] serde_json::Error),
    #[error("server returned HTTP {0} without a valid API error")]
    UnexpectedStatus(StatusCode),
    #[error("server rejected request with {code:?}: {message}")]
    Api {
        status: StatusCode,
        code: ApiErrorCode,
        message: String,
        retryable: bool,
    },
    #[error("server uses unsupported protocol version {0}")]
    UnsupportedProtocol(u32),
    #[error("enrollment token must be 64 lowercase hexadecimal characters")]
    InvalidEnrollmentToken,
    #[error("server response violates the upload protocol: {0}")]
    InvalidResponse(&'static str),
    #[error("upload selection changed; run the upload again")]
    SelectionChanged,
    #[error("upload destination does not match the verified source")]
    DestinationMismatch,
    #[error("upload commit outcome is uncertain and mirror contents do not match")]
    CommitUncertain,
    #[error("upload session was aborted before publication")]
    UploadAborted,
    #[error("sync operation cancelled by a control request")]
    Cancelled,
    #[error("upload failed and its abort outcome is uncertain: transfer={transfer}; abort={abort}")]
    AbortFailed {
        transfer: Box<EngineError>,
        abort: Box<EngineError>,
    },
}

impl EngineError {
    /// Returns whether the saved registration must be repaired.
    pub fn registration_required(&self) -> bool {
        match self {
            Self::Api { status, .. } | Self::UnexpectedStatus(status) => {
                *status == StatusCode::UNAUTHORIZED
            }
            Self::AbortFailed { transfer, abort } => {
                transfer.registration_required() || abort.registration_required()
            }
            _ => false,
        }
    }

    pub(crate) fn should_retry_request(&self) -> bool {
        match self {
            Self::Transport(TransportError::Request(_)) => true,
            Self::Api { retryable, .. } => *retryable,
            _ => false,
        }
    }

    pub(crate) fn is_uncertain_commit(&self) -> bool {
        match self {
            Self::Transport(TransportError::Request(_))
            | Self::ResponseJson(_)
            | Self::ResponseTooLarge => true,
            Self::UnexpectedStatus(status) => status.is_server_error(),
            Self::Api { status, .. } => status.is_server_error(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_api_response_requires_registration_repair() {
        let error = EngineError::Api {
            status: StatusCode::UNAUTHORIZED,
            code: ApiErrorCode::InvalidCredential,
            message: "registration rejected".into(),
            retryable: false,
        };

        assert!(error.registration_required());
    }

    #[test]
    fn unexpected_unauthorized_inside_abort_failure_requires_registration_repair() {
        let error = EngineError::AbortFailed {
            transfer: Box::new(EngineError::UploadAborted),
            abort: Box::new(EngineError::AbortFailed {
                transfer: Box::new(EngineError::UnexpectedStatus(StatusCode::UNAUTHORIZED)),
                abort: Box::new(EngineError::UploadAborted),
            }),
        };

        assert!(error.registration_required());
    }

    #[test]
    fn non_unauthorized_transport_and_io_errors_do_not_require_registration_repair() {
        for error in [
            EngineError::Api {
                status: StatusCode::FORBIDDEN,
                code: ApiErrorCode::InvalidCredential,
                message: "forbidden".into(),
                retryable: false,
            },
            EngineError::UnexpectedStatus(StatusCode::BAD_GATEWAY),
            EngineError::Transport(TransportError::WrongOrigin),
            EngineError::Io(io::Error::other("disk unavailable")),
        ] {
            assert!(!error.registration_required());
        }
    }

    #[test]
    fn structured_retryable_server_errors_retry_but_integrity_errors_do_not() {
        let retryable = EngineError::Api {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: ApiErrorCode::RetryableIo,
            message: "temporary filesystem failure".into(),
            retryable: true,
        };
        let corrupt = EngineError::Api {
            status: StatusCode::BAD_REQUEST,
            code: ApiErrorCode::CorruptContent,
            message: "chunk digest mismatch".into(),
            retryable: false,
        };

        assert!(retryable.should_retry_request());
        assert!(retryable.is_uncertain_commit());
        assert!(!corrupt.should_retry_request());
        assert!(matches!(
            corrupt,
            EngineError::Api {
                code: ApiErrorCode::CorruptContent,
                retryable: false,
                ..
            }
        ));
    }
}
