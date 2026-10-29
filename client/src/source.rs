#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::SourceFile;

#[cfg(windows)]
pub use windows::{
    ScanIssue, ScannedEntry, SourceRoot, SourceScan, capture_root, confirm_absent, scan_root,
    validate_root,
};

#[cfg(windows)]
pub(crate) use windows::{Observation, SourceError, random_id, same_path};

#[cfg(windows)]
pub(crate) use windows::is_application_data_name;

#[cfg(windows)]
pub(crate) use windows::confirm_root_absent;
