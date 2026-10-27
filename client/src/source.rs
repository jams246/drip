#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::SourceFile;

#[cfg(windows)]
pub(crate) use windows::{SourceError, random_id, same_path};
