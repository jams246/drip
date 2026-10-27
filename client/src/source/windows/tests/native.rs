#![expect(
    unsafe_code,
    reason = "native case-sensitivity test uses Windows file handles"
)]

use std::{
    fs::{self, OpenOptions},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    process::Command,
};

use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
        FileCaseSensitiveInfo, GetDriveTypeW, SetFileInformationByHandle,
    },
    System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR,
};
use windows::core::PCWSTR;

use drip_core::SourcePath;

use super::{SourceError, SourceFile, TestDirectory};

const DRIVE_REMOTE: u32 = 4;

#[test]
fn mapped_network_drives_are_rejected_when_available() {
    let remote_drive = ('A'..='Z').find(|drive| {
        let root: Vec<u16> = format!("{drive}:\\")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        (unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }) == DRIVE_REMOTE
    });
    let Some(drive) = remote_drive else {
        eprintln!("no mapped network drive available");
        return;
    };

    assert!(matches!(
        super::super::native::validate_drive(drive),
        Err(SourceError::NetworkDrive)
    ));
}

#[test]
fn source_rejects_an_ancestor_junction() {
    let directory = TestDirectory::new();
    let target = directory.path.join("target");
    let junction = directory.path.join("junction");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("source.bin"), b"synthetic").unwrap();
    let command = format!(
        "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        junction.display(),
        target.display()
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create synthetic junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(matches!(
        SourceFile::open(&junction.join("source.bin")),
        Err(SourceError::ReparsePoint(_))
    ));
}

#[test]
fn source_rejects_a_case_sensitive_directory_when_native_flag_is_available() {
    let directory = TestDirectory::new();
    let case_sensitive = directory.path.join("case-sensitive");
    fs::create_dir(&case_sensitive).unwrap();
    if let Err(error) = set_case_sensitive(&case_sensitive) {
        eprintln!("case-sensitive flag setup unavailable: {error}");
        return;
    }
    let source = case_sensitive.join("source.bin");
    fs::write(&source, b"synthetic").unwrap();

    assert!(matches!(
        SourceFile::open(&source),
        Err(SourceError::CaseSensitiveDirectory(path)) if path == case_sensitive
    ));
}

#[test]
fn executable_data_exclusion_handles_verbatim_internal_paths() {
    const CHILD: &str = "DRIP_SOURCE_VERBATIM_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        let executable = std::env::current_exe().unwrap();
        if mode == "verbatim" {
            let path = executable.to_str().unwrap();
            assert!(path.starts_with(r"\\?\"));
            assert!(SourcePath::try_from_path(executable.parent().unwrap()).is_err());
        }
        let directory = executable.parent().unwrap().to_str().unwrap();
        let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
        let application_data = Path::new(directory).join("drip.credentials");
        fs::write(&application_data, b"synthetic credential").unwrap();
        let result = SourceFile::open(&application_data);
        let result_description = match &result {
            Ok(_) => "Ok".to_owned(),
            Err(error) => format!("{error:?}"),
        };
        assert!(
            matches!(&result, Err(SourceError::ExcludedApplicationData)),
            "exe={executable:?}, app_data={application_data:?}, result={result_description}"
        );
        return;
    }

    let directory = TestDirectory::new();
    let child = directory.path.join("source-test.exe");
    fs::copy(std::env::current_exe().unwrap(), &child).unwrap();
    let canonical = fs::canonicalize(&child).unwrap();
    let canonical = PathBuf::from(
        canonical
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or_else(|| canonical.to_str().unwrap()),
    );
    let verbatim = PathBuf::from(format!(r"\\?\{}", child.display()));

    let output = Command::new(canonical)
        .args([
            "--exact",
            "source::windows::tests::native::executable_data_exclusion_handles_verbatim_internal_paths",
            "--nocapture",
        ])
        .env(CHILD, "canonical")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "canonical child test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let command = format!(
        "$env:{CHILD}='verbatim'; $p=Start-Process -FilePath '{}' -ArgumentList @('--exact','source::windows::tests::native::executable_data_exclusion_handles_verbatim_internal_paths','--nocapture') -NoNewWindow -Wait -PassThru; exit $p.ExitCode",
        verbatim.display()
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "verbatim child test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn set_case_sensitive(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options
        .access_mode(FILE_WRITE_ATTRIBUTES.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0);
    let directory = options.open(path)?;
    let info = FILE_CASE_SENSITIVE_INFO {
        Flags: FILE_CS_FLAG_CASE_SENSITIVE_DIR,
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(directory.as_raw_handle()),
            FileCaseSensitiveInfo,
            (&info as *const FILE_CASE_SENSITIVE_INFO).cast(),
            std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
        )
    }
    .map_err(std::io::Error::other)
}
