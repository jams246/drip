mod common;

use std::fs;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{Fixture, enroll, mirror_path, request, select_kind, upload};
use drip_core::{
    SourcePath, Timestamp,
    protocol::{EntryKind, SelectionRelocationRequest, SelectionRootMove, SelectionUpdate},
};
use tower::ServiceExt;

fn root(parent: &str, name: &str) -> SourcePath {
    SourcePath::new('C', vec![parent.to_owned(), name.to_owned()]).unwrap()
}

fn child(parent: &SourcePath) -> SourcePath {
    let mut parts = parent.components().to_vec();
    parts.push("file.bin".to_owned());
    SourcePath::new(parent.drive(), parts).unwrap()
}

async fn move_request(
    fixture: &Fixture,
    credential: &str,
    old: SourcePath,
    new: SourcePath,
) -> StatusCode {
    let current = fixture.store.get_selections(credential).unwrap();
    let id = current
        .selections
        .iter()
        .find(|item| item.active)
        .unwrap()
        .id
        .clone();
    let mut selections = current.selections;
    selections
        .iter_mut()
        .find(|item| item.id == id)
        .unwrap()
        .source = new;
    let body = SelectionRelocationRequest {
        update: SelectionUpdate {
            expected_generation: current.generation,
            selections,
        },
        root_moves: vec![SelectionRootMove {
            selection_id: id,
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
async fn root_spelling_move_retains_inactive_ancestor_contents_and_fences_old_upload() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Root spelling");
    let credential = device.device_credential.expose_secret();
    let old = root("Parent", "é-root");
    let new = root("PARENT", "É-ROOT");
    let retained_parent = SourcePath::new('C', vec!["Parent".to_owned()]).unwrap();
    select_kind(
        &fixture.store,
        credential,
        &"f".repeat(32),
        retained_parent.clone(),
        EntryKind::Directory,
    );
    let id = "c".repeat(32);
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
            child(&old),
            b"complete original",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"complete original",
    )
    .await;
    let retained = mirror_path(&fixture, &device, &root("Parent", "retained.bin"));
    fs::write(&retained, b"retained sibling").unwrap();
    let staged = fixture
        .store
        .prepare_upload(
            credential,
            request(
                &id,
                generation,
                child(&old),
                b"obsolete",
                Timestamp::new(11, 0).unwrap(),
            ),
        )
        .unwrap();
    fixture
        .store
        .put_upload_data(credential, &staged.upload_id, Body::from("obsolete"))
        .await
        .unwrap();

    assert_eq!(
        move_request(&fixture, credential, old.clone(), new.clone()).await,
        StatusCode::OK
    );
    assert!(!mirror_path(&fixture, &device, &old).exists());
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &child(&new))).unwrap(),
        b"complete original"
    );
    assert_eq!(fs::read(retained).unwrap(), b"retained sibling");
    assert!(
        fixture
            .store
            .get_selections(credential)
            .unwrap()
            .selections
            .iter()
            .any(|selection| selection.source == retained_parent && !selection.active)
    );
    assert!(
        fixture
            .store
            .commit_upload(credential, &staged.upload_id)
            .is_err()
    );
}

#[tokio::test]
async fn relocation_recovers_filesystem_move_without_committed_selection_metadata() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Relocation recovery");
    let credential = device.device_credential.expose_secret();
    let old = root("Parent", "old");
    let new = root("PARENT", "OLD");
    let id = "d".repeat(32);
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
            child(&old),
            b"verified bytes",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"verified bytes",
    )
    .await;
    let new_path = mirror_path(&fixture, &device, &new);
    fs::create_dir_all(new_path.parent().unwrap()).unwrap();
    fs::rename(mirror_path(&fixture, &device, &old), &new_path).unwrap();

    assert_eq!(
        move_request(&fixture, credential, old, new.clone()).await,
        StatusCode::OK
    );
    let selection = fixture.store.get_selections(credential).unwrap();
    assert_eq!(selection.selections[0].source, new);
    assert_eq!(
        fs::read(new_path.join("file.bin")).unwrap(),
        b"verified bytes"
    );
}

#[tokio::test]
async fn root_relocation_never_overwrites_an_existing_destination() {
    let fixture = Fixture::new();
    let device = enroll(&fixture.store, "Relocation collision");
    let credential = device.device_credential.expose_secret();
    let old = root("Parent", "old");
    let new = root("Parent", "OLD");
    let id = "e".repeat(32);
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
            child(&old),
            b"old complete",
            Timestamp::new(10, 0).unwrap(),
        ),
        b"old complete",
    )
    .await;
    let new_path = mirror_path(&fixture, &device, &new);
    fs::create_dir(&new_path).unwrap();
    fs::write(new_path.join("retained.bin"), b"existing destination").unwrap();

    assert_eq!(
        move_request(&fixture, credential, old.clone(), new).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &child(&old))).unwrap(),
        b"old complete"
    );
    assert_eq!(
        fs::read(new_path.join("retained.bin")).unwrap(),
        b"existing destination"
    );
}
