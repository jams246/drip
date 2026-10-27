use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use windows::Win32::System::Registry::REG_SZ;

use super::{SettingsStore, command_value, set_start_at_sign_in_at};
use crate::{
    settings::{ClientSettings, PendingSelectionUpdate, SettingsError},
    windows_integration::startup::{
        RegistryValue, create_readonly_key, read_value, remove_key, write_value,
    },
};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

#[test]
fn startup_registration_preserves_pending_selection_state() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    let original = ClientSettings {
        acknowledged_generation: 4,
        pending_update: Some(PendingSelectionUpdate {
            expected_generation: 4,
            selections: Vec::new(),
        }),
        ..ClientSettings::default()
    };
    store.save(&original).unwrap();
    let run_key = TestRunKey::new();
    let executable = directory.path.join("drip.exe");
    fs::write(&executable, b"synthetic").unwrap();

    set_start_at_sign_in_at(&store, true, &executable, &run_key.path).unwrap();

    let saved = store.load().unwrap();
    assert!(saved.start_at_sign_in);
    assert_eq!(saved.acknowledged_generation, 4);
    assert_eq!(saved.pending_update, original.pending_update);
    let command = decode_string(read_value(&run_key.path, "Drip").unwrap().unwrap());
    assert_eq!(
        command,
        format!(
            "\"{}\" --background",
            fs::canonicalize(executable).unwrap().display()
        )
    );
}

#[test]
fn startup_disable_removes_owned_value_after_executable_moves() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    let run_key = TestRunKey::new();
    let old = directory.path.join("old/drip.exe");
    let moved = directory.path.join("moved/drip.exe");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::write(&old, b"synthetic").unwrap();
    fs::write(&moved, b"synthetic").unwrap();
    set_start_at_sign_in_at(&store, true, &old, &run_key.path).unwrap();

    set_start_at_sign_in_at(&store, false, &moved, &run_key.path).unwrap();

    assert!(read_value(&run_key.path, "Drip").unwrap().is_none());
    assert!(!store.load().unwrap().start_at_sign_in);
}

#[test]
fn startup_enable_preserves_foreign_drip_value() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    let run_key = TestRunKey::new();
    let foreign = string_value("\"C:\\Other\\drip.exe\" --background");
    write_value(&run_key.path, "Drip", Some(&foreign)).unwrap();
    let executable = directory.path.join("drip.exe");
    fs::write(&executable, b"synthetic").unwrap();

    assert!(matches!(
        set_start_at_sign_in_at(&store, true, &executable, &run_key.path),
        Err(SettingsError::StartupConflict)
    ));

    assert_eq!(read_value(&run_key.path, "Drip").unwrap(), Some(foreign));
    assert!(!store.load().unwrap().start_at_sign_in);
}

#[test]
fn failed_settings_save_restores_previous_startup_value() {
    use std::os::windows::fs::OpenOptionsExt;

    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    store.save(&ClientSettings::default()).unwrap();
    let run_key = TestRunKey::new();
    let executable = directory.path.join("drip.exe");
    fs::write(&executable, b"synthetic").unwrap();
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(directory.path.join("drip.json"))
        .unwrap();

    assert!(matches!(
        set_start_at_sign_in_at(&store, true, &executable, &run_key.path),
        Err(SettingsError::Io {
            action: "atomically replace",
            ..
        })
    ));

    drop(blocker);
    assert!(read_value(&run_key.path, "Drip").unwrap().is_none());
    assert!(!store.load().unwrap().start_at_sign_in);
}

#[test]
fn readonly_startup_key_keeps_registration_disabled() {
    let directory = TestDirectory::new();
    let store = SettingsStore::open(&directory.path);
    let run_key = TestRunKey::new();
    create_readonly_key(&run_key.path).unwrap();
    let executable = directory.path.join("drip.exe");
    fs::write(&executable, b"synthetic").unwrap();

    assert!(matches!(
        set_start_at_sign_in_at(&store, true, &executable, &run_key.path),
        Err(SettingsError::Startup(_))
    ));

    assert!(!store.load().unwrap().start_at_sign_in);
    assert!(read_value(&run_key.path, "Drip").unwrap().is_none());
}

#[test]
fn startup_command_obeys_run_key_length_limit() {
    let maximum = format!("C:\\{}", "a".repeat(242));
    let too_long = format!("C:\\{}", "a".repeat(243));

    assert_eq!(command_value(&maximum).unwrap().data.len() / 2, 261);
    assert!(matches!(
        command_value(&too_long),
        Err(SettingsError::StartupPathTooLong)
    ));
}

fn string_value(value: &str) -> RegistryValue {
    RegistryValue {
        kind: REG_SZ,
        data: value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    }
}

fn decode_string(value: RegistryValue) -> String {
    let words = value
        .data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| u16::from_le_bytes([word[0], word[1]]))
        .take_while(|word| *word != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&words).unwrap()
}

struct TestRunKey {
    path: String,
}

impl TestRunKey {
    fn new() -> Self {
        let id = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        let path = format!(
            r"Software\Drip\Verification\startup-{}-{id}",
            std::process::id()
        );
        Self { path }
    }
}

impl Drop for TestRunKey {
    fn drop(&mut self) {
        let _ = remove_key(&self.path);
    }
}

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let root = workspace.join(".verification/phase-04/startup-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let id = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("test-{}-{id}", std::process::id()));
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
