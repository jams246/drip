use std::fs;

use super::support::Fixture;
use crate::engine::EventKind;

#[tokio::test]
#[ignore = "requires the owned HTTPS Docker fixture, deletion-response fault proxy, and isolated executable"]
async fn root_deletions_reach_server_before_watches_are_removed() {
    let mut fixture = Fixture::new().await;
    let file = fixture.sources.join("deleted.txt");
    let folder = fixture.sources.join("deleted-folder");
    let kept = fixture.sources.join("kept.txt");
    fs::write(&file, b"deleted source file").unwrap();
    fs::write(&kept, b"unrelated source file").unwrap();
    fs::create_dir_all(folder.join("nested/deeper")).unwrap();
    fs::write(folder.join("nested/deeper/file.txt"), b"nested source").unwrap();
    fs::write(folder.join("root.txt"), b"root source").unwrap();
    let deleted_file = fixture.engine.add(&file).unwrap().selection.id;
    let deleted_folder = fixture.engine.add(&folder).unwrap().selection.id;
    let retained = fixture.engine.add(&kept).unwrap().selection.id;
    fixture.pass(true, true).await;
    assert!(fixture.server_exists(&file) && fixture.server_exists(&folder));

    let epoch = fixture.signal.epoch(&deleted_file);
    fs::remove_file(&file).unwrap();
    fixture.changed(&deleted_file, epoch).await;
    let events = fixture.pass(false, false).await;
    assert!(!fixture.server_exists(&file));
    assert!(
        !fixture
            .engine
            .status()
            .unwrap()
            .selections
            .iter()
            .find(|s| s.id == deleted_file)
            .unwrap()
            .active
    );
    assert!(events.iter().any(|event| event.kind == EventKind::Deleted));
    assert!(
        !events
            .iter()
            .any(|event| event.selection_id.as_deref() == Some(&retained)
                && event.kind == EventKind::Scanning)
    );

    let control = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/root-deletion/proxy-control.json");
    fs::write(&control, br#"{"drop_delete_response":true}"#).unwrap();
    let epoch = fixture.signal.epoch(&deleted_folder);
    fs::remove_dir_all(&folder).unwrap();
    fixture.changed(&deleted_folder, epoch).await;
    let (complete, _) = fixture.attempt(false, false).await;
    let fault: serde_json::Value = serde_json::from_slice(&fs::read(&control).unwrap()).unwrap();
    assert_eq!(
        fault["drop_delete_response"], false,
        "fault proxy did not drop the deletion response"
    );
    assert!(!complete, "uncertain deletion was incorrectly acknowledged");
    assert!(
        fixture
            .engine
            .status()
            .unwrap()
            .selections
            .iter()
            .find(|s| s.id == deleted_folder)
            .unwrap()
            .active
    );
    assert!(fixture.server_exists(&folder));
    fs::create_dir(&folder).unwrap();
    let restored = folder.join("recreated.txt");
    fs::write(&restored, b"recreated while deletion was pending").unwrap();
    fixture.pass(false, false).await;
    assert!(fixture.server_exists(&restored));
    assert!(
        fixture
            .engine
            .status()
            .unwrap()
            .selections
            .iter()
            .find(|s| s.id == deleted_folder)
            .unwrap()
            .active
    );
    let epoch = fixture.signal.epoch(&deleted_folder);
    fs::remove_dir_all(&folder).unwrap();
    fixture.changed(&deleted_folder, epoch).await;
    fixture.pass(false, false).await;
    assert!(!fixture.server_exists(&folder));
    assert!(
        !fixture
            .engine
            .status()
            .unwrap()
            .selections
            .iter()
            .find(|s| s.id == deleted_folder)
            .unwrap()
            .active
    );
    assert!(fixture.server_exists(&kept));

    fs::write(&file, b"recreated after removal").unwrap();
    let reactivated = fixture.engine.add(&file).unwrap();
    assert_eq!(reactivated.selection.id, deleted_file);
    fixture.signal.refresh();
    fixture.pass(false, false).await;
    assert!(fixture.server_exists(&file));

    fixture.engine.remove(&kept).await.unwrap();
    fixture.signal.refresh();
    fixture.pass(false, false).await;
    assert!(
        fixture.server_exists(&kept),
        "manual watch removal deleted the retained mirror"
    );
    assert!(file.parent().unwrap().is_dir());
}

use std::path::PathBuf;
