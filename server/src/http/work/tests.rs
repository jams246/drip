use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use tokio::sync::{Semaphore, oneshot};

use super::ControlWork;

#[tokio::test]
async fn disconnected_control_keeps_admission_until_blocking_work_finishes() {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/security-audit/http-workers");
    let root = crate::upload_test_support::create_test_dir(&parent, "worker", &AtomicU64::new(0));
    let store = crate::Store::open(root.join("state"), root.join("mirror")).unwrap();
    let controls = Arc::new(Semaphore::new(32));
    let workers = Arc::new(Semaphore::new(8));
    let work = ControlWork {
        store,
        workers: Arc::clone(&workers),
        admission: Arc::new(Arc::clone(&controls).try_acquire_owned().unwrap()),
        lease: None,
    };
    let (started, entered) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let request = tokio::spawn(async move {
        work.run(move |_| {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(())
        })
        .await
    });
    entered.await.unwrap();
    request.abort();
    let _ = request.await;
    let retained = (controls.available_permits(), workers.available_permits());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while controls.available_permits() != 32 || workers.available_permits() != 8 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(retained, (31, 7));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(start_paused = true)]
async fn unauthenticated_routes_cannot_exhaust_device_admission() {
    use axum::{
        body::{Body, Bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/security-audit/http-workers");
    let root =
        crate::upload_test_support::create_test_dir(&parent, "unauthenticated", &AtomicU64::new(0));
    let store = crate::Store::open(root.join("state"), root.join("mirror")).unwrap();
    let token = store.create_enrollment_token().unwrap();
    let device = store
        .enroll(drip_core::protocol::EnrollmentRequest {
            token: drip_core::SensitiveString::new(token),
            label: "device".to_owned(),
        })
        .unwrap();
    let app = crate::router(store.clone());
    let (started, mut ready) = tokio::sync::mpsc::unbounded_channel();
    let mut stalled = Vec::new();
    for _ in 0..4 {
        let started = started.clone();
        let mut announced = false;
        let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
            if !announced {
                announced = true;
                started.send(()).unwrap();
            }
            std::task::Poll::<Option<Result<Bytes, std::convert::Infallible>>>::Pending
        }));
        stalled.push(tokio::spawn(
            app.clone()
                .oneshot(Request::post("/v1/enroll").body(body).unwrap()),
        ));
    }
    for _ in 0..4 {
        ready.recv().await.unwrap();
    }
    let rejected = app
        .clone()
        .oneshot(Request::post("/v1/enroll").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    let selections = app
        .clone()
        .oneshot(
            Request::get("/v1/selections")
                .header(
                    "authorization",
                    format!("Bearer {}", device.device_credential.expose_secret()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(selections.status(), StatusCode::OK);
    for request in stalled {
        request.abort();
        let _ = request.await;
    }
    drop((app, store));
    // Aborted stalled requests may still hold store clones for a moment.
    let _ = std::fs::remove_dir_all(root);
}
