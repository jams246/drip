use drip_core::protocol::SelectionUpdate;
use drip_core::{CHUNK_MIN_SIZE, SourcePath};
use drip_server::StoreError;

use super::support::{
    Fixture, descriptors, plan, prepare, pseudo_random, send_missing, upload_legacy,
};

#[tokio::test]
async fn stale_base_revision_cannot_replace_external_mirror_changes() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Stale revision");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["stale-revision.bin".to_owned()]).unwrap();
    let selection_id = "1".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let baseline = b"verified baseline";
    upload_legacy(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        baseline,
    )
    .await;
    let replacement = pseudo_random(CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &replacement,
    );
    let chunks = descriptors(&replacement);
    let page = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks.clone()))
        .unwrap();
    send_missing(
        &fixture.store,
        credential,
        &prepared.upload_id,
        &replacement,
        &chunks,
        &page,
    )
    .await;
    let external = b"external mirror edit";
    std::fs::write(fixture.mirror_path(&device, &source), external).unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        external
    );
}

#[tokio::test]
async fn stale_selection_generation_cannot_publish_planned_chunks() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Stale generation");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["stale-generation.bin".to_owned()]).unwrap();
    let selection_id = "2".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let baseline = b"old file";
    upload_legacy(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        baseline,
    )
    .await;
    let replacement = pseudo_random(CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &replacement,
    );
    let chunks = descriptors(&replacement);
    let page = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks.clone()))
        .unwrap();
    send_missing(
        &fixture.store,
        credential,
        &prepared.upload_id,
        &replacement,
        &chunks,
        &page,
    )
    .await;
    let selections = fixture.store.get_selections(credential).unwrap();
    fixture
        .store
        .update_selections(
            credential,
            SelectionUpdate {
                expected_generation: selections.generation,
                selections: selections.selections,
            },
        )
        .unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        baseline
    );
}

#[tokio::test]
async fn corrupted_staging_is_rejected_without_replacing_baseline() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Corrupt staging");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["staging.bin".to_owned()]).unwrap();
    let selection_id = "3".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let baseline = b"last good bytes";
    upload_legacy(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        baseline,
    )
    .await;
    let replacement = pseudo_random(CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &replacement,
    );
    let chunks = descriptors(&replacement);
    let page = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks.clone()))
        .unwrap();
    send_missing(
        &fixture.store,
        credential,
        &prepared.upload_id,
        &replacement,
        &chunks,
        &page,
    )
    .await;
    std::fs::write(
        fixture
            .root
            .join("mirror/.drip-staging")
            .join(format!("{}.part", prepared.upload_id)),
        b"corrupt staged bytes",
    )
    .unwrap();
    assert!(matches!(
        fixture.store.commit_upload(credential, &prepared.upload_id),
        Err(StoreError::CorruptContent)
    ));
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        baseline
    );
}
