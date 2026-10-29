#![cfg(target_os = "linux")]

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Fixture, enroll, mirror_path, request, select_kind, upload};
use drip_core::{
    SourcePath, Timestamp,
    protocol::{EntryKind, SelectionRelocationRequest, SelectionRootMove, SelectionUpdate},
};
use std::fs;
use tower::ServiceExt;

fn path(parts: &[&str]) -> SourcePath {
    SourcePath::new('C', parts.iter().map(|part| (*part).to_owned()).collect()).unwrap()
}

async fn relocate(
    fixture: &Fixture,
    credential: &str,
    id: &str,
    old: SourcePath,
    new: SourcePath,
) -> StatusCode {
    let current = fixture.store.get_selections(credential).unwrap();
    let mut selections = current.selections;
    selections
        .iter_mut()
        .find(|selection| selection.id == id)
        .unwrap()
        .source = new;
    let body = SelectionRelocationRequest {
        update: SelectionUpdate {
            expected_generation: current.generation,
            selections,
        },
        root_moves: vec![SelectionRootMove {
            selection_id: id.to_owned(),
            previous_source: old,
        }],
    };
    drip_server::router(fixture.store.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/selections")
                .header("authorization", format!("Bearer {credential}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn file_rename_and_move_preserve_bytes_selection_id_and_generation_fence() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "File relocation");
    let credential = device.device_credential.expose_secret();
    let id = "a".repeat(32);
    let old = path(&["original", "source.txt"]);
    let new = path(&["unwatched", "nested", "renamed.txt"]);
    let generation = select_kind(
        &fixture.store,
        credential,
        &id,
        old.clone(),
        EntryKind::File,
    );
    upload(
        &fixture.store,
        credential,
        request(
            &id,
            generation,
            old.clone(),
            b"original",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"original",
    )
    .await;
    let pending = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &id,
                generation,
                old.clone(),
                b"obsolete",
                Timestamp::new(11, 0).unwrap(),
            ),
        )
        .unwrap();
    fixture
        .store
        .put_upload_data(credential, &pending.upload_id, Body::from("obsolete"))
        .await
        .unwrap();
    assert_eq!(
        relocate(&fixture, credential, &id, old.clone(), new.clone()).await,
        StatusCode::OK
    );
    assert!(!mirror_path(&fixture, &device, &old).exists());
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &new)).unwrap(),
        b"original"
    );
    let selections = fixture.store.get_selections(credential).unwrap();
    assert_eq!(selections.selections[0].id, id);
    assert_eq!(selections.selections[0].source, new);
    assert!(selections.selections[0].active);
    assert!(
        fixture
            .store
            .commit_upload(credential, &pending.upload_id)
            .is_err()
    );
}

#[tokio::test]
async fn folder_move_recovers_after_filesystem_rename_before_metadata_commit() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Folder relocation");
    let credential = device.device_credential.expose_secret();
    let id = "b".repeat(32);
    let old = path(&["parent", "folder"]);
    let new = path(&["other", "deeper", "renamed"]);
    let child = path(&["parent", "folder", "nested", "file.txt"]);
    let generation = select_kind(
        &fixture.store,
        credential,
        &id,
        old.clone(),
        EntryKind::Directory,
    );
    upload(
        &fixture.store,
        credential,
        request(
            &id,
            generation,
            child,
            b"nested bytes",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"nested bytes",
    )
    .await;
    fixture.store.list_entries(credential, &id, None).unwrap();
    let destination = mirror_path(&fixture, &device, &new);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::rename(mirror_path(&fixture, &device, &old), &destination).unwrap();
    assert_eq!(
        relocate(&fixture, credential, &id, old, new.clone()).await,
        StatusCode::OK
    );
    assert_eq!(
        fs::read(destination.join("nested/file.txt")).unwrap(),
        b"nested bytes"
    );
    assert_eq!(
        fixture.store.get_selections(credential).unwrap().selections[0].source,
        new
    );
}

#[tokio::test]
async fn destination_collision_preserves_both_copies_and_the_original_binding() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Move collision");
    let credential = device.device_credential.expose_secret();
    let id = "c".repeat(32);
    let old = path(&["parent", "source.txt"]);
    let new = path(&["other", "destination.txt"]);
    let generation = select_kind(
        &fixture.store,
        credential,
        &id,
        old.clone(),
        EntryKind::File,
    );
    upload(
        &fixture.store,
        credential,
        request(
            &id,
            generation,
            old.clone(),
            b"original",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"original",
    )
    .await;
    let destination = mirror_path(&fixture, &device, &new);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, b"retained destination").unwrap();
    assert_eq!(
        relocate(&fixture, credential, &id, old.clone(), new.clone()).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &old)).unwrap(),
        b"original"
    );
    assert_eq!(fs::read(&destination).unwrap(), b"retained destination");
    fs::remove_file(mirror_path(&fixture, &device, &old)).unwrap();
    assert_eq!(
        relocate(&fixture, credential, &id, old.clone(), new).await,
        StatusCode::CONFLICT
    );
    assert_eq!(fs::read(destination).unwrap(), b"retained destination");
    assert_eq!(
        fixture.store.get_selections(credential).unwrap().selections[0].source,
        old
    );
}
