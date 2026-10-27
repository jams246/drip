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
pub(crate) use windows::{SourceError, random_id, same_path};
