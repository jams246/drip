use std::{fs, path::Path};

use super::support::Fixture;
use crate::engine::{EventKind, LifecycleEvent};

fn unchanged(events: &[LifecycleEvent], id: &str) {
    assert!(!events.iter().any(
        |event| event.selection_id.as_deref() == Some(id) && event.kind == EventKind::Scanning
    ));
}

fn moved(fixture: &Fixture, id: &str, old: &Path, new: &Path, events: &[LifecycleEvent]) {
    let saved = fixture
        .engine
        .status()
        .unwrap()
        .selections
        .into_iter()
        .find(|saved| saved.id == id)
        .unwrap();
    assert!(saved.active);
    assert_eq!(
        saved.source,
        drip_core::SourcePath::try_from_path(new).unwrap()
    );
    assert!(!fixture.server_exists(old));
    assert!(fixture.server_exists(new));
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        EventKind::Deleted | EventKind::UploadStarted | EventKind::Chunking
    )));
}

#[tokio::test]
#[ignore = "requires the owned HTTPS Docker fixture with the root-relocation server and an isolated executable"]
async fn same_drive_root_moves_keep_watches_and_relocate_server_copies() {
    let mut fixture = Fixture::new().await;
    let file = fixture.sources.join("original.txt");
    let folder = fixture.sources.join("folder");
    fs::write(&file, b"original file").unwrap();
    fs::create_dir_all(folder.join("nested")).unwrap();
    fs::write(folder.join("nested/child.txt"), b"nested file").unwrap();
    let file_id = fixture.engine.add(&file).unwrap().selection.id;
    let folder_id = fixture.engine.add(&folder).unwrap().selection.id;
    fixture.pass(true, true).await;

    let renamed = fixture.sources.join("renamed.txt");
    let epoch = fixture.signal.epoch(&file_id);
    fs::rename(&file, &renamed).unwrap();
    fixture.changed(&file_id, epoch).await;
    let events = fixture.pass(false, false).await;
    moved(&fixture, &file_id, &file, &renamed, &events);
    unchanged(&events, &folder_id);

    let destination = fixture.sources.join("unwatched/deeper/file.txt");
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    let epoch = fixture.signal.epoch(&file_id);
    fs::rename(&renamed, &destination).unwrap();
    fixture.changed(&file_id, epoch).await;
    let events = fixture.pass(false, false).await;
    moved(&fixture, &file_id, &renamed, &destination, &events);
    unchanged(&events, &folder_id);
    let epoch = fixture.signal.epoch(&file_id);
    fs::write(&destination, b"edited at destination").unwrap();
    fixture.changed(&file_id, epoch).await;
    let events = fixture.pass(false, false).await;
    assert!(
        events
            .iter()
            .any(|event| event.kind == EventKind::UploadCompleted)
    );
    unchanged(&events, &folder_id);

    let renamed_folder = fixture.sources.join("renamed-folder");
    let epoch = fixture.signal.epoch(&folder_id);
    fs::rename(&folder, &renamed_folder).unwrap();
    fixture.changed(&folder_id, epoch).await;
    let events = fixture.pass(false, false).await;
    moved(&fixture, &folder_id, &folder, &renamed_folder, &events);
    unchanged(&events, &file_id);

    let parent = fixture.sources.join("other-parent");
    let moved_folder = parent.join("deep/moved-folder");
    fs::create_dir_all(moved_folder.parent().unwrap()).unwrap();
    let epoch = fixture.signal.epoch(&folder_id);
    fs::rename(&renamed_folder, &moved_folder).unwrap();
    fixture.changed(&folder_id, epoch).await;
    let events = fixture.pass(false, false).await;
    moved(
        &fixture,
        &folder_id,
        &renamed_folder,
        &moved_folder,
        &events,
    );

    let new_parent = fixture.sources.join("moved-parent");
    let final_folder = new_parent.join("deep/moved-folder");
    fixture.restart_worker();
    fs::rename(&parent, &new_parent).unwrap();
    let events = fixture.pass(true, true).await;
    moved(&fixture, &folder_id, &moved_folder, &final_folder, &events);
    assert!(fixture.server_exists(&final_folder.join("nested/child.txt")));

    fixture.restart_worker();
    let offline_move = fixture.sources.join("moved-while-closed.txt");
    fs::rename(&destination, &offline_move).unwrap();
    let events = fixture.pass(true, true).await;
    moved(&fixture, &file_id, &destination, &offline_move, &events);
    let epoch = fixture.signal.epoch(&file_id);
    fs::remove_file(&offline_move).unwrap();
    fixture.changed(&file_id, epoch).await;
    fixture.pass(false, false).await;
    assert!(!fixture.server_exists(&offline_move));
    assert!(
        !fixture
            .engine
            .status()
            .unwrap()
            .selections
            .into_iter()
            .find(|saved| saved.id == file_id)
            .unwrap()
            .active
    );
    assert!(fixture.server_exists(&final_folder));
}
