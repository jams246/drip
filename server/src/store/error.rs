use thiserror::Error;

/// Result of a server storage operation.
pub type StoreResult<T> = Result<T, StoreError>;

/// Storage failures, with retryable SQLite contention kept distinct.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Request data did not satisfy server validation.
    #[error("invalid input")]
    InvalidInput,
    /// Token or device credential is absent, expired, consumed, or revoked.
    #[error("invalid credential")]
    InvalidCredential,
    /// Requested device or selection does not exist.
    #[error("resource not found")]
    NotFound,
    /// Authenticated device cannot mutate an inactive or unassigned selection.
    #[error("selection is outside the active device namespace")]
    DisallowedNamespace,
    /// Actual mirror contains an entry the protocol cannot represent safely.
    #[error("mirror contains an unsupported entry")]
    UnsupportedEntry,
    /// Mirror filesystem does not support atomic file/directory exchange.
    #[error("mirror filesystem does not support safe file/directory exchange")]
    UnsupportedExchange,
    /// Request conflicts with current generation or retained selection state.
    #[error("state conflict")]
    Conflict,
    /// Declared chunk or staged file bytes failed content verification.
    #[error("uploaded content failed integrity verification")]
    CorruptContent,
    /// Temporary filesystem failure can succeed after retry.
    #[error("temporary filesystem failure")]
    RetryableIo,
    /// SQLite writer lock could not be acquired within the bounded wait.
    #[error("storage is busy")]
    Busy,
    /// Operating system could not provide cryptographically secure random bytes.
    #[error("operating system random source failed")]
    Random,
    /// SQLite operation failed.
    #[error("database operation failed: {0}")]
    Database(#[source] rusqlite::Error),
    /// Server state or mirror filesystem operation failed.
    #[error("filesystem operation failed: {0}")]
    Filesystem(#[source] std::io::Error),
    /// Stored protocol value could not be serialized or decoded.
    #[error("stored protocol value is invalid: {0}")]
    Serialization(#[source] serde_json::Error),
    /// Upload staging has no available space.
    #[error("upload staging has insufficient space")]
    InsufficientStagingSpace,
    /// Upload session capacity is temporarily exhausted.
    #[error("upload capacity is exhausted")]
    CapacityExhausted,
    /// System clock could not be represented as Unix seconds.
    #[error("system clock is outside supported range")]
    Clock,
}
