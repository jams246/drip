#![expect(unsafe_code, reason = "Tests inspect Windows security descriptors")]

use super::*;
use std::{fs, io::Write, path::PathBuf};

fn directory() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/security-audit/private-files")
        .join(crate::source::random_id().unwrap());
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn bounded_reads_reject_large_private_state() {
    let path = directory().join("state");
    let mut file = open(&path, true, true).unwrap();
    file.write_all(b"12345").unwrap();
    drop(file);
    assert_eq!(
        read(&path, 4).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(read(&path, 5).unwrap(), b"12345");
}

pub(crate) fn assert_private(path: &Path) {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo,
                    SDDL_REVISION_1, SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            },
        },
        core::{PCWSTR, PWSTR},
    };
    let file = File::open(path).unwrap();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let mut text = PWSTR::null();
    // SAFETY: live file handle and writable outputs; both allocated buffers are released below.
    let permissions = unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .unwrap();
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
        .unwrap();
        let permissions = PCWSTR(text.0).to_string().unwrap();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        permissions
    };
    assert!(permissions.starts_with("D:P"), "{permissions}");
    assert_eq!(permissions.matches("(A;;FA;;;").count(), 3, "{permissions}");
    for sid in ["OW", "SY", "BA"] {
        assert!(
            permissions.contains(&format!("(A;;FA;;;{sid})")),
            "{permissions}"
        );
    }
}

#[test]
fn private_state_protects_new_and_existing_files() {
    let directory = directory();
    for name in ["new", "existing"] {
        let path = directory.join(name);
        if name == "existing" {
            fs::write(&path, b"sentinel").unwrap();
        }
        drop(open(&path, true, name == "new").unwrap());
        assert_private(&path);
    }
}

#[test]
fn private_state_rejects_hardlinks_without_changing_target() {
    let directory = directory();
    let target = directory.join("target");
    let alias = directory.join("alias");
    fs::write(&target, b"sentinel").unwrap();
    fs::hard_link(&target, &alias).unwrap();
    assert!(open(&alias, true, false).is_err());
    assert_eq!(fs::read(target).unwrap(), b"sentinel");
}

#[test]
fn private_state_rejects_directory_targets() {
    let directory = directory();
    assert!(open(&directory, true, false).is_err());
}

#[test]
fn private_state_rejects_ancestor_junctions_before_reading_or_writing() {
    let directory = directory();
    let target = directory.join("target");
    let alias = directory.join("junction");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("state"), b"sentinel").unwrap();
    let command = format!(
        "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        alias.display(),
        target.display()
    );
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(open(&alias.join("state"), true, false).is_err());
    assert_eq!(fs::read(target.join("state")).unwrap(), b"sentinel");
}

#[test]
fn open_state_handle_prevents_path_replacement() {
    let directory = directory();
    let path = directory.join("state");
    fs::write(&path, b"sentinel").unwrap();
    let file = open(&path, false, false).unwrap();
    assert!(fs::rename(&path, directory.join("renamed")).is_err());
    assert_eq!(read_open(file, 8).unwrap(), b"sentinel");
}

#[test]
fn newly_created_private_state_stays_exclusive_until_initialization_finishes() {
    let path = directory().join("initializing");
    let mut file = open(&path, true, true).unwrap();
    assert!(File::open(&path).is_err());
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    file.write_all(b"private").unwrap();
    drop(file);
    assert_private(&path);
    assert_eq!(read(&path, 7).unwrap(), b"private");
}

#[test]
fn reading_private_state_preserves_its_change_time() {
    let path = directory().join("state");
    let mut file = open(&path, true, true).unwrap();
    file.write_all(b"private").unwrap();
    drop(file);
    let before = native_change_time(&path);
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(read(&path, 7).unwrap(), b"private");
        assert_eq!(native_change_time(&path), before);
    }
    assert_private(&path);
}

fn native_change_time(path: &Path) -> i64 {
    use windows::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
    };
    let file = File::open(path).unwrap();
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: the live file handle and correctly sized writable output remain valid.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).unwrap(),
        )
        .unwrap();
    }
    basic.ChangeTime
}
