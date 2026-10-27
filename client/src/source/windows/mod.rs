mod error;
mod file;
mod native;

pub use error::SourceError;
pub use file::SourceFile;
pub(crate) use native::{random_id, same_path};

#[cfg(test)]
mod tests;
