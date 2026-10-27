use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use drip_core::protocol::Selection;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;

use crate::{database, mirror};

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
    /// Request conflicts with current generation or retained selection state.
    #[error("state conflict")]
    Conflict,
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
    /// System clock could not be represented as Unix seconds.
    #[error("system clock is outside supported range")]
    Clock,
}

/// Authenticated identity passed to a mutation held under SQLite's writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    /// Server-generated device identifier.
    pub(crate) pc_id: String,
    /// Stable, sanitized directory name inside the mirror capability.
    pub(crate) mirror_directory: String,
}

/// Device details available to the local administration CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    /// Server-generated device identifier.
    pub pc_id: String,
    /// Human-readable label supplied during enrollment.
    pub label: String,
    /// Whether the device credential remains valid.
    pub active: bool,
}

#[derive(Clone)]
pub struct Store {
    pub(crate) database_path: Arc<PathBuf>,
    pub(crate) mirror: Arc<Dir>,
    pub(crate) clock: Arc<dyn Fn() -> StoreResult<i64> + Send + Sync>,
}

impl Store {
    /// Opens the database and mirror root, creating them when absent.
    pub fn open(
        state_directory: impl AsRef<Path>,
        mirror_directory: impl AsRef<Path>,
    ) -> StoreResult<Self> {
        let state_directory = state_directory.as_ref();
        let mirror_directory = mirror_directory.as_ref();
        std::fs::create_dir_all(state_directory).map_err(database::io_error)?;
        std::fs::create_dir_all(mirror_directory).map_err(database::io_error)?;
        let database_path = state_directory.join("drip.sqlite3");
        let mirror = mirror::open_root(mirror_directory)?;
        let store = Self {
            database_path: Arc::new(database_path),
            mirror: Arc::new(mirror),
            clock: Arc::new(system_unix_seconds),
        };
        let connection = store.connect()?;
        database::initialize(&connection)?;
        Ok(store)
    }

    /// Opens storage with a controllable Unix clock, useful for deterministic expiry checks.
    pub fn open_with_clock<F>(
        state_directory: impl AsRef<Path>,
        mirror_directory: impl AsRef<Path>,
        clock: F,
    ) -> StoreResult<Self>
    where
        F: Fn() -> StoreResult<i64> + Send + Sync + 'static,
    {
        let mut store = Self::open(state_directory, mirror_directory)?;
        store.clock = Arc::new(clock);
        Ok(store)
    }

    pub(crate) fn connect(&self) -> StoreResult<Connection> {
        database::connect(&self.database_path)
    }

    pub(crate) fn now(&self) -> StoreResult<i64> {
        (self.clock)()
    }

    /// Rechecks credentials inside `BEGIN IMMEDIATE`, then runs one visible mutation before commit.
    ///
    /// Upload publication and deletion must use this method so CLI revocation and selection updates
    /// cannot acknowledge success while older authorized filesystem work remains in flight.
    pub(crate) fn with_authorized_mutation<T>(
        &self,
        credential: &str,
        mutation: impl FnOnce(&Transaction<'_>, &DeviceIdentity) -> StoreResult<T>,
    ) -> StoreResult<T> {
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database::sql_error)?;
        let device = crate::auth::authenticate_transaction(&transaction, credential)?;
        let result = mutation(&transaction, &device)?;
        transaction.commit().map_err(database::sql_error)?;
        Ok(result)
    }

    /// Runs a mutation with the authenticated device's no-follow mirror capability.
    pub(crate) fn with_authorized_device_mutation<T>(
        &self,
        credential: &str,
        mutation: impl FnOnce(&Transaction<'_>, &DeviceIdentity, &Dir) -> StoreResult<T>,
    ) -> StoreResult<T> {
        self.with_authorized_mutation(credential, |transaction, device| {
            let directory = self.open_device_directory(device)?;
            mutation(transaction, device, &directory)
        })
    }

    /// Rechecks credential, active selection, and generation under one writer lock.
    pub fn with_authorized_selection_mutation<T>(
        &self,
        credential: &str,
        selection_id: &str,
        expected_generation: u64,
        mutation: impl FnOnce(&Transaction<'_>, &DeviceIdentity, &Selection, &Dir) -> StoreResult<T>,
    ) -> StoreResult<T> {
        self.with_authorized_device_mutation(credential, |transaction, device, directory| {
            let selection = crate::selections::require_active_selection(
                transaction,
                device,
                selection_id,
                expected_generation,
            )?;
            mutation(transaction, device, &selection, directory)
        })
    }

    pub(crate) fn open_device_directory(&self, device: &DeviceIdentity) -> StoreResult<Dir> {
        self.mirror
            .open_dir_nofollow(&device.mirror_directory)
            .map_err(database::io_error)
    }
}

fn system_unix_seconds() -> StoreResult<i64> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Clock)?;
    i64::try_from(duration.as_secs()).map_err(|_| StoreError::Clock)
}
