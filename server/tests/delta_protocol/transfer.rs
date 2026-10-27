use super::support::{Fixture, descriptors, digest, pseudo_random, run_delta, upload_legacy};
use drip_core::SourcePath;

#[tokio::test]
async fn midpoint_crlf_delta_reconstructs_32_mib_and_sends_at_most_8_mib() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Large delta");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["large.bin".to_owned()]).unwrap();
    let selection_id = "a".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let baseline = pseudo_random(32 * 1024 * 1024);
    let mirror = fixture.mirror_path(&device, &source);
    upload_legacy(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &baseline,
    )
    .await;
    assert_eq!(std::fs::read(&mirror).unwrap(), baseline);

    let (unchanged, unchanged_json_sent, unchanged_json_received) = run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        &baseline,
        &mirror,
    )
    .await;
    assert_eq!(unchanged, 0);
    assert!(unchanged_json_sent > 0);
    assert!(unchanged_json_received > 0);

    let mut midpoint = baseline.clone();
    let midpoint_offset = midpoint.len() / 2;
    midpoint.splice(midpoint_offset..midpoint_offset, *b"\r\n");
    let (midpoint_sent, midpoint_json_sent, midpoint_json_received) = run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        &midpoint,
        &mirror,
    )
    .await;
    assert!(midpoint_sent <= 8 * 1024 * 1024);
    assert!(midpoint_json_sent > 0);
    assert!(midpoint_json_received > 0);
    assert_eq!(digest(&std::fs::read(&mirror).unwrap()), digest(&midpoint));

    let mut beginning = midpoint.clone();
    beginning.splice(1..1, *b"\r\n");
    run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        &beginning,
        &mirror,
    )
    .await;

    let mut end = beginning.clone();
    end.extend_from_slice(b"\r\n");
    run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        &end,
        &mirror,
    )
    .await;

    let truncated = &end[..end.len() - 32_768];
    run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        truncated,
        &mirror,
    )
    .await;
}

#[tokio::test]
async fn repeated_content_ranges_reuse_the_same_baseline_file() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Repeated content");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["repeat.bin".to_owned()]).unwrap();
    let selection_id = "b".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let baseline = vec![0x5a; 1024 * 1024];
    let repeated = vec![0x5a; 3 * 1024 * 1024];
    let mirror = fixture.mirror_path(&device, &source);
    upload_legacy(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &baseline,
    )
    .await;
    let chunks = descriptors(&repeated);
    assert!(chunks.len() > 1);
    assert!(
        chunks
            .iter()
            .map(|chunk| chunk.digest)
            .all(|digest| digest == chunks[0].digest)
    );
    let (sent, _, _) = run_delta(
        &fixture,
        credential,
        &selection_id,
        generation,
        &source,
        &repeated,
        &mirror,
    )
    .await;
    assert_eq!(sent, 0);
}
