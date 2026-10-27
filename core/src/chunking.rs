use std::io::Read;

use fastcdc::v2020::{Normalization, StreamCDC};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Blake3Digest;

/// Version identifying the fixed v1 chunking parameters.
pub const CHUNK_CONFIG_VERSION: u32 = 1;
/// Minimum FastCDC chunk size in bytes.
pub const CHUNK_MIN_SIZE: u32 = 65_536;
/// Target FastCDC chunk size in bytes.
pub const CHUNK_TARGET_SIZE: u32 = 262_144;
/// Maximum FastCDC chunk size in bytes.
pub const CHUNK_MAX_SIZE: u32 = 1_048_576;
/// Maximum descriptors in one upload plan page.
pub const CHUNK_PLAN_MAX_DESCRIPTORS: usize = 1_024;
/// Maximum encoded upload plan request size in bytes.
pub const CHUNK_PLAN_MAX_BODY_BYTES: usize = 256 * 1_024;

/// Version of the fixed chunking configuration accepted by this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkConfig {
    /// Chunking configuration version.
    pub chunking_version: u32,
}

impl ChunkConfig {
    /// Fixed v1 FastCDC 2020 configuration.
    pub const V1: Self = Self {
        chunking_version: CHUNK_CONFIG_VERSION,
    };

    /// Rejects configuration versions unsupported by this build.
    pub fn validate(self) -> Result<(), ChunkConfigError> {
        if self == Self::V1 {
            Ok(())
        } else {
            Err(ChunkConfigError(self.chunking_version))
        }
    }
}

/// One chunk's location and independent BLAKE3 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkDescriptor {
    /// Zero-based chunk ordinal.
    pub ordinal: u64,
    /// Byte offset in the complete file.
    pub offset: u64,
    /// Chunk length in bytes.
    pub length: u32,
    /// Full BLAKE3 digest of the chunk bytes.
    pub digest: Blake3Digest,
}

/// One bounded FastCDC chunk with its descriptor and literal bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkData {
    /// Chunk location and digest.
    pub descriptor: ChunkDescriptor,
    /// Literal bytes in this chunk.
    pub bytes: Vec<u8>,
}

/// Streaming FastCDC 2020 chunker with independent chunk and file hashes.
pub struct ChunkStream<R: Read> {
    chunks: StreamCDC<R>,
    file_hasher: blake3::Hasher,
    next_ordinal: u64,
    next_offset: u64,
    exhausted: bool,
    failed: bool,
}

impl<R: Read> ChunkStream<R> {
    /// Creates a stream using fixed v1 parameters and normalization level 1.
    pub fn new(reader: R) -> Self {
        let chunks = StreamCDC::with_level(
            reader,
            CHUNK_MIN_SIZE as usize,
            CHUNK_TARGET_SIZE as usize,
            CHUNK_MAX_SIZE as usize,
            Normalization::Level1,
        );

        Self {
            chunks,
            file_hasher: blake3::Hasher::new(),
            next_ordinal: 0,
            next_offset: 0,
            exhausted: false,
            failed: false,
        }
    }

    /// Reads and hashes the next bounded chunk, or returns `None` at EOF.
    pub fn next_chunk(&mut self) -> Result<Option<ChunkData>, ChunkStreamError> {
        if self.failed {
            return Err(ChunkStreamError::Failed);
        }
        let Some(chunk) = self.chunks.next() else {
            self.exhausted = true;
            return Ok(None);
        };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                self.failed = true;
                return Err(ChunkStreamError::FastCdc(error));
            }
        };

        if chunk.offset != self.next_offset || chunk.length != chunk.data.len() || chunk.length == 0
        {
            self.failed = true;
            return Err(ChunkStreamError::InvalidChunk);
        }

        let length = match u32::try_from(chunk.length) {
            Ok(length) => length,
            Err(_) => {
                self.failed = true;
                return Err(ChunkStreamError::InvalidChunk);
            }
        };
        if length > CHUNK_MAX_SIZE {
            self.failed = true;
            return Err(ChunkStreamError::InvalidChunk);
        }

        let next_ordinal = match self.next_ordinal.checked_add(1) {
            Some(ordinal) => ordinal,
            None => {
                self.failed = true;
                return Err(ChunkStreamError::Overflow);
            }
        };
        let next_offset = match self.next_offset.checked_add(u64::from(length)) {
            Some(offset) => offset,
            None => {
                self.failed = true;
                return Err(ChunkStreamError::Overflow);
            }
        };
        let descriptor = ChunkDescriptor {
            ordinal: self.next_ordinal,
            offset: self.next_offset,
            length,
            digest: Blake3Digest::new(*blake3::hash(&chunk.data).as_bytes()),
        };
        self.file_hasher.update(&chunk.data);
        self.next_ordinal = next_ordinal;
        self.next_offset = next_offset;

        Ok(Some(ChunkData {
            descriptor,
            bytes: chunk.data,
        }))
    }

    /// Returns the complete-file digest after the stream reaches EOF.
    pub fn finish(self) -> Result<Blake3Digest, ChunkStreamError> {
        if self.failed {
            return Err(ChunkStreamError::Failed);
        }
        if !self.exhausted {
            return Err(ChunkStreamError::NotExhausted);
        }
        Ok(Blake3Digest::new(*self.file_hasher.finalize().as_bytes()))
    }
}

/// Chunk stream failure.
#[derive(Debug, Error)]
pub enum ChunkStreamError {
    /// FastCDC source read failed.
    #[error("FastCDC stream failed: {0}")]
    FastCdc(#[source] fastcdc::v2020::Error),
    /// A chunk's byte length, offset, or bounded size was invalid.
    #[error("FastCDC returned an invalid chunk")]
    InvalidChunk,
    /// Stream cannot continue after an earlier read or validation failure.
    #[error("chunk stream already failed")]
    Failed,
    /// Chunk ordinal or file offset exceeded its integer range.
    #[error("chunk ordinal or file offset overflowed")]
    Overflow,
    /// Full-file digest requested before the stream reached EOF.
    #[error("chunk stream must reach EOF before finalizing its file digest")]
    NotExhausted,
}

/// Unsupported fixed chunking configuration version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("unsupported chunking configuration version: {0}")]
pub struct ChunkConfigError(pub u32);
