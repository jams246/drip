use crate::{
    engine::{
        EngineError, EventKind, mirror_api, saved, selection::ActiveSelection, watcher::ChangeEpoch,
    },
    settings::SavedSelection,
};

use super::PassContext;

impl PassContext<'_> {
    pub(super) async fn delete_root(
        &self,
        active: &ActiveSelection,
        source: &SavedSelection,
        operation: &str,
        epoch: ChangeEpoch,
    ) -> Result<bool, EngineError> {
        self.delete_extras(active, source, None, epoch).await?;
        if !mirror_api::inventory(self.transport, self.pairing.credential(), active)
            .await?
            .is_empty()
        {
            return Err(EngineError::SelectionChanged);
        }
        self.check_deletion(active, source, &source.source, epoch, true)?;
        if !self.store.remove_deleted_root(source, active.generation)? {
            return Err(EngineError::SelectionChanged);
        }
        self.cache.forget(&source.id);
        self.signal.refresh();
        saved::synchronize(self.store, self.transport, self.pairing, true).await?;
        self.emit(
            EventKind::SelectionChanged,
            active,
            &source.source,
            operation,
            "source deletion confirmed on server; watch removed",
        )
        .await?;
        Ok(true)
    }
}
