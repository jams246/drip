mod common;

use std::fs;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Fixture, enroll, mirror_path, request as upload_request, select_kind, upload};
use drip_core::protocol::{
    ApiError, ApiErrorCode, DirectoryCreateRequest, DirectoryCreateResponse, EntryDeleteRequest,
    EntryDeleteResponse, EntryKind, EntryPage, EntryPrecondition,
};
use drip_core::{SourcePath, Timestamp};
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

#[tokio::test]
async fn unsupported_type_exchange_returns_conflict_and_keeps_old_bytes() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "HTTP type exchange");
    let credential = device.device_credential.expose_secret();
    let selection_id = "2".repeat(32);
    let root = SourcePath::new('C', vec!["folder".to_owned()]).unwrap();
    let target = SourcePath::new('C', vec!["folder".to_owned(), "branch".to_owned()]).unwrap();
    let generation = select_kind(
        &fixture.store,
        credential,
        &selection_id,
        root,
        EntryKind::Directory,
    );
    let bytes = b"keep the original file";
    upload(
        &fixture.store,
        credential,
        upload_request(
            &selection_id,
            generation,
            target.clone(),
            bytes,
            Timestamp::new(1_700_000_000, 123_456_789).unwrap(),
        ),
        bytes,
    )
    .await;
    let page = fixture
        .store
        .list_entries(credential, &selection_id, None)
        .unwrap();
    let existing = page
        .entries
        .iter()
        .find(|entry| entry.source_path == target)
        .unwrap();
    assert_eq!(existing.kind, EntryKind::File);
    let body = DirectoryCreateRequest {
        selection_id,
        generation,
        source_path: target.clone(),
        expected_entry: Some(EntryPrecondition {
            kind: existing.kind,
            revision: existing.revision,
        }),
    };
    let app = drip_server::router(fixture.store.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/directories")
        .header("authorization", format!("Bearer {credential}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    if std::env::var_os("DRIP_REQUIRE_UNSUPPORTED_EXCHANGE").is_some() {
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    if response.status() == StatusCode::CONFLICT {
        let error: ApiError =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(error.code, ApiErrorCode::Conflict);
        assert!(error.message.contains("exchange"));
        assert_eq!(
            fs::read(mirror_path(&fixture, &device, &target)).unwrap(),
            bytes
        );
    } else {
        assert_eq!(response.status(), StatusCode::OK);
        let created: DirectoryCreateResponse =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(created.entry.kind, EntryKind::Directory);
        assert_eq!(
            fs::read_dir(mirror_path(&fixture, &device, &target))
                .unwrap()
                .count(),
            0
        );
    }
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
