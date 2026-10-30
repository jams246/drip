use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::Store;

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.verification/security-audit")
            .join("storage-tests")
            .join(format!(
                "storage-{}-{}",
                std::process::id(),
                NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn daemon_ownership_fences_either_shared_root_and_allows_admin() {
    let fixture = Fixture::new();
    let state = fixture.0.join("state");
    let mirror = fixture.0.join("mirror");
    let owner = Store::open_for_daemon(&state, &mirror).unwrap();
    let stage_name = owner.uploads.create_stage("sentinel").unwrap();
    let stage = mirror.join(".drip-staging").join(stage_name);
    std::fs::write(&stage, b"unfinished synthetic upload").unwrap();
    assert!(Store::open_for_daemon(&state, fixture.0.join("other-mirror")).is_err());
    assert!(Store::open_for_daemon(fixture.0.join("other-state"), &mirror).is_err());
    let admin = Store::open(&state, &mirror).unwrap();
    assert!(admin.create_enrollment_token().is_ok());
    assert_eq!(
        std::fs::read(&stage).unwrap(),
        b"unfinished synthetic upload"
    );
    drop(admin);
    let lease = owner.take_daemon_lease().unwrap();
    assert!(owner.take_daemon_lease().is_err());
    drop(lease);
    assert!(owner.take_daemon_lease().is_ok());
}

#[test]
fn identical_storage_roots_take_one_lock() {
    let fixture = Fixture::new();
    let store = Store::open_for_daemon(&fixture.0, &fixture.0).unwrap();
    let lease = store.take_daemon_lease().unwrap();
    assert!(Store::open_for_daemon(&fixture.0, &fixture.0).is_err());
    drop(lease);
    assert!(Store::open_for_daemon(&fixture.0, &fixture.0).is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn storage_repairs_private_modes_without_changing_bytes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let fixture = Fixture::new();
    let state = fixture.0.join("state");
    let mirror = fixture.0.join("mirror");
    let original = Store::open(&state, &mirror).unwrap();
    let token = original.create_enrollment_token().unwrap();
    drop(original);
    for path in [&state, &mirror] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    let database = state.join("drip.sqlite3");
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o666)).unwrap();
    let reopened = Store::open_for_daemon(&state, &mirror).unwrap();
    assert_eq!(std::fs::metadata(&state).unwrap().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(&mirror).unwrap().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(database).unwrap().mode() & 0o777, 0o600);
    assert!(
        reopened
            .enroll(drip_core::protocol::EnrollmentRequest {
                token: drip_core::SensitiveString::new(token),
                label: "Synthetic privacy check".into(),
            })
            .is_ok()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn database_aliases_are_rejected_without_changing_targets() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let state = fixture.0.join("state");
    let mirror = fixture.0.join("mirror");
    std::fs::create_dir(&state).unwrap();
    let sentinel = fixture.0.join("sentinel");
    std::fs::write(&sentinel, b"synthetic private sentinel").unwrap();
    let database = state.join("drip.sqlite3");
    symlink(&sentinel, &database).unwrap();
    assert!(Store::open(&state, &mirror).is_err());
    std::fs::remove_file(&database).unwrap();
    std::fs::hard_link(&sentinel, &database).unwrap();
    assert!(Store::open(&state, &mirror).is_err());
    assert_eq!(
        std::fs::read(sentinel).unwrap(),
        b"synthetic private sentinel"
    );
}
