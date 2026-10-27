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

#[tokio::test]
async fn root_relocation_keeps_both_existing_case_aliases_for_inventory_cleanup() {
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
        move_request(&fixture, credential, old.clone(), new.clone()).await,
        StatusCode::OK
    );
    assert_eq!(
        fs::read(mirror_path(&fixture, &device, &child(&old))).unwrap(),
        b"old complete"
    );
    assert_eq!(
        fs::read(new_path.join("retained.bin")).unwrap(),
        b"existing destination"
    );
    let selection = fixture.store.get_selections(credential).unwrap();
    assert_eq!(selection.selections[0].source, new);
    let inventory = fixture
        .store
        .list_entries_at_generation(credential, &id, Some(selection.generation), None)
        .unwrap();
    assert!(
        inventory
            .entries
            .iter()
            .any(|entry| entry.source_path == old)
    );
    assert!(
        inventory
            .entries
            .iter()
            .any(|entry| entry.source_path == child(&old))
    );
    assert!(
        inventory
            .entries
            .iter()
            .any(|entry| entry.source_path == new)
    );
    assert!(
        inventory
            .entries
            .iter()
            .any(|entry| { entry.source_path == child_named(&new, "retained.bin") })
    );
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

fn root(parent: &str, name: &str) -> SourcePath {
    SourcePath::new('C', vec![parent.to_owned(), name.to_owned()]).unwrap()
}

fn child(parent: &SourcePath) -> SourcePath {
    child_named(parent, "file.bin")
}

fn child_named(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
