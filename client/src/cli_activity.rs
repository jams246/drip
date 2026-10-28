use drip_client::{ActivityEvent, UploadPhase};
use tokio::sync::mpsc;

pub async fn report_progress(mut events: mpsc::Receiver<ActivityEvent>) {
    let mut previous = None;
    while let Some(event) = events.recv().await {
        match event {
            ActivityEvent::Progress(snapshot) => {
                let name = match snapshot.phase {
                    UploadPhase::Hashing => "Hashing",
                    UploadPhase::Chunking => "Chunking / hashing",
                    UploadPhase::Uploading => "Uploading",
                    UploadPhase::Verifying => "Verifying",
                };
                let stage_changed = previous != Some(snapshot.phase);
                if stage_changed || snapshot.processed_bytes % (1024 * 1024) == 0 {
                    let total = snapshot
                        .total_bytes
                        .map_or_else(|| "unknown".to_owned(), |total| total.to_string());
                    eprintln!("{name}: {} of {total} bytes.", snapshot.processed_bytes);
                }
                previous = Some(snapshot.phase);
            }
            ActivityEvent::ControlRetry {
                source_path,
                attempt,
                max_attempts,
                status,
                recovered,
            } => {
                let status = status.map_or_else(String::new, |status| format!("HTTP {status}; "));
                let outcome = if recovered { "recovered" } else { "retrying" };
                eprintln!(
                    "{}: {status}upload plan {outcome} {attempt} of {max_attempts}.",
                    source_path.to_windows_path()
                );
            }
            ActivityEvent::UploadStarted { .. } | ActivityEvent::UploadCompleted { .. } => {}
        }
    }
}
