use drip_core::{
    Blake3Digest,
    protocol::{Entry, EntryKind, EntryPrecondition},
};

use super::{PassContext, upload_events};
use crate::engine::{
    ActivityEvent, EngineError, EventKind, ProgressSnapshot, UploadPhase,
    selection::ActiveSelection, upload,
};
use crate::{
    settings::SavedSelection,
    source::{ScannedEntry, SourceFile},
};

impl PassContext<'_> {
    pub(super) async fn file(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        entry: &ScannedEntry,
        remote: Option<&Entry>,
        expected_entry: Option<EntryPrecondition>,
        operation: &str,
    ) -> Result<(), EngineError> {
        let source = SourceFile::open(&entry.local_path)?;
        if source.protocol_path() != &entry.source_path
            || source.volume_identity() != saved.volume_identity
        {
            return Err(crate::source::SourceError::Replaced.into());
        }
        source.verify().await?;
        let cached = self.cache.get(&saved.id, &source);
        if let Some(digest) = cached
            && matches_remote(remote, &source, digest)
        {
            source.verify().await?;
            return self
                .emit(
                    EventKind::FileChecked,
                    active,
                    &entry.source_path,
                    operation,
                    "verified source observations and digest match the actual mirror",
                )
                .await;
        }
        let observation = source.observation();
        let volume = source.volume_identity().to_owned();
        let (sender, report) = upload_events::start(
            self.events.clone(),
            active.selection.id.clone(),
            operation.to_owned(),
        );
        let work = async {
            let digest = match cached {
                Some(digest) => digest,
                None => {
                    let _ = sender
                        .send(ActivityEvent::Progress(ProgressSnapshot {
                            phase: UploadPhase::Hashing,
                            source_path: entry.source_path.clone(),
                            processed_bytes: 0,
                            total_bytes: Some(source.size()),
                            source_hashed_bytes: 0,
                            source_chunked_bytes: 0,
                            chunk_count: 0,
                            planned_payload_bytes: 0,
                            file_payload_sent: 0,
                        }))
                        .await;
                    source.hash(&sender).await?
                }
            };
            if matches_remote(remote, &source, digest) {
                source.verify().await?;
                Ok((true, digest))
            } else {
                self.check(active, saved)?;
                self.fresh_kind(saved, entry)?;
                upload::upload_selected(
                    self.transport,
                    self.pairing,
                    source,
                    active,
                    expected_entry,
                    Some(digest),
                    &sender,
                )
                .await
                .map(|_| (false, digest))
            }
        };
        let result = tokio::select! {
            result = work => result,
            () = cancelled(self.control.clone()) => Err(EngineError::Cancelled),
        };
        drop(sender);
        report
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))??;
        let (checked, digest) = result?;
        self.cache
            .remember(&saved.id, &entry.source_path, observation, &volume, digest);
        if checked {
            self.emit(
                EventKind::FileChecked,
                active,
                &entry.source_path,
                operation,
                "source hash and metadata match the actual mirror",
            )
            .await?;
        }
        Ok(())
    }
}

fn matches_remote(remote: Option<&Entry>, source: &SourceFile, digest: Blake3Digest) -> bool {
    remote.is_some_and(|remote| {
        remote.kind == EntryKind::File
            && remote.size == source.size()
            && remote.digest == Some(digest)
            && remote.last_write == Some(source.last_write())
    })
}

async fn cancelled(mut control: tokio::sync::watch::Receiver<crate::engine::RunControl>) {
    loop {
        let current = *control.borrow_and_update();
        if current.paused || current.stop || control.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cancelled;
    use crate::engine::RunControl;
    use tokio::sync::watch;

    #[tokio::test]
    async fn rescan_does_not_cancel_inflight_work_but_pause_does() {
        let (commands, control) = watch::channel(RunControl::default());
        let pending = tokio::spawn(cancelled(control));
        commands.send_modify(|current| current.reconcile += 1);
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        commands.send_modify(|current| current.paused = true);
        tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stop_cancels_inflight_work() {
        let (commands, control) = watch::channel(RunControl::default());
        let pending = tokio::spawn(cancelled(control));
        commands.send_modify(|current| current.stop = true);
        tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
    }
}
