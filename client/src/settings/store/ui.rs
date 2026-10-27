use super::{SettingsError, SettingsStore};

impl SettingsStore {
    /// Saves the last registration label independently of the DPAPI binding.
    pub fn save_pc_display_name(&self, name: &str) -> Result<(), SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        settings.pc_display_name = Some(name.to_owned());
        self.save_unlocked(&settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_display_name_retains_pending_selection_generation() {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.verification/phase-04/ui-prefs-tests")
            .join(crate::source::random_id().unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let store = SettingsStore::open(&directory);
        let original = crate::settings::ClientSettings {
            acknowledged_generation: 4,
            pending_update: Some(crate::settings::PendingSelectionUpdate {
                expected_generation: 4,
                selections: vec![],
            }),
            ..crate::settings::ClientSettings::default()
        };
        store.save(&original).unwrap();
        store.save_pc_display_name("Synthetic PC").unwrap();
        let saved = store.load().unwrap();
        assert_eq!(saved.pc_display_name.as_deref(), Some("Synthetic PC"));
        assert_eq!(saved.acknowledged_generation, 4);
        assert_eq!(saved.pending_update, original.pending_update);
    }
}
