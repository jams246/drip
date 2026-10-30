use std::{io, path::PathBuf};

/// Source paths or handles rejected before upload.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("source must be an absolute local drive-letter path with valid Unicode")]
    InvalidPath,
    #[error("network drives are not supported")]
    NetworkDrive,
    #[error("source must be a regular file, not a directory or reparse point")]
    NotRegular,
    #[error("source path contains a reparse point: {0}")]
    ReparsePoint(PathBuf),
    #[error("case-sensitive source directory is not supported: {0}")]
    CaseSensitiveDirectory(PathBuf),
    #[error("Drip application data cannot be uploaded")]
    ExcludedApplicationData,
    #[error("source scan exceeds its safety limits; reduce the selected tree")]
    ScanLimit,
    #[error("source path resolves through a different path")]
    PathResolution,
    #[error("source file changed during upload")]
    Changed,
    #[error("source pathname now identifies a different file or volume")]
    Replaced,
    #[error("selected source root is unavailable: {0}")]
    RootUnavailable(#[source] io::Error),
    #[error("selected source volume identity changed")]
    VolumeChanged,
    #[error("selected source root kind changed and needs a new selection generation")]
    RootKindChanged,
    #[error("source last-write timestamp is invalid")]
    InvalidTimestamp,
    #[error("could not read source: {0}")]
    Io(#[from] io::Error),
    #[error("Windows source inspection failed: {0}")]
    Windows(#[from] windows::core::Error),
}
