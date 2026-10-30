use serde::{Deserialize, Serialize};

/// Stable API failure categories for `/v1/` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    /// Request did not satisfy protocol validation.
    InvalidInput,
    /// Credential was absent, invalid, or revoked.
    InvalidCredential,
    /// Authenticated device cannot mutate this namespace or selection.
    DisallowedNamespace,
    /// Requested resource is absent or belongs to another device.
    NotFound,
    /// Generation, revision, or session state is stale.
    Conflict,
    /// Uploaded or staged bytes failed their declared content digest.
    CorruptContent,
    /// Temporary server filesystem failure may succeed after retry.
    RetryableIo,
    /// Control body exceeds the configured limit.
    PayloadTooLarge,
    /// Control request body did not arrive within its deadline.
    RequestTimeout,
    /// Staging filesystem lacks space.
    InsufficientStagingSpace,
    /// Upload capacity is temporarily exhausted.
    CapacityExhausted,
    /// Protocol version is unsupported.
    UnsupportedVersion,
    /// Unexpected server failure without additional diagnostic details.
    InternalError,
}

/// Secret-free JSON API failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// Stable failure category.
    pub code: ApiErrorCode,
    /// Human-readable explanation without credentials or secrets.
    pub message: String,
    /// Whether retrying may succeed without changing the request.
    pub retryable: bool,
}
