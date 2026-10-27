#![cfg(windows)]

#[expect(
    dead_code,
    reason = "shared fixture exposes helpers used by the large-delta test"
)]
#[path = "large_delta/support.rs"]
mod support;

use std::{fs, path::PathBuf};

use drip_client::{ActivityEvent, Engine, UploadPhase};
use drip_core::CHUNK_MAX_SIZE;
use tokio::sync::mpsc;

#[tokio::test]
#[ignore = "requires the owned synthetic HTTPS fixture and 64 MiB free source space"]
async fn accepted_chunk_response_loss_retries_and_counts_each_payload() {
    let fixture = support::Fixture::prepare();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/phase-03");
    let control_path = root.join("proxy-control.json");
    let mut control: serde_json::Value =
        serde_json::from_slice(&fs::read(&control_path).unwrap()).unwrap();
    control["action"] = "drop_chunk_200".into();
    fs::write(&control_path, serde_json::to_vec(&control).unwrap()).unwrap();

    let payload_before = fixture.traffic().0;
    let engine = Engine::beside_executable().unwrap();
    let token = fixture.token();
    engine
        .pair(&fixture.server, "Phase 03 chunk retry", &token)
        .await
        .unwrap();
    let expected = fixture.generate(false);
    let (sender, mut receiver) = mpsc::channel(16);
    let reporting = tokio::spawn(async move {
        let mut completed = 0;
        let mut final_upload = None;
        let mut verifying = None;
        while let Some(event) = receiver.recv().await {
            match event {
                ActivityEvent::Progress(snapshot) => match snapshot.phase {
                    UploadPhase::Uploading => final_upload = Some(snapshot),
                    UploadPhase::Verifying => verifying = Some(snapshot),
                    _ => {}
                },
                ActivityEvent::UploadCompleted { .. } => completed += 1,
                ActivityEvent::UploadStarted { .. } => {}
            }
        }
        (completed, final_upload, verifying)
    });

    let result = engine.upload(&fixture.source, &sender).await.unwrap();
    drop(sender);
    let (completed, final_upload, verifying) = reporting.await.unwrap();
    let payload = fixture.traffic().0 - payload_before;
    let final_upload = final_upload.expect("final upload progress missing");
    let verifying = verifying.expect("verification phase missing");

    assert_eq!(result.digest, expected);
    assert_eq!(payload, verifying.file_payload_sent);
    assert_eq!(payload, final_upload.file_payload_sent);
    assert_eq!(payload, final_upload.processed_bytes);
    assert_eq!(final_upload.total_bytes, Some(payload));
    assert_eq!(verifying.planned_payload_bytes, fixture.size);
    assert!(payload > fixture.size);
    assert!(payload <= fixture.size + u64::from(CHUNK_MAX_SIZE));
    assert_eq!(completed, 1);
}
