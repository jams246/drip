use serde::{Deserialize, Serialize};

use crate::{ProtocolVersion, SensitiveString};

/// One-time token and display label sent to `POST /v1/enroll`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    /// One-time server-generated enrollment token.
    pub token: SensitiveString,
    /// Human-readable registration label.
    pub label: String,
}

/// Device binding and credential returned by `POST /v1/enroll`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    /// Server-generated device identifier.
    pub pc_id: String,
    /// Stable mirror directory assigned to this device.
    pub mirror_directory: String,
    /// Device credential for authenticated requests.
    pub device_credential: SensitiveString,
    /// Protocol version supported by the server.
    pub protocol_version: ProtocolVersion,
}
