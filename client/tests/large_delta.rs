#![cfg(windows)]

#[path = "large_delta/support.rs"]
mod support;

use drip_client::{ActivityEvent, Engine, UploadPhase};
use tokio::sync::mpsc;

#[tokio::test]
#[ignore = "requires the owned synthetic HTTPS fixture and 10 GiB free source space"]
async fn large_delta() {
    let fixture = support::Fixture::prepare();
    let traffic_before = fixture.traffic();
    let engine = Engine::beside_executable().unwrap();
    let token = fixture.token();
    let pairing = engine
        .pair(&fixture.server, "Phase 03 large_delta", &token)
        .await
        .unwrap();
    let expected = fixture.generate(false);
    let (baseline, baseline_payload, baseline_phases) = upload(&engine, &fixture).await;
    assert_eq!(baseline.digest, expected);
    assert_eq!(baseline_payload, fixture.size);
    fixture.require_mirror(&pairing, expected, fixture.size);
    let traffic_baseline = fixture.traffic();
    assert_eq!(traffic_baseline.0 - traffic_before.0, baseline_payload);

    let expected = fixture.generate(true);
    let (delta, payload, delta_phases) = upload(&engine, &fixture).await;
    assert_eq!(delta.digest, expected);
    assert_eq!(delta.size, fixture.size + 2);
    assert!(payload <= 8 * 1024 * 1024, "delta payload: {payload}");
    fixture.require_mirror(&pairing, expected, fixture.size + 2);
    let traffic_delta = fixture.traffic();
    let measured_payload = traffic_delta.0 - traffic_baseline.0;
    assert_eq!(measured_payload, payload);
    assert!(measured_payload <= 8 * 1024 * 1024);
    let memory = fixture.memory();
    println!(
        "LARGE_DELTA {}",
        serde_json::json!({
            "size": fixture.size, "delta_size": delta.size,
            "generator": support::GENERATOR, "seed": support::SEED,
            "baseline_file_payload": baseline_payload, "delta_file_payload": payload,
            "baseline_phases": baseline_phases, "delta_phases": delta_phases,
            "source_blake3": expected.to_hex(), "mirror_blake3": expected.to_hex(),
            "pc_id": pairing.pc_id(), "mirror_directory": pairing.mirror_directory(),
            "baseline_metadata_bytes": traffic_baseline.1 - traffic_before.1,
            "delta_metadata_bytes": traffic_delta.1 - traffic_baseline.1,
            "metadata_definition": "HTTP headers and decoded JSON request/response bytes; excludes TLS and transfer framing",
            "memory": memory,
        })
    );
}

async fn upload(
    engine: &Engine,
    fixture: &support::Fixture,
) -> (
    drip_client::UploadResult,
    u64,
    std::collections::BTreeSet<String>,
) {
    let (sender, mut receiver) = mpsc::channel(16);
    let reporting = tokio::spawn(async move {
        let mut sent = 0;
        let mut phases = std::collections::BTreeSet::new();
        let mut latest = None;
        let mut completed = 0;
        while let Some(event) = receiver.recv().await {
            match event {
                ActivityEvent::Progress(snapshot) => {
                    phases.insert(format!("{:?}", snapshot.phase));
                    if snapshot.phase == UploadPhase::Uploading {
                        sent = sent.max(snapshot.processed_bytes);
                    }
                    latest = Some(snapshot);
                }
                ActivityEvent::UploadCompleted { .. } => completed += 1,
                _ => {}
            }
        }
        (sent, phases, latest.unwrap(), completed)
    });
    let result = engine.upload(&fixture.source, &sender).await.unwrap();
    drop(sender);
    let (sent, phases, latest, completed) = reporting.await.unwrap();
    assert_eq!(completed, 1);
    assert_eq!(latest.source_hashed_bytes, result.size);
    assert_eq!(latest.source_chunked_bytes, result.size);
    assert!(latest.chunk_count > 0);
    assert_eq!(latest.planned_payload_bytes, sent);
    assert_eq!(latest.file_payload_sent, sent);
    for phase in ["Hashing", "Chunking", "Uploading", "Verifying"] {
        assert!(phases.contains(phase), "missing actual phase {phase}");
    }
    (result, sent, phases)
}
