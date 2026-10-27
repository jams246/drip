#![cfg(windows)]

#[path = "common/windows_acl.rs"]
mod windows_acl;

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use drip_client::credentials::{CredentialError, CredentialStore, Pairing};
use std::os::windows::fs::OpenOptionsExt;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
const TEST_ROOT: &str = "client-credential-store";
const CHILD_MODE: &str = "DRIP_CREDENTIAL_CHILD_MODE";

#[test]
fn child_process_store_operations() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let store = CredentialStore::beside_executable().unwrap();
    match mode.as_str() {
        "write-old" => store.save(&pairing(false)).unwrap(),
        "read-old" => assert_binding(store.load().unwrap(), false),
        "read-new" => assert_binding(store.load().unwrap(), true),
        "read-corrupt" => assert!(matches!(store.load(), Err(CredentialError::Corrupt))),
        "save-new" => assert!(store.save(&pairing(true)).is_err()),
        "save-new-wait" => {
            let mut signal = [0];
            std::io::stdin().read_exact(&mut signal).unwrap();
            assert!(store.save(&pairing(true)).is_err());
        }
        "save-new-exit-after" => {
            store.save(&pairing(true)).unwrap();
            std::process::exit(75);
        }
        _ => panic!("unknown credential test operation"),
    }
}

#[test]
fn credential_should_survive_process_restart_and_after_replace_exit() {
    let directory = TestDirectory::new(TEST_ROOT);
    let executable = copy_test_executable(&directory.path);

    assert!(run_child(&executable, "write-old").status.success());
    assert!(run_child(&executable, "read-old").status.success());
    let after_replace = run_child(&executable, "save-new-exit-after");
    assert_eq!(after_replace.status.code(), Some(75));
    assert!(run_child(&executable, "read-new").status.success());
}

#[test]
fn corrupt_blob_should_fail_without_fallback() {
    let directory = TestDirectory::new(TEST_ROOT);
    let executable = copy_test_executable(&directory.path);
    assert!(run_child(&executable, "write-old").status.success());
    let credential_path = directory.path.join("drip.credentials");
    fs::write(&credential_path, b"not dpapi data").unwrap();
    let corrupted = fs::read(&credential_path).unwrap();

    let result = run_child(&executable, "read-corrupt");
    assert!(result.status.success());
    assert_eq!(fs::read(&credential_path).unwrap(), corrupted);
    assert!(!directory.path.join("drip.json").exists());
}

#[test]
fn failed_replace_should_preserve_previous_binding() {
    let directory = TestDirectory::new(TEST_ROOT);
    let executable = copy_test_executable(&directory.path);
    assert!(run_child(&executable, "write-old").status.success());
    let credentials = directory.path.join("drip.credentials");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(credentials)
        .unwrap();

    let result = run_child(&executable, "save-new");
    drop(lock);
    assert!(result.status.success());
    assert!(run_child(&executable, "read-old").status.success());
}

#[test]
fn unwritable_app_directory_should_preserve_previous_binding() {
    let directory = TestDirectory::new(TEST_ROOT);
    let executable = copy_test_executable(&directory.path);
    assert!(run_child(&executable, "write-old").status.success());
    let mut child = spawn_waiting_save(&executable);
    let acl = windows_acl::DirectoryAcl::deny_create_file(&directory.path).unwrap();

    let signal_result = child.stdin.take().map(|mut input| input.write_all(&[1]));
    let failed = child.wait_with_output().unwrap();
    acl.restore().unwrap();
    assert!(signal_result.is_some_and(|result| result.is_ok()));
    assert!(failed.status.success());
    assert!(run_child(&executable, "read-old").status.success());
}

fn run_child(executable: &Path, mode: &str) -> Output {
    let result = Command::new(executable)
        .arg("--exact")
        .arg("child_process_store_operations")
        .arg("--nocapture")
        .env(CHILD_MODE, mode)
        .output();
    result.unwrap_or_else(|error| panic!("could not launch credential child ({mode}): {error}"))
}

fn spawn_waiting_save(executable: &Path) -> Child {
    let result = Command::new(executable)
        .arg("--exact")
        .arg("child_process_store_operations")
        .arg("--nocapture")
        .env(CHILD_MODE, "save-new-wait")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    result.unwrap_or_else(|error| panic!("could not launch waiting credential child: {error}"))
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

fn assert_binding(saved: Pairing, new: bool) {
    let (server, id, key, mirror) = if new {
        (
            "https://new.example/",
            "22222222222222222222222222222222",
            'b',
            "drip-new-22222222222222222222222222222222",
        )
    } else {
        (
            "https://old.example/",
            "11111111111111111111111111111111",
            'a',
            "drip-old-11111111111111111111111111111111",
        )
    };
    assert_eq!(saved.server_url(), server);
    assert_eq!(saved.pc_id(), id);
    assert_eq!(saved.mirror_directory(), mirror);
    assert!(saved.credential() == key.to_string().repeat(64));
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
