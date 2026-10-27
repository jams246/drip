use tokio::{sync::mpsc, task::JoinHandle};

use crate::engine::{ActivityEvent, EngineError, EventKind, EventSink};

pub(super) fn start(
    events: EventSink,
    selection: String,
    operation: String,
) -> (
    mpsc::Sender<ActivityEvent>,
    JoinHandle<Result<(), EngineError>>,
) {
    let (sender, mut receiver) = mpsc::channel(16);
    let report = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                ActivityEvent::Progress(progress) => {
                    events.progress(&selection, &operation, progress)
                }
                ActivityEvent::UploadStarted { source_path, size } => {
                    events
                        .emit(
                            EventKind::UploadStarted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("uploading {size} bytes"),
                        )
                        .await?;
                }
                ActivityEvent::UploadCompleted {
                    source_path, size, ..
                } => {
                    events
                        .emit(
                            EventKind::UploadCompleted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("verified publication of {size} bytes"),
                        )
                        .await?;
                }
            }
        }
        Ok(())
    });
    (sender, report)
}
