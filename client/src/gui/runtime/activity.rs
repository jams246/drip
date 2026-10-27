use chrono::{DateTime, Local};
use drip_client::{LifecycleEvent, SelectionProgress};
use tokio::sync::mpsc;

use super::Event;
use crate::gui::model::LogRow;

pub(super) fn row(event: LifecycleEvent) -> LogRow {
    let local_time =
        DateTime::from_timestamp(event.timestamp.unix_seconds(), event.timestamp.nanos())
            .map(|time| {
                time.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S%.3f %:z")
                    .to_string()
            })
            .unwrap_or_else(|| {
                format!(
                    "Unix {}.{:09} +00:00",
                    event.timestamp.unix_seconds(),
                    event.timestamp.nanos()
                )
            });
    LogRow { event, local_time }
}

pub(super) async fn forward(
    mut receiver: mpsc::Receiver<LifecycleEvent>,
    generation: u64,
    events: mpsc::Sender<Event>,
) {
    while let Some(mut event) = receiver.recv().await {
        let message = if let Some(file) = event.progress.take() {
            let (Some(selection_id), operation_id) = (event.selection_id, event.operation_id)
            else {
                continue;
            };
            Event::Progress {
                generation,
                progress: Some(Box::new(SelectionProgress {
                    selection_id,
                    operation_id,
                    source_bytes_processed: file.source_hashed_bytes,
                    uploaded_file_bytes: file.file_payload_sent,
                    file,
                })),
            }
        } else {
            Event::Activity(Box::new(row(event)))
        };
        if events.send(message).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip_client::EventKind;

    #[test]
    fn local_log_time_contains_an_explicit_offset_without_changing_utc_storage() {
        let event = LifecycleEvent {
            timestamp: drip_core::Timestamp::new(1_700_000_000, 123_000_000).unwrap(),
            kind: EventKind::UploadCompleted,
            selection_id: None,
            source_path: None,
            operation_id: "synthetic".into(),
            message: "verified".into(),
            progress: None,
        };
        let row = row(event);
        assert_eq!(row.event.timestamp.unix_seconds(), 1_700_000_000);
        let offset = &row.local_time[row.local_time.len() - 6..];
        assert!(offset.starts_with(['+', '-']));
        assert_eq!(&offset[3..4], ":");
    }
}
