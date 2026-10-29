use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use cap_std::{ambient_authority, fs::Dir};

use super::snapshot_directory;

#[test]
fn directory_membership_stamp_changes_when_a_child_is_added() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/persistent-cache/server-membership-tests");
    fs::create_dir_all(&root).unwrap();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = root.join(format!("{}-{nanos}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let directory = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();

    let before = snapshot_directory(&directory).unwrap();
    fs::write(path.join("child.txt"), b"child").unwrap();
    let after = snapshot_directory(&directory).unwrap();

    assert_ne!(
        (before.change_seconds, before.change_nanos),
        (after.change_seconds, after.change_nanos)
    );
    drop(directory);
    fs::remove_dir_all(path).unwrap();
}
