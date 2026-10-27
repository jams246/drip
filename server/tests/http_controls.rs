mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::Fixture;
use drip_core::{
    SensitiveString, SourcePath,
    protocol::{
        ApiError, ApiErrorCode, EnrollmentRequest, EnrollmentResponse, EntryKind, Selection,
        SelectionSet, SelectionUpdate, SelectionUpdateResponse,
    },
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn enrollment_and_selection_routes_exchange_core_dtos() {
    let fixture = Fixture::new();
    let token = fixture.store.create_enrollment_token().unwrap();
    let app = drip_server::router(fixture.store.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/enroll")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&EnrollmentRequest {
                token: SensitiveString::new(token),
                label: "HTTP test".to_owned(),
            })
            .unwrap(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let device: EnrollmentResponse = serde_json::from_slice(&bytes).unwrap();
    let request = Request::builder()
        .method("GET")
        .uri("/v1/selections")
        .header(
            "authorization",
            format!("Bearer {}", device.device_credential.expose_secret()),
        )
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let selections: SelectionSet = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(selections.generation, 0);
    let request = Request::builder()
        .method("PUT")
        .uri("/v1/selections")
        .header(
            "authorization",
            format!("Bearer {}", device.device_credential.expose_secret()),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&SelectionUpdate {
                expected_generation: 0,
                selections: vec![Selection {
                    id: "a".repeat(32),
                    source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
                    root_kind: EntryKind::File,
                    active: true,
                }],
            })
            .unwrap(),
        ))
        .unwrap();
    let response = drip_server::router(fixture.store.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let updated: SelectionUpdateResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated.generation, 1);
}

#[tokio::test]
async fn oversized_control_body_returns_shared_json_error() {
    let fixture = Fixture::new();
    let app = drip_server::router(fixture.store.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/enroll")
        .body(Body::from(vec![b'x'; 256 * 1024 + 1]))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: ApiError = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(error.code, ApiErrorCode::PayloadTooLarge);
}
