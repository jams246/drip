use super::SettingsStore;
use crate::{
    settings::{SettingsError, selection},
    source::{SourceError, SourceRoot, same_path},
};

impl SettingsStore {
    /// Refreshes observed canonical spelling and kind without changing source-volume authority.
    pub fn refresh_root(&self, id: &str, current: &SourceRoot) -> Result<bool, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        let saved = settings
            .selections
            .iter_mut()
            .find(|saved| saved.id == id && saved.active)
            .ok_or_else(|| selection::SelectionError::NotFound(id.to_owned()))?;
        if current.volume_identity != saved.volume_identity
            || !same_path(&current.source, &saved.source)
        {
            return Err(selection::SelectionError::Source(SourceError::Replaced).into());
        }
        if saved.source == current.source && saved.root_kind == current.kind {
            return Ok(false);
        }
        saved.source = current.source.clone();
        saved.root_kind = current.kind;
        let generation = settings.acknowledged_generation;
        selection::restage_after_ack(&mut settings, generation)?;
        self.save_unlocked(&settings)?;
        Ok(true)
    }
}
