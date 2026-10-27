use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CHUNK_MAX_SIZE, CHUNK_MIN_SIZE, CHUNK_PLAN_MAX_DESCRIPTORS, ChunkConfig, ChunkConfigError,
    ChunkDescriptor,
};

/// One ordered page of descriptors for an upload session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadPlanRequest {
    /// Fixed chunk configuration used to produce these descriptors.
    pub config: ChunkConfig,
    /// Ordered descriptors beginning at the session's next ordinal and offset.
    pub descriptors: Vec<ChunkDescriptor>,
}

impl UploadPlanRequest {
    /// Validates one page against the expected file size and next range.
    pub fn validate(
        &self,
        expected_size: u64,
        next_ordinal: u64,
        next_offset: u64,
    ) -> Result<(), UploadPlanValidationError> {
        self.config.validate()?;
        if self.descriptors.len() > CHUNK_PLAN_MAX_DESCRIPTORS {
            return Err(UploadPlanValidationError::TooManyDescriptors);
        }
        if expected_size == 0 {
            return if self.descriptors.is_empty() && next_ordinal == 0 && next_offset == 0 {
                Ok(())
            } else {
                Err(UploadPlanValidationError::InvalidEmptyFilePlan)
            };
        }
        if self.descriptors.is_empty() {
            return Err(UploadPlanValidationError::EmptyPage);
        }
        if next_offset >= expected_size {
            return Err(UploadPlanValidationError::FileAlreadyCovered);
        }

        let mut ordinal = next_ordinal;
        let mut offset = next_offset;
        let last_index = self.descriptors.len() - 1;
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            if descriptor.ordinal != ordinal {
                return Err(UploadPlanValidationError::UnexpectedOrdinal);
            }
            if descriptor.offset != offset {
                return Err(UploadPlanValidationError::UnexpectedOffset);
            }
            if descriptor.length == 0 || descriptor.length > CHUNK_MAX_SIZE {
                return Err(UploadPlanValidationError::InvalidLength);
            }

            let end = offset
                .checked_add(u64::from(descriptor.length))
                .ok_or(UploadPlanValidationError::RangeOverflow)?;
            if end > expected_size {
                return Err(UploadPlanValidationError::RangeBeyondFile);
            }
            if end < expected_size && descriptor.length < CHUNK_MIN_SIZE {
                return Err(UploadPlanValidationError::InvalidLength);
            }
            if end == expected_size && index != last_index {
                return Err(UploadPlanValidationError::DescriptorsAfterFinalChunk);
            }

            offset = end;
            ordinal = ordinal
                .checked_add(1)
                .ok_or(UploadPlanValidationError::RangeOverflow)?;
        }
        Ok(())
    }
}

/// Missing descriptor ordinals for one accepted upload plan page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadPlanResponse {
    /// Chunks the client must send before posting another page.
    pub missing_ordinals: Vec<u64>,
}

/// Invalid upload plan page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UploadPlanValidationError {
    /// Chunking configuration version is unsupported.
    #[error(transparent)]
    UnsupportedConfiguration(#[from] ChunkConfigError),
    /// Page contains more than 1,024 descriptors.
    #[error("upload plan page contains too many descriptors")]
    TooManyDescriptors,
    /// Non-empty files cannot use empty plan pages.
    #[error("upload plan page is empty")]
    EmptyPage,
    /// Empty-file plans must contain no descriptors at the initial cursor.
    #[error("empty file plan must contain no descriptors at the initial cursor")]
    InvalidEmptyFilePlan,
    /// All file bytes were already covered by an earlier page.
    #[error("file is already covered by prior plan pages")]
    FileAlreadyCovered,
    /// Descriptor ordinal does not match the next expected ordinal.
    #[error("descriptor ordinal is not contiguous")]
    UnexpectedOrdinal,
    /// Descriptor offset does not match the next expected offset.
    #[error("descriptor offset is not contiguous")]
    UnexpectedOffset,
    /// Descriptor length violates the fixed chunk bounds.
    #[error("descriptor length violates fixed chunk bounds")]
    InvalidLength,
    /// Descriptor offset plus length overflowed.
    #[error("descriptor range overflowed")]
    RangeOverflow,
    /// Descriptor range extends beyond the expected file size.
    #[error("descriptor range extends beyond file size")]
    RangeBeyondFile,
    /// Descriptor followed the chunk that completed the file.
    #[error("descriptors follow the final file chunk")]
    DescriptorsAfterFinalChunk,
}
