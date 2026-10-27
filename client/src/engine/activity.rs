use drip_core::{Blake3Digest, SourcePath};

/// Upload stage represented by a progress snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadPhase {
    /// Reading the source to calculate its full-file digest.
    Hashing,
    /// Sending the verified source bytes to the server.
    Uploading,
}

/// Bounded-memory progress for one upload stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressSnapshot {
    /// Current upload stage.
    pub phase: UploadPhase,
    /// Source path being uploaded.
    pub source_path: SourcePath,
    /// Bytes processed in this stage.
    pub processed_bytes: u64,
    /// Total bytes expected in this stage.
    pub total_bytes: u64,
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
