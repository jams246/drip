use super::PassContext;
use crate::engine::{EngineError, selection::ActiveSelection};
use crate::{
    settings::SavedSelection,
    source::{ScannedEntry, capture_root, validate_root},
};

impl PassContext<'_> {
    pub fn check(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
    ) -> Result<(), EngineError> {
        self.check_selection(active, saved)?;
        validate_root(saved)?;
        Ok(())
    }

    pub(super) fn check_selection(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
    ) -> Result<(), EngineError> {
        let control = self.control.borrow();
        if control.paused || control.stop {
            return Err(EngineError::Cancelled);
        }
        drop(control);
        let settings = self.store.load()?;
        if settings.pending_update.is_some()
            || settings.acknowledged_generation != active.generation
            || !settings
                .selections
                .iter()
                .any(|item| item == saved && item.active)
        {
            return Err(EngineError::SelectionChanged);
        }
        Ok(())
    }

    pub fn fresh_kind(
        &self,
        saved: &SavedSelection,
        entry: &ScannedEntry,
    ) -> Result<(), EngineError> {
        let current = capture_root(&entry.local_path)?;
        if current.source != entry.source_path
            || current.kind != entry.kind
            || current.volume_identity != saved.volume_identity
        {
            return Err(crate::source::SourceError::Changed.into());
        }
        Ok(())
    }
}
