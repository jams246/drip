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
