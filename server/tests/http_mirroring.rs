mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Fixture, enroll, select_kind};
use drip_core::SourcePath;
use drip_core::protocol::{
    DirectoryCreateRequest, DirectoryCreateResponse, EntryDeleteRequest, EntryDeleteResponse,
    EntryKind, EntryPage, EntryPrecondition,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn recursive_mirror_routes_exchange_protocol_dtos() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "HTTP mirror");
    let credential = device.device_credential.expose_secret();
    let selection_id = "1".repeat(32);
    let source_path = SourcePath::new('C', vec!["folder".to_owned()]).unwrap();
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        source_path.clone(),
        EntryKind::Directory,
    );
    let app = drip_server::router(fixture.store.clone());
    let created: DirectoryCreateResponse = post_json(
        &app,
        credential,
        "/v1/directories",
        &DirectoryCreateRequest {
            selection_id: selection_id.clone(),
            generation,
            source_path: source_path.clone(),
            expected_entry: None,
        },
    )
    .await;
    assert_eq!(created.entry.kind, EntryKind::Directory);

    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/v1/entries?selection_id={selection_id}&generation={generation}"
        ))
        .header("authorization", format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let entries: EntryPage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(entries.entries[0], created.entry);

    let deleted: EntryDeleteResponse = post_json(
        &app,
        credential,
        "/v1/entries/delete",
        &EntryDeleteRequest {
            selection_id: selection_id.clone(),
            generation,
            source_path,
            expected_entry: EntryPrecondition {
                kind: created.entry.kind,
                revision: created.entry.revision,
            },
        },
    )
    .await;
    assert!(deleted.deleted);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/v1/entries?selection_id={selection_id}"))
        .header("authorization", format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: EntryPage =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(page.entries.is_empty());
}

async fn post_json<T: serde::de::DeserializeOwned>(
    app: &axum::Router,
    credential: &str,
    path: &str,
    body: &impl serde::Serialize,
) -> T {
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("authorization", format!("Bearer {credential}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
