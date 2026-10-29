use std::{fs, io, path::PathBuf};

use std::os::windows::fs::MetadataExt;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::super::{SourceError, native};
use super::{open_directory, verify_volume};
use crate::settings::SavedSelection;

/// Confirms absence through readable ancestors on the original volume without following links.
pub(crate) fn confirm_root_absent(selection: &SavedSelection) -> Result<bool, SourceError> {
    native::validate_drive(selection.source.drive())?;
    native::reject_application_data(&selection.source)?;
    let mut path = PathBuf::from(format!("{}:\\", selection.source.drive()));
    let mut directories = vec![open_directory(&path)?];
    verify_volume(&directories[0], &selection.volume_identity)?;
    for (index, component) in selection.source.components().iter().enumerate() {
        let next = path.join(component);
        match fs::symlink_metadata(&next) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let missing = drip_core::SourcePath::try_from_path(&next)
                    .map_err(|_| SourceError::InvalidPath)?;
                let present = fs::read_dir(&path)?.try_fold(false, |found, entry| {
                    let entry = entry?;
                    let candidate = drip_core::SourcePath::try_from_path(&entry.path())
                        .map_err(|_| SourceError::InvalidPath)?;
                    Ok::<_, SourceError>(found || native::same_path(&candidate, &missing))
                })?;
                for directory in &directories {
                    verify_volume(directory, &selection.volume_identity)?;
                }
                return Ok(!present);
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                    return Err(SourceError::ReparsePoint(next));
                }
                if index + 1 == selection.source.components().len() {
                    return Ok(false);
                }
                let directory = open_directory(&next)?;
                verify_volume(&directory, &selection.volume_identity)?;
                directories.push(directory);
                path = next;
            }
        }
    }
    Ok(false)
}
