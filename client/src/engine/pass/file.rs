use drip_core::{
    Blake3Digest,
    protocol::{Entry, EntryKind},
};

use super::{PassContext, PassOptions, upload_events};
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
        options: PassOptions,
        operation: &str,
    ) -> Result<(), EngineError> {
        let source = SourceFile::open(&entry.local_path)?;
        if source.protocol_path() != &entry.source_path
            || source.volume_identity() != saved.volume_identity
        {
            return Err(crate::source::SourceError::Replaced.into());
        }
        source.verify().await?;
        let lookup = self.cache.get(saved, &source, options.audit).await;
        #[cfg(test)]
        if lookup.digest.is_some() {
            mutate_cached_source_for_test(&source);
        }
        if let Some(warning) = lookup.warning {
            let _ = self
                .events
                .emit(EventKind::Error, None, None, "local-cache", warning)
                .await;
        }
        let cached = lookup.digest;
        if let Some(digest) = cached
            && matches_remote(remote, &source, digest)
        {
            let verification = source.verify().await;
            #[cfg(test)]
            if verification.is_err()
                && test_path_for_source("DRIP_TEST_MUTATE_AFTER_CACHE_LOOKUP", &source).is_some()
            {
                let metrics = std::path::PathBuf::from(
                    std::env::var_os("DRIP_SOURCE_READ_METRICS_PATH")
                        .expect("source read metrics path is required"),
                );
                crate::engine::test_metrics::write(&metrics).unwrap();
                std::process::exit(85);
            }
            verification?;
            return self
                .emit(
                    EventKind::FileChecked,
                    active,
                    &entry.source_path,
                    operation,
                    "Source observations and saved fingerprint match the current mirror",
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
                    options.expected_entry,
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
        if cached.is_none() {
            let warning = self
                .cache
                .remember(saved, &entry.source_path, observation, &volume, digest)
                .await;
            #[cfg(test)]
            if warning.is_none() && exit_after_cache_commit_for_test(&entry.source_path) {
                std::process::exit(86);
            }
            if let Some(warning) = warning {
                let _ = self
                    .events
                    .emit(EventKind::Error, None, None, "local-cache", warning)
                    .await;
            }
        }
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

#[cfg(test)]
fn mutate_cached_source_for_test(source: &SourceFile) {
    let Some(path) = test_path_for_source("DRIP_TEST_MUTATE_AFTER_CACHE_LOOKUP", source) else {
        return;
    };
    let size = usize::try_from(source.size()).unwrap();
    std::fs::write(path, vec![0xa5; size]).unwrap();
}

#[cfg(test)]
fn test_path_for_source(name: &str, source: &SourceFile) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var_os(name)?);
    drip_core::SourcePath::try_from_path(&path)
        .is_ok_and(|path| path == *source.protocol_path())
        .then_some(path)
}

#[cfg(test)]
fn exit_after_cache_commit_for_test(path: &drip_core::SourcePath) -> bool {
    std::env::var_os("DRIP_TEST_EXIT_AFTER_CACHE_COMMIT").is_some_and(|target| {
        drip_core::SourcePath::try_from_path(std::path::Path::new(&target))
            .is_ok_and(|target| target == *path)
    })
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
