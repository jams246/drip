use super::SettingsStore;
use crate::{
    settings::{SavedSelection, SettingsError, selection},
    source::confirm_root_absent,
};

impl SettingsStore {
    pub(crate) fn remove_deleted_root(
        &self,
        saved: &SavedSelection,
        generation: u64,
    ) -> Result<bool, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        if settings.pending_update.is_some()
            || settings.acknowledged_generation != generation
            || !settings
                .selections
                .iter()
                .any(|current| current == saved && current.active)
        {
            return Ok(false);
        }
        if !confirm_root_absent(saved).map_err(selection::SelectionError::Source)? {
            return Ok(false);
        }
        selection::remove(&mut settings, &saved.source)?;
        self.save_unlocked(&settings)?;
        Ok(true)
    }
}
