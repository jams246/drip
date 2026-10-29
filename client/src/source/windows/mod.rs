mod error;
mod file;
mod native;
mod root;
mod scan;

pub use error::SourceError;
pub(crate) use file::Observation;
pub use file::SourceFile;
pub(crate) use native::is_application_data_name;
pub(crate) use native::{random_id, same_path};
pub(crate) use root::confirm_root_absent;
pub use root::{SourceRoot, capture_root, confirm_absent, validate_root};
pub use scan::{ScanIssue, ScannedEntry, SourceScan, scan_root};

#[cfg(test)]
mod tests;
