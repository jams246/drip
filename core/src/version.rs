use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current `/v1/` control protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Numeric protocol version advertised during enrollment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolVersion(u32);

impl ProtocolVersion {
    /// Creates a version value received from the wire.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns this version number.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Rejects protocol versions unsupported by this build.
    pub fn validate(self) -> Result<(), ProtocolVersionError> {
        if self.0 == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ProtocolVersionError(self.0))
        }
    }
}

/// Unsupported wire protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("unsupported protocol version: {0}")]
pub struct ProtocolVersionError(pub u32);
