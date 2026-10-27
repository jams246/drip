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
    /// Control body exceeds the configured limit.
    PayloadTooLarge,
    /// Staging filesystem lacks space.
    InsufficientStagingSpace,
    /// Upload capacity is temporarily exhausted.
    CapacityExhausted,
    /// Protocol version is unsupported.
    UnsupportedVersion,
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
