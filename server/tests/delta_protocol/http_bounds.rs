use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drip_core::SourcePath;
use drip_core::protocol::{ApiError, ApiErrorCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::support::{Fixture, prepare, pseudo_random};

#[tokio::test]
async fn oversized_plan_control_body_returns_413_json_error() {
    let fixture = Fixture::new();
    let device = fixture.enroll("Large page request");
    let credential = device.device_credential.expose_secret();
    let source = SourcePath::new('C', vec!["http-plan.bin".to_owned()]).unwrap();
    let selection_id = "f".repeat(32);
    let generation = fixture.select(credential, &selection_id, source.clone());
    let prepared = prepare(
        &fixture.store,
        credential,
        &selection_id,
        generation,
        source,
        &pseudo_random(1),
    );
    let response = drip_server::router(fixture.store.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/uploads/{}/plan", prepared.upload_id))
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(vec![b' '; 256 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let error: ApiError = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, ApiErrorCode::PayloadTooLarge);
}
