use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use drip_core::SourcePath;
use tokio::sync::mpsc;

use super::{SourceError, SourceFile, same_path};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn pathname_replacement_while_original_handle_is_open_is_rejected() {
    let directory = TestDirectory::new();
    let source_path = directory.path.join("source.bin");
    let old_path = directory.path.join("old.bin");
    fs::write(&source_path, b"original contents").unwrap();
    let source = SourceFile::open(&source_path).unwrap();
    let (events, _receiver) = mpsc::channel(2);
    let digest = source.hash(&events).await.unwrap();
    assert_eq!(
        digest,
        drip_core::Blake3Digest::new(*blake3::hash(b"original contents").as_bytes())
    );

    fs::rename(&source_path, old_path).unwrap();
    fs::write(&source_path, b"replacement contents").unwrap();

    assert!(matches!(source.verify().await, Err(SourceError::Replaced)));
}

#[test]
fn path_comparison_uses_windows_case_insensitive_rules() {
    let upper = SourcePath::new('c', vec!["Users".to_owned(), "Jason".to_owned()]).unwrap();
    let lower = SourcePath::new('C', vec!["users".to_owned(), "jason".to_owned()]).unwrap();

    assert!(same_path(&upper, &lower));
}

#[test]
fn application_data_names_are_excluded_case_insensitively() {
    for name in [
        "drip.json",
        "DRIP.CREDENTIALS",
        "DrIp.LoG.1",
        ".DRIP.CREDENTIALS.123.0.TMP",
        "drip.json.replacement",
    ] {
        assert!(super::native::is_application_data_name(name));
    }
    assert!(!super::native::is_application_data_name("drip.exe"));
    assert!(!super::native::is_application_data_name("report.txt"));
}

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let root = project.join(".verification/phase-01/source-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let count = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("test-{}-{count}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { root, path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create verification directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.path.parent() == Some(self.root.as_path()) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
