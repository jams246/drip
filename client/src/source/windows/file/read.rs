use std::io::{Read, Seek, SeekFrom};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::super::{
    error::SourceError,
    native::{self, file_info, is_disk_file, open_file},
};
use super::SourceFile;
use crate::source::windows::root::file_volume_identity;

impl SourceFile {
    pub(crate) fn chunk_reader(&self) -> Result<std::fs::File, SourceError> {
        self.open_reader()
    }

    pub(crate) async fn read_range(
        &self,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, SourceError> {
        let end = offset
            .checked_add(u64::from(length))
            .ok_or(SourceError::Changed)?;
        if length == 0 || end > self.initial.size {
            return Err(SourceError::Changed);
        }
        self.verify().await?;
        let mut file = self.open_reader()?;
        let expected = self.initial;
        let length = usize::try_from(length).map_err(|_| SourceError::Changed)?;
        let bytes = tokio::task::spawn_blocking(move || {
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; length];
            file.read_exact(&mut bytes)?;
            if file_info(&file)?.observation()? != expected {
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
