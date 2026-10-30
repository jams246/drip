//! Windows client foundations for pairing and authenticated transport.

pub mod transport;

#[cfg(windows)]
pub mod credentials;

#[cfg(windows)]
mod private_file;

#[cfg(windows)]
mod engine;

#[cfg(windows)]
pub mod source;

#[cfg(windows)]
pub mod settings;

#[cfg(windows)]
pub mod windows_integration;

#[cfg(windows)]
pub use engine::{
    ActivityEvent, ActivityLog, ConnectionState, Engine, EngineError, EventKind, EventSink,
    LifecycleEvent, ProgressSnapshot, RootStatus, RunControl, SelectionProgress, SelectionState,
    SyncSnapshot, UploadPhase, UploadResult,
};
