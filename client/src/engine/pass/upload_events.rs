use std::time::{Duration, Instant};

use tokio::{sync::mpsc, task::JoinHandle};

use crate::engine::{ActivityEvent, EngineError, EventKind, EventSink};

const REPORT_INTERVAL: Duration = Duration::from_millis(200);

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
        let mut phase = None;
        let mut pending = None;
        let mut last_report = None;
        let timer = tokio::time::sleep(REPORT_INTERVAL);
        tokio::pin!(timer);
        loop {
            tokio::select! {
                event = receiver.recv() => match event {
                    Some(ActivityEvent::Progress(progress)) => {
                        let now = Instant::now();
                        if phase != Some(progress.phase) {
                            phase = Some(progress.phase);
                            pending = None;
                            events.progress(&selection, &operation, progress.clone());
                            events.progress_phase(&selection, &operation, progress).await?;
                            last_report = Some(now);
                            timer
                                .as_mut()
                                .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                        } else if last_report.is_none_or(|last| now.duration_since(last) >= REPORT_INTERVAL) {
                            events.progress(&selection, &operation, progress);
                            last_report = Some(now);
                            timer
                                .as_mut()
                                .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                        } else {
                            pending = Some(progress);
                        }
                    }
                    Some(ActivityEvent::UploadStarted { source_path, size }) => {
                        events.emit(
                            EventKind::UploadStarted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("uploading {size} bytes"),
                        ).await?;
                    }
                    Some(ActivityEvent::UploadCompleted { source_path, size, .. }) => {
                        events.emit(
                            EventKind::UploadCompleted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("verified publication of {size} bytes"),
                        ).await?;
                    }
                    Some(ActivityEvent::ControlRetry { source_path, attempt, max_attempts, status, recovered }) => {
                        let message = match (recovered, status) {
                            (true, Some(status)) => format!("HTTP {status}; upload plan recovered on attempt {attempt} of {max_attempts}"),
                            (false, Some(status)) => format!("HTTP {status}; upload plan retry {attempt} of {max_attempts}"),
                            (true, None) => format!("Upload plan recovered on attempt {attempt} of {max_attempts}"),
                            (false, None) => format!("Upload plan retry {attempt} of {max_attempts}"),
                        };
                        events.emit(
                            EventKind::ControlRetry,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            message,
                        ).await?;
                    }
                    None => {
                        if let Some(progress) = pending.take() {
                            timer.as_mut().await;
                            events.progress(&selection, &operation, progress);
                        }
                        break;
                    }
                },
                _ = &mut timer, if pending.is_some() => {
                    if let Some(progress) = pending.take() {
                        events.progress(&selection, &operation, progress);
                        last_report = Some(Instant::now());
                        timer
                            .as_mut()
                            .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                    }
                }
            }
        }
        Ok(())
    });
    (sender, report)
}

#[cfg(test)]
#[path = "upload_events/tests.rs"]
mod tests;
