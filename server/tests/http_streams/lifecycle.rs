use super::*;

fn prepared() -> (
    Fixture,
    drip_core::protocol::EnrollmentResponse,
    drip_core::protocol::UploadPrepareResponse,
) {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Stream duration");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["duration.bin".to_owned()]).unwrap();
    let selection = "a".repeat(32);
    let generation = select(&fixture.store, credential, &selection, source.clone());
    let upload = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &selection,
                generation,
                source,
                b"payload",
                Timestamp::new(0, 0).unwrap(),
            ),
        )
        .unwrap();
    (fixture, device, upload)
}

#[tokio::test(start_paused = true)]
async fn valid_stream_can_complete_after_the_control_body_deadline() {
    let (fixture, device, upload) = prepared();
    let credential = device.device_credential.expose_secret();
    let (started, mut ready) = mpsc::unbounded_channel();
    let (sender, receiver) = mpsc::channel(1);
    let task = tokio::spawn(
        drip_server::router(fixture.store.clone()).oneshot(
            Request::put(format!("/v1/uploads/{}/data", upload.upload_id))
                .header("authorization", format!("Bearer {credential}"))
                .body(body(started, receiver))
                .unwrap(),
        ),
    );
    ready.recv().await.unwrap();
    tokio::time::advance(Duration::from_secs(20)).await;
    sender.send(Bytes::from_static(b"ayload")).await.unwrap();
    drop(sender);
    assert_eq!(task.await.unwrap().unwrap().status(), StatusCode::OK);
    let committed = fixture
        .store
        .commit_upload(credential, &upload.upload_id)
        .unwrap();
    assert_eq!(committed.entry.size, 7);
    assert_eq!(
        committed.entry.digest,
        Some(Blake3Digest::new(*blake3::hash(b"payload").as_bytes()))
    );
}

#[tokio::test(start_paused = true)]
async fn stalled_stream_still_expires_at_the_idle_deadline() {
    let (fixture, device, upload) = prepared();
    let credential = device.device_credential.expose_secret();
    let (started, mut ready) = mpsc::unbounded_channel();
    let (_sender, receiver) = mpsc::channel(1);
    let task = tokio::spawn(
        drip_server::router(fixture.store.clone()).oneshot(
            Request::put(format!("/v1/uploads/{}/data", upload.upload_id))
                .header("authorization", format!("Bearer {credential}"))
                .body(body(started, receiver))
                .unwrap(),
        ),
    );
    ready.recv().await.unwrap();
    tokio::time::advance(Duration::from_secs(30 * 60)).await;
    assert_eq!(task.await.unwrap().unwrap().status(), StatusCode::NOT_FOUND);
    assert!(
        !fixture
            ._root
            .join("mirror/.drip-staging")
            .join(format!("{}.part", upload.upload_id))
            .exists()
    );
}
