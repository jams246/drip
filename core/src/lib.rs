#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Shared protocol values and validated Windows source paths for Drip.

mod chunking;
mod digest;
mod path;
mod secret;
mod time;
mod version;

/// HTTP control-message types shared by the client and server.
pub mod protocol;

pub use chunking::{
    CHUNK_CONFIG_VERSION, CHUNK_MAX_SIZE, CHUNK_MIN_SIZE, CHUNK_PLAN_MAX_BODY_BYTES,
    CHUNK_PLAN_MAX_DESCRIPTORS, CHUNK_TARGET_SIZE, ChunkConfig, ChunkConfigError, ChunkData,
    ChunkDescriptor, ChunkStream, ChunkStreamError,
};
pub use digest::{Blake3Digest, DigestError};
pub use path::{SourcePath, SourcePathError};
pub use secret::SensitiveString;
pub use time::{Timestamp, TimestampError};
pub use version::{PROTOCOL_VERSION, ProtocolVersion, ProtocolVersionError};
