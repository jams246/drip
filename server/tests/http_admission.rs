mod common;

use std::{convert::Infallible, task::Poll, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use common::Fixture;
use drip_core::protocol::{ApiError, ApiErrorCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn stalled_body() -> Body {
    Body::from_stream(futures_util::stream::pending::<Result<Bytes, Infallible>>())
}

#[tokio::test(start_paused = true)]
async fn stalled_control_body_returns_retryable_timeout() {
    let fixture = Fixture::new();
    let response = drip_server::router(fixture.store.clone())
        .oneshot(Request::post("/v1/enroll").body(stalled_body()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: ApiError = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(error.code, ApiErrorCode::RequestTimeout);
    assert!(error.retryable);
}

#[tokio::test(start_paused = true)]
async fn invalid_credentials_are_rejected_before_reading_control_bodies() {
    let fixture = Fixture::new();
    for body in [stalled_body(), Body::from(vec![0; 256 * 1024 + 1])] {
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            drip_server::router(fixture.store.clone())
                .oneshot(Request::put("/v1/selections").body(body).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test(start_paused = true)]
async fn control_admission_is_bounded_and_disconnects_release_capacity() {
    let fixture = Fixture::new();
    let app = drip_server::router(fixture.store.clone());
    let (started, mut ready) = tokio::sync::mpsc::unbounded_channel();
    let mut requests = Vec::new();
    for _ in 0..4 {
        let started = started.clone();
        let mut announced = false;
        let body = Body::from_stream(futures_util::stream::poll_fn(move |_| {
            if !announced {
                announced = true;
                started.send(()).unwrap();
            }
            Poll::<Option<Result<Bytes, Infallible>>>::Pending
        }));
        requests.push(tokio::spawn(
            app.clone()
                .oneshot(Request::post("/v1/enroll").body(body).unwrap()),
        ));
    }
    for _ in 0..4 {
        ready.recv().await.unwrap();
    }
    let response = app
        .clone()
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
    for request in requests {
        request.abort();
        let _ = request.await;
    }
    let response = app
        .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
