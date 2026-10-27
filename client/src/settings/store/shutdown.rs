use std::{fs, io, os::windows::fs::MetadataExt};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{SettingsError, SettingsStore, path_error};
use crate::settings::ClientSettings;

impl SettingsStore {
    pub(super) fn load_unlocked(&self) -> Result<ClientSettings, SettingsError> {
        match fs::symlink_metadata(&self.path) {
            Ok(_) => self.load_existing_unlocked(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ClientSettings::default()),
            Err(source) => Err(path_error("inspect", &self.path, source)),
        }
    }

    pub(crate) fn pristine_shutdown_state(&self) -> Result<bool, SettingsError> {
        let _lock = self.lock()?;
        match fs::symlink_metadata(&self.path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(path_error("inspect", &self.path, source)),
        }
        let settings = self.load_existing_unlocked()?;
        Ok(settings.selections.is_empty()
            && settings.acknowledged_generation == 0
            && settings.pending_update.is_none())
    }

    fn load_existing_unlocked(&self) -> Result<ClientSettings, SettingsError> {
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|source| path_error("inspect", &self.path, source))?;
        if !metadata.file_type().is_file()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(SettingsError::InvalidTarget(self.path.clone()));
        }
        let bytes =
            fs::read(&self.path).map_err(|source| path_error("read", &self.path, source))?;
        let settings: ClientSettings =
            serde_json::from_slice(&bytes).map_err(SettingsError::Decode)?;
        super::super::validation::validate(&settings)?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{PendingSelectionUpdate, SavedSelection};
    use drip_core::{SourcePath, protocol::EntryKind};

    fn store() -> SettingsStore {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.verification/phase-04/pristine-shutdown/store-tests")
            .join(crate::source::random_id().unwrap());
        fs::create_dir_all(&directory).unwrap();
        SettingsStore::open(&directory)
    }

    fn retained_selection() -> SavedSelection {
        SavedSelection {
            id: "retained".to_owned(),
            source: SourcePath::parse_absolute("C:\\old").unwrap(),
            root_kind: EntryKind::Directory,
            volume_identity: "volume".to_owned(),
            active: false,
        }
    }

    #[test]
    fn persisted_empty_object_proves_pristine_state() {
        let store = store();
        fs::write(&store.path, b"{}").unwrap();
        assert!(store.pristine_shutdown_state().unwrap());
    }

    #[test]
    fn missing_settings_do_not_prove_pristine_state() {
        assert!(!store().pristine_shutdown_state().unwrap());
    }

    #[test]
    fn retained_rows_do_not_prove_pristine_state() {
        let store = store();
        store
            .save(&ClientSettings {
                selections: vec![retained_selection()],
                ..ClientSettings::default()
            })
            .unwrap();
        assert!(!store.pristine_shutdown_state().unwrap());
    }

    #[test]
    fn acknowledged_history_without_active_rows_blocks_pristine_state() {
        let store = store();
        store
            .save(&ClientSettings {
                acknowledged_generation: 1,
                ..ClientSettings::default()
            })
            .unwrap();
        assert!(!store.pristine_shutdown_state().unwrap());
    }

    #[test]
    fn pending_generation_zero_blocks_pristine_state() {
        let store = store();
        store
            .save(&ClientSettings {
                pending_update: Some(PendingSelectionUpdate {
                    expected_generation: 0,
                    selections: Vec::new(),
                }),
                ..ClientSettings::default()
            })
            .unwrap();
        assert!(!store.pristine_shutdown_state().unwrap());
    }

    #[test]
    fn corrupt_settings_cannot_prove_pristine_state() {
        let store = store();
        fs::write(&store.path, b"not-json").unwrap();
        assert!(store.pristine_shutdown_state().is_err());
    }
}
