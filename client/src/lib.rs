//! Windows client foundations for pairing and authenticated transport.

pub mod transport;

#[cfg(windows)]
pub mod credentials;

#[cfg(windows)]
mod engine;

#[cfg(windows)]
pub mod source;

#[cfg(windows)]
pub mod settings;

#[cfg(windows)]
pub use engine::{
    ActivityEvent, ActivityLog, Engine, EngineError, EventKind, EventSink, LifecycleEvent,
    ProgressSnapshot, SelectionProgress, UploadPhase, UploadResult,
};
