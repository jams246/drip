use std::io::{Read, Seek, SeekFrom};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

#[cfg(test)]
use super::super::read_metrics::CountedReader;
use super::super::{
    error::SourceError,
    native::{self, file_info, is_disk_file, open_file},
};
use super::SourceFile;
use crate::source::windows::root::file_volume_identity;

impl SourceFile {
    #[cfg(not(test))]
    pub(crate) fn chunk_reader(&self) -> Result<std::fs::File, SourceError> {
        self.open_reader()
    }

    #[cfg(test)]
    pub(crate) fn chunk_reader(&self) -> Result<CountedReader, SourceError> {
        Ok(CountedReader::new(
            self.open_reader()?,
            self.protocol_path.clone(),
            crate::engine::test_metrics::ReadKind::Chunking,
        ))
    }

    pub(crate) async fn read_range(
        &self,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, SourceError> {
        self.read_range_inner(offset, length, || {}).await
    }

    #[cfg(test)]
    pub(crate) async fn read_range_with_checkpoint(
        &self,
        offset: u64,
        length: u32,
        checkpoint: impl FnOnce() + Send + 'static,
    ) -> Result<Vec<u8>, SourceError> {
        self.read_range_inner(offset, length, checkpoint).await
    }

    async fn read_range_inner(
        &self,
        offset: u64,
        length: u32,
        checkpoint: impl FnOnce() + Send + 'static,
    ) -> Result<Vec<u8>, SourceError> {
        let end = offset
            .checked_add(u64::from(length))
            .ok_or(SourceError::Changed)?;
        if length == 0 || end > self.initial.size {
            return Err(SourceError::Changed);
        }
        self.verify().await?;
        let file = self.open_reader()?;
        let expected = self.initial;
        let length = usize::try_from(length).map_err(|_| SourceError::Changed)?;
        #[cfg(test)]
        let source_path = self.protocol_path.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let mut file = CountedReader::new(
                file,
                source_path,
                crate::engine::test_metrics::ReadKind::RangeReread,
            );
            #[cfg(not(test))]
            let mut file = file;
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; length];
            file.read_exact(&mut bytes)?;
            checkpoint();
            #[cfg(test)]
            let current = file_info(file.inner())?.observation()?;
            #[cfg(not(test))]
            let current = file_info(&file)?.observation()?;
            if current != expected {
                return Err(SourceError::Changed);
            }
            Ok(bytes)
        })
        .await
        .map_err(|error| SourceError::Io(std::io::Error::other(error.to_string())))??;
        self.verify().await?;
        Ok(bytes)
    }

    fn open_reader(&self) -> Result<std::fs::File, SourceError> {
        let file = open_file(&self.path)?;
        let info = file_info(&file)?;
        if !is_disk_file(&file)
            || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || info.standard.Directory
        {
            return Err(SourceError::NotRegular);
        }
        native::validate_final_path(&file, &self.protocol_path)?;
        let observation = info.observation()?;
        if observation.volume != self.initial.volume || observation.file_id != self.initial.file_id
        {
            return Err(SourceError::Replaced);
        }
        if file_volume_identity(&file)? != self.volume_identity {
            return Err(SourceError::VolumeChanged);
        }
        if observation != self.initial {
            return Err(SourceError::Changed);
        }
        Ok(file)
    }
}
