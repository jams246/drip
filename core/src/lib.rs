#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Shared protocol values and validated Windows source paths for Drip.

mod digest;
mod path;
mod secret;
mod time;
mod version;

/// HTTP control-message types shared by the client and server.
pub mod protocol;

pub use digest::{Blake3Digest, DigestError};
pub use path::{SourcePath, SourcePathError};
pub use secret::SensitiveString;
pub use time::{Timestamp, TimestampError};
pub use version::{PROTOCOL_VERSION, ProtocolVersion, ProtocolVersionError};
