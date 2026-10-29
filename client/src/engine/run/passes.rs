use crate::engine::{
    Engine, EngineError, EventKind,
    pass::{PassContext, auth_failure},
};

use super::setup::PassState;

impl Engine {
    pub(super) async fn continuous_pass(
        &self,
        context: &PassContext<'_>,
        state: &mut PassState,
        barrier: bool,
        all: bool,
        force_hash: bool,
    ) -> Result<bool, EngineError> {
        let (active, latest) = state.prepare(self, context, barrier).await?;
        let batch = context.signal.take();
        if force_hash || batch.full {
            context.cache.clear();
        }
        for id in batch
            .uncertain
            .iter()
            .chain(state.watcher.iter().flat_map(|watcher| &watcher.failed))
        {
            context.cache.forget(id);
        }
        let active: Vec<_> = active
            .into_iter()
            .filter(|selection| {
                all || batch.full || batch.selections.contains(&selection.selection.id)
            })
            .collect();
        let mut successful = true;
        for (index, selection) in active.iter().enumerate() {
            let saved = latest
                .selections
                .iter()
                .find(|saved| saved.id == selection.selection.id)
                .ok_or(EngineError::SelectionChanged)?;
            let operation = crate::source::random_id()?;
            match context.root(selection, saved, &operation).await {
                Ok(true) => {}
                Ok(false) => {
                    context.cache.forget(&saved.id);
                    context.signal.retry(&saved.id);
                    successful = false;
                }
                Err(error)
                    if matches!(
                        error,
                        EngineError::Cancelled | EngineError::SelectionChanged
                    ) || auth_failure(&error) =>
                {
                    for pending in &active[index..] {
                        context.cache.forget(&pending.selection.id);
                        context.signal.retry(&pending.selection.id);
                    }
                    return Err(error);
                }
                Err(error) => {
                    context.cache.forget(&saved.id);
                    context.events.reachability().observe_error(&error);
                    context.signal.retry(&saved.id);
                    successful = false;
                    let kind = if matches!(error, EngineError::Source(_)) {
                        EventKind::UnavailableRoot
                    } else {
                        EventKind::Error
                    };
                    context
                        .events
                        .emit(
                            kind,
                            Some(&saved.id),
                            Some(&saved.source),
                            &operation,
                            error.to_string(),
                        )
                        .await?;
                }
            }
        }
        Ok(successful)
    }
}
