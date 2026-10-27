use drip_core::{Blake3Digest, SourcePath};
use serde::{Deserialize, Serialize};

/// Upload stage represented by a progress snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadPhase {
    /// Reading the source to calculate its full-file digest.
    Hashing,
    /// Chunking the source and hashing each chunk.
    Chunking,
    /// Sending missing verified chunk bytes to the server.
    Uploading,
    /// Waiting for server-side full-file verification and publication.
    Verifying,
}

/// Bounded-memory progress for one upload stage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    /// Current upload stage.
    pub phase: UploadPhase,
    /// Source path being uploaded.
    pub source_path: SourcePath,
    /// Bytes processed in this stage.
    pub processed_bytes: u64,
    /// Total bytes expected in this stage.
    pub total_bytes: Option<u64>,
    /// Cumulative source bytes read by the initial full-file hash pass.
    pub source_hashed_bytes: u64,
    /// Cumulative source bytes processed by FastCDC.
    pub source_chunked_bytes: u64,
    /// Cumulative number of source chunks processed by FastCDC.
    pub chunk_count: u64,
    /// Cumulative missing payload bytes planned across descriptor pages.
    pub planned_payload_bytes: u64,
    /// Cumulative file payload bytes sent, including retries.
    pub file_payload_sent: u64,
}

/// Typed events emitted by the engine for CLI or GUI consumers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivityEvent {
    /// Upload began for this source.
    UploadStarted {
        /// Original source path.
        source_path: SourcePath,
        /// Source size in bytes.
        size: u64,
    },
    /// Current bounded upload progress.
    Progress(ProgressSnapshot),
    /// Server verified and published the source.
    UploadCompleted {
        /// Original source path.
        source_path: SourcePath,
        /// Verified source size.
        size: u64,
        /// Full-file BLAKE3 digest.
        digest: Blake3Digest,
    },
}
