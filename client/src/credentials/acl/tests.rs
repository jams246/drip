use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn fresh_directory() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.verification/security-audit/acl");
    fs::create_dir_all(&root).unwrap();
    let path = root.join(format!(
        "dir-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn private_state_accepts_only_current_user_and_system_administration_owners() {
    for owner in ["S-1-5-21-1", "S-1-5-18", "S-1-5-32-544"] {
        assert!(super::trusted_owner(owner, "S-1-5-21-1"));
    }
    assert!(!super::trusted_owner("S-1-5-21-2", "S-1-5-21-1"));
}

#[test]
fn secure_directory_is_idempotent_on_a_fresh_directory() {
    let directory = fresh_directory();
    super::secure_directory(&directory).unwrap();
    super::secure_directory(&directory).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn secure_directory_rejects_a_regular_file() {
    let directory = fresh_directory();
    let file = directory.join("file.txt");
    fs::write(&file, b"synthetic").unwrap();
    assert!(super::secure_directory(&file).is_err());
    fs::remove_dir_all(directory).unwrap();
}
