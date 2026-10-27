use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

use axum::body::{Body, Bytes};
use drip_core::{CHUNK_MIN_SIZE, SourcePath, protocol::UploadState};
use futures_util::{StreamExt, stream};

use super::support::{Fixture, descriptors, plan, prepare, pseudo_random};

#[tokio::test]
async fn progress_on_each_delta_frame_prevents_idle_expiry() {
    let clock = Arc::new(AtomicI64::new(100));
    let fixture = Fixture::with_clock(Arc::clone(&clock));
    let device = fixture.enroll("Delta frame progress");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["progress.bin".to_owned()]).unwrap();
    let selection_id = "f".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let bytes = pseudo_random(CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &bytes,
    );
    let chunks = descriptors(&bytes);
    assert_eq!(chunks.len(), 1);
    let page = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks))
        .unwrap();
    assert_eq!(page.missing_ordinals, [0]);

    let split = bytes.len() / 2;
    let between_frames = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&between_frames);
    let second_clock = Arc::clone(&clock);
    let store = fixture.store.clone();
    let body_credential = credential.to_owned();
    let upload_id = prepared.upload_id.clone();
    let body = Body::from_stream(
        stream::iter([
            Ok::<_, Infallible>(Bytes::copy_from_slice(&bytes[..split])),
            Ok(Bytes::copy_from_slice(&bytes[split..])),
        ])
        .enumerate()
        .map(move |(index, frame)| {
            if index == 0 {
                clock.store(1800, Ordering::SeqCst);
            } else {
                second_clock.store(3500, Ordering::SeqCst);
                observed.store(
                    store
                        .get_upload_state(&body_credential, &upload_id)
                        .is_ok_and(|state| state.state == UploadState::Active),
                    Ordering::SeqCst,
                );
            }
            frame
        }),
    );
    fixture
        .store
        .put_upload_chunk(
            device.device_credential.expose_secret(),
            &prepared.upload_id,
            0,
            body,
        )
        .await
        .unwrap();
    assert!(between_frames.load(Ordering::SeqCst));

    fixture
        .store
        .commit_upload(
            device.device_credential.expose_secret(),
            &prepared.upload_id,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(fixture.mirror_path(&device, &source)).unwrap(),
        bytes
    );
}

#[tokio::test]
async fn expired_delta_frame_removes_stage_and_plan_before_fresh_prepare() {
    let clock = Arc::new(AtomicI64::new(100));
    let fixture = Fixture::with_clock(Arc::clone(&clock));
    let device = fixture.enroll("Expired delta frame");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["expired.bin".to_owned()]).unwrap();
    let selection_id = "e".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let bytes = pseudo_random(CHUNK_MIN_SIZE as usize);
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source.clone(),
        &bytes,
    );
    let chunks = descriptors(&bytes);
    let page = fixture
        .store
        .plan_upload(credential, &prepared.upload_id, plan(chunks))
        .unwrap();
    assert_eq!(page.missing_ordinals, [0]);
    let stage_path = fixture
        .root
        .join("mirror/.drip-staging")
        .join(format!("{}.part", prepared.upload_id));
    assert!(stage_path.exists());

    let split = bytes.len() / 2;
    let second_clock = Arc::clone(&clock);
    let body = Body::from_stream(
        stream::iter([
            Ok::<_, Infallible>(Bytes::copy_from_slice(&bytes[..split])),
            Ok(Bytes::copy_from_slice(&bytes[split..])),
        ])
        .enumerate()
        .map(move |(index, frame)| {
            if index == 1 {
                second_clock.store(1900, Ordering::SeqCst);
            }
            frame
        }),
    );
    assert!(matches!(
        fixture
            .store
            .put_upload_chunk(credential, &prepared.upload_id, 0, body)
            .await,
        Err(drip_server::StoreError::NotFound)
    ));
    assert!(!stage_path.exists());

    let connection = rusqlite::Connection::open(fixture.root.join("state/drip.sqlite3")).unwrap();
    let counts = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM upload_plan_pages WHERE upload_id = ?1), \
                    (SELECT COUNT(*) FROM upload_chunks WHERE upload_id = ?1)",
            [&prepared.upload_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0));

    let replacement = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source,
        &bytes,
    );
    assert_ne!(replacement.upload_id, prepared.upload_id);
    assert_eq!(
        fixture
            .store
            .get_upload_state(credential, &replacement.upload_id)
            .unwrap()
            .state,
        UploadState::Active
    );
}
