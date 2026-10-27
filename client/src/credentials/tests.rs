use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use super::storage::Stage;
use super::{CredentialError, CredentialStore, Pairing};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const TEST_ROOT: &str = "client-credential-interruption";

#[test]
fn pairing_should_reject_inconsistent_or_unsafe_bindings() {
    let id = "11111111111111111111111111111111".to_owned();
    let credential = "a".repeat(64);
    for (server, pc_id, mirror, key) in [
        (
            "http://old.example",
            id.clone(),
            format!("drip-old-{id}"),
            credential.clone(),
        ),
        (
            "https://old.example",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            format!("drip-old-{id}"),
            credential.clone(),
        ),
        (
            "https://old.example",
            id.clone(),
            format!("..\\{id}"),
            credential.clone(),
        ),
        (
            "https://old.example",
            id.clone(),
            format!("drip-old-{id}"),
            "not-a-device-credential".to_owned(),
        ),
    ] {
        assert!(matches!(
            Pairing::new(server, pc_id, mirror, key),
            Err(CredentialError::InvalidPairing(_))
        ));
    }
}

#[test]
fn interruption_child() {
    let Ok(mode) = std::env::var("DRIP_CREDENTIAL_INTERRUPT_MODE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("DRIP_CREDENTIAL_INTERRUPT_PATH").unwrap());
    let store = CredentialStore { path };
    let stage = if mode == "before" {
        Stage::BeforeReplace
    } else {
        Stage::AfterReplace
    };
    let exit_code = if mode == "before" { 70 } else { 71 };
    let _ = store.save_with_checkpoint(&pairing(true), move |current| {
        if current == stage {
            std::process::exit(exit_code);
        }
    });
}

#[test]
fn interrupted_repair_should_keep_one_complete_binding() {
    let directory = TestDirectory::new(TEST_ROOT);
    let path = directory.path.join("drip.credentials");
    let store = CredentialStore { path: path.clone() };
    store.save(&pairing(false)).unwrap();
    let executable = copy_test_executable(&directory.path);

    let before = run_interrupted(&executable, &path, "before");
    assert_eq!(before.status.code(), Some(70));
    let saved = store.load().unwrap();
    assert_eq!(saved.server_url(), "https://old.example/");
    assert_eq!(saved.pc_id(), "11111111111111111111111111111111");
    assert_eq!(
        saved.mirror_directory(),
        "drip-old-11111111111111111111111111111111"
    );
    assert!(saved.credential() == "a".repeat(64));

    let after = run_interrupted(&executable, &path, "after");
    assert_eq!(after.status.code(), Some(71));
    let saved = store.load().unwrap();
    assert_eq!(saved.server_url(), "https://new.example/");
    assert_eq!(saved.pc_id(), "22222222222222222222222222222222");
    assert_eq!(
        saved.mirror_directory(),
        "drip-new-22222222222222222222222222222222"
    );
    assert!(saved.credential() == "b".repeat(64));
}

fn run_interrupted(executable: &Path, path: &Path, mode: &str) -> std::process::Output {
    Command::new(executable)
        .arg("--exact")
        .arg("credentials::tests::interruption_child")
        .arg("--nocapture")
        .env("DRIP_CREDENTIAL_INTERRUPT_MODE", mode)
        .env("DRIP_CREDENTIAL_INTERRUPT_PATH", path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap()
}

fn copy_test_executable(directory: &Path) -> PathBuf {
    let executable = std::env::current_exe().unwrap();
    let copy = directory.join("credential-child.exe");
    fs::copy(executable, &copy).unwrap();
    copy
}

fn pairing(new: bool) -> Pairing {
    let (server, id, key, label) = if new {
        (
            "https://new.example",
            "22222222222222222222222222222222",
            'b',
            "new",
        )
    } else {
        (
            "https://old.example",
            "11111111111111111111111111111111",
            'a',
            "old",
        )
    };
    Pairing::new(
        server,
        id.to_owned(),
        format!("drip-{label}-{id}"),
        key.to_string().repeat(64),
    )
    .unwrap()
}

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let root = project.join(".verification/phase-01").join(name);
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
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
