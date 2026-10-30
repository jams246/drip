mod common;
#[path = "http_streams/lifecycle.rs"]
mod lifecycle;

use std::{convert::Infallible, process::Command, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use common::{Fixture, enroll, request, select};
use drip_core::{
    Blake3Digest, ChunkConfig, ChunkDescriptor, SourcePath, Timestamp, protocol::UploadPlanRequest,
};
use tokio::sync::mpsc;
use tower::ServiceExt;

fn body(ready: mpsc::UnboundedSender<()>, mut incoming: mpsc::Receiver<Bytes>) -> Body {
    let mut first = true;
    let mut announced = false;
    Body::from_stream(futures_util::stream::poll_fn(move |cx| {
        if first {
            first = false;
            return std::task::Poll::Ready(Some(Ok::<_, Infallible>(Bytes::from_static(b"p"))));
        }
        if !announced {
            announced = true;
            ready.send(()).unwrap();
        }
        incoming
            .poll_recv(cx)
            .map(|item| item.map(Ok::<_, Infallible>))
    }))
}

#[tokio::test(start_paused = true)]
async fn eight_stalled_uploads_leave_control_workers_available() {
    let fixture = Fixture::new();
    let app = drip_server::router(fixture.store.clone());
    let (started, mut ready) = mpsc::unbounded_channel();
    let mut senders = Vec::new();
    let mut tasks = Vec::new();
    let mut first_credential = String::new();
    for index in 0..8 {
        let device = enroll(&fixture.store, &format!("Stream {index}"));
        let credential = device.device_credential.expose_secret();
        if index == 0 {
            first_credential = credential.to_owned();
        }
        let source = SourcePath::new('C', vec!["payload.bin".to_owned()]).unwrap();
        let selection = format!("{index:032x}");
        let generation = select(&fixture.store, credential, &selection, source.clone());
        let prepared = fixture
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
        let (sender, receiver) = mpsc::channel(1);
        senders.push(sender);
        tasks.push(tokio::spawn(
            app.clone().oneshot(
                Request::put(format!("/v1/uploads/{}/data", prepared.upload_id))
                    .header("authorization", format!("Bearer {credential}"))
                    .body(body(started.clone(), receiver))
                    .unwrap(),
            ),
        ));
    }
    for _ in 0..8 {
        ready.recv().await.unwrap();
    }
    let response = app
        .clone()
        .oneshot(
            Request::get("/v1/selections")
                .header("authorization", format!("Bearer {first_credential}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .oneshot(
            Request::put("/v1/uploads/unavailable/data")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(senders);
    for task in tasks {
        assert_eq!(
            task.await.unwrap().unwrap().status(),
            StatusCode::BAD_REQUEST
        );
    }
}

#[tokio::test(start_paused = true)]
async fn separate_cli_revocation_stops_stalled_full_and_chunk_streams() {
    for delta in [false, true] {
        let fixture = Fixture::new();
        let device = enroll(&fixture.store, "Revocation stream");
        let credential = device.device_credential.expose_secret();
        let source = SourcePath::new('C', vec!["payload.bin".to_owned()]).unwrap();
        let selection = "1".repeat(32);
        let generation = select(&fixture.store, credential, &selection, source.clone());
        let prepared = fixture
            .store
            .prepare_upload(
                credential,
                request(
                    &selection,
                    generation,
                    source.clone(),
                    b"payload",
                    Timestamp::new(0, 0).unwrap(),
                ),
            )
            .unwrap();
        if delta {
            fixture
                .store
                .plan_upload(
                    credential,
                    &prepared.upload_id,
                    UploadPlanRequest {
                        config: ChunkConfig::V1,
                        descriptors: vec![ChunkDescriptor {
                            ordinal: 0,
                            offset: 0,
                            length: 7,
                            digest: Blake3Digest::new(*blake3::hash(b"payload").as_bytes()),
                        }],
                    },
                )
                .unwrap();
        }
        let (started, mut ready) = mpsc::unbounded_channel();
        let (_sender, receiver) = mpsc::channel(1);
        let endpoint = if delta { "chunks/0" } else { "data" };
        let task = tokio::spawn(
            drip_server::router(fixture.store.clone()).oneshot(
                Request::put(format!("/v1/uploads/{}/{endpoint}", prepared.upload_id))
                    .header("authorization", format!("Bearer {credential}"))
                    .body(body(started, receiver))
                    .unwrap(),
            ),
        );
        ready.recv().await.unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_server"))
            .arg("--state-dir")
            .arg(fixture._root.join("state"))
            .arg("--mirror-dir")
            .arg(fixture._root.join("mirror"))
            .args(["devices", "revoke", &device.pc_id])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(
            task.await.unwrap().unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(
            !fixture
                ._root
                .join("mirror/.drip-staging")
                .join(format!("{}.part", prepared.upload_id))
                .exists()
        );
        assert!(!common::mirror_path(&fixture, &device, &source).exists());
        let connection =
            rusqlite::Connection::open(fixture._root.join("state/drip.sqlite3")).unwrap();
        let plans: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM upload_plan_pages WHERE upload_id = ?1",
                [&prepared.upload_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(plans, 0);
    }
}
