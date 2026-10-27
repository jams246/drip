use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use drip_core::SourcePath;

use super::{SourceError, SourceFile, TestDirectory, same_path};

#[test]
fn path_comparison_uses_windows_case_insensitive_rules() {
    let upper = SourcePath::new('c', vec!["Users".to_owned(), "Jason".to_owned()]).unwrap();
    let lower = SourcePath::new('C', vec!["users".to_owned(), "jason".to_owned()]).unwrap();
    let unicode_upper = SourcePath::new('C', vec!["Ä".to_owned()]).unwrap();
    let unicode_lower = SourcePath::new('c', vec!["ä".to_owned()]).unwrap();

    assert!(same_path(&upper, &lower));
    assert!(same_path(&unicode_upper, &unicode_lower));
}

#[tokio::test]
async fn opened_source_preserves_unicode_component_spelling() {
    let directory = TestDirectory::new();
    let path = directory.path.join("雪🙂.bin");
    std::fs::write(&path, b"synthetic").unwrap();

    let source = SourceFile::open(&path).unwrap();

    assert_eq!(
        source.protocol_path().components().last().unwrap(),
        "雪🙂.bin"
    );
}

#[test]
fn source_rejects_paths_that_cannot_be_represented_as_unicode() {
    let path = PathBuf::from(OsString::from_wide(&[
        b'C' as u16,
        b':' as u16,
        b'\\' as u16,
        0xd800,
    ]));

    assert!(matches!(
        SourceFile::open(&path),
        Err(SourceError::InvalidPath)
    ));
}

#[test]
fn source_rejects_device_namespace_paths() {
    for path in [
        r"\\?\C:\source.bin",
        r"\\.\C:\source.bin",
        r"\??\C:\source.bin",
        r"\\server\share\source.bin",
    ] {
        assert!(matches!(
            SourceFile::open(Path::new(path)),
            Err(SourceError::InvalidPath)
        ));
    }
}

#[test]
fn application_data_names_are_excluded_case_insensitively() {
    for name in [
        "drip.json",
        "DRIP.CREDENTIALS",
        "drip.log",
        "DrIp.LoG.1",
        "DRIP.LOG.2",
        ".DRIP.CREDENTIALS.123.0.TMP",
        ".DRIP.JSON.123.0.TMP",
        "drip.json.replacement",
        "drip.credentials.replacement",
        "drip.log.replacement",
        "drip.state",
        "DRIP.STATE.17.TMP",
        "drip.settings.lock",
        "DRIP.RUN.LOCK",
    ] {
        assert!(super::super::native::is_application_data_name(name));
    }
    assert!(!super::super::native::is_application_data_name("drip.exe"));
    assert!(!super::super::native::is_application_data_name(
        "report.txt"
    ));
}
