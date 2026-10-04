use crate::watch_queue::queue;
use crate::watch_win32::wide_path;
use crate::{metadata, names, watch};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

static TESTS: Mutex<()> = Mutex::new(());

fn start(path: &str, recursive: bool) -> Result<usize, u32> {
    watch::start_wide(wide_path(path), recursive)
}

fn query(path: &str) -> String {
    metadata::query_wide(&wide_path(path))
}

fn fixture(name: &str) -> PathBuf {
    let path = PathBuf::from(r"C:\code\drip\.perry\verification\watch")
        .join(format!("native-{}-{name}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn wait_for(needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut records = String::new();
    while Instant::now() < deadline {
        records.push_str(&queue().lock().unwrap().drain());
        if records.contains(needle) {
            return records;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("Missing notification {needle}: {records}");
}

fn remove_fixture(path: &PathBuf) {
    let absolute = fs::canonicalize(path).unwrap();
    let permitted = fs::canonicalize(r"C:\code\drip\.perry\verification\watch").unwrap();
    assert!(absolute.starts_with(&permitted));
    assert_ne!(absolute, permitted);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn real_recursive_notifications_cover_write_rename_and_delete() {
    let _guard = TESTS.lock().unwrap();
    let path = fixture("notifications");
    let id = start(path.to_str().unwrap(), true).unwrap();
    fs::create_dir(path.join("nested")).unwrap();
    wait_for("nested");
    fs::write(path.join("nested/file.txt"), "before").unwrap();
    wait_for("file.txt");
    fs::write(path.join("nested/file.txt"), "after!").unwrap();
    let written = wait_for("file.txt");
    assert!(written.contains("\"action\":3"));
    fs::rename(path.join("nested/file.txt"), path.join("nested/moved.txt")).unwrap();
    let renamed = wait_for("moved.txt");
    assert!(renamed.contains("\"action\":4"));
    assert!(renamed.contains("\"action\":5"));
    fs::remove_file(path.join("nested/moved.txt")).unwrap();
    assert!(wait_for("moved.txt").contains("\"action\":2"));
    watch::stop(id);
    assert_eq!(queue().lock().unwrap().drain(), "[]");
    watch::shutdown();
    remove_fixture(&path);
}

#[test]
fn nonrecursive_watch_excludes_descendants_and_stop_releases_handles() {
    let _guard = TESTS.lock().unwrap();
    let path = fixture("nonrecursive");
    fs::create_dir(path.join("child")).unwrap();
    let id = start(path.to_str().unwrap(), false).unwrap();
    fs::write(path.join("child/hidden.txt"), "content").unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(!queue().lock().unwrap().drain().contains("hidden.txt"));
    watch::stop(id);
    for _ in 0..40 {
        let id = start(path.to_str().unwrap(), true).unwrap();
        watch::stop(id);
    }
    watch::shutdown();
    remove_fixture(&path);
}

#[test]
fn raw_metadata_returns_fixed_drive_attributes_and_missing_errors() {
    let _guard = TESTS.lock().unwrap();
    let path = fixture("metadata");
    let directory = query(path.to_str().unwrap());
    assert!(directory.contains("\"attributes\":16"), "{directory}");
    assert!(directory.contains("\"driveType\":3"), "{directory}");
    assert!(directory.contains("\"error\":0"), "{directory}");
    fs::write(path.join("file.txt"), "file").unwrap();
    assert!(query(path.join("file.txt").to_str().unwrap()).contains("\"error\":0"));
    let names = names::query_wide(&wide_path(path.join("file.txt").to_str().unwrap()));
    assert!(names.contains("\"longPath\":"));
    assert!(names.contains("file.txt"));
    assert!(names.contains("\"error\":0"));
    let missing = path.join("absent");
    assert!(start(missing.to_str().unwrap(), true).is_err());
    assert!(!query(missing.to_str().unwrap()).contains("\"error\":0"));
    watch::shutdown();
    remove_fixture(&path);
}
