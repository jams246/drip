use drip_core::{SourcePath, SourcePathError};

#[test]
fn parse_absolute_path_uppercases_drive_and_preserves_component_case() {
    let path = SourcePath::parse_absolute("c:/Users/Jason/Photos/holiday.jpg");

    assert_eq!(
        path.unwrap().to_windows_path(),
        "C:\\Users\\Jason\\Photos\\holiday.jpg"
    );
}

#[test]
fn parse_absolute_path_accepts_drive_root() {
    let path = SourcePath::parse_absolute("D:\\").unwrap();

    assert!(path.components().is_empty());
}

#[test]
fn parse_absolute_path_rejects_unc_and_device_paths() {
    assert_eq!(
        SourcePath::parse_absolute("\\\\server\\share\\file"),
        Err(SourcePathError::NotAbsoluteLocalPath)
    );
    assert_eq!(
        SourcePath::parse_absolute("\\\\?\\C:\\file"),
        Err(SourcePathError::NotAbsoluteLocalPath)
    );
}

#[test]
fn parse_absolute_path_rejects_drive_relative_paths() {
    assert_eq!(
        SourcePath::parse_absolute("C:relative\\file"),
        Err(SourcePathError::NotAbsoluteLocalPath)
    );
}

#[test]
fn new_rejects_unsafe_components() {
    for component in ["", ".", "..", "a/b", "a\\b", "a\0b", "a:b"] {
        assert!(SourcePath::new('C', vec![component.to_owned()]).is_err());
    }
}

#[test]
fn deserialize_rejects_unsafe_components() {
    let result =
        serde_json::from_str::<SourcePath>(r#"{"drive":"C","components":["safe","..","outside"]}"#);

    assert!(result.is_err());
}

#[test]
fn deserialize_canonicalizes_drive_letter() {
    let path =
        serde_json::from_str::<SourcePath>(r#"{"drive":"c","components":["Users"]}"#).unwrap();

    assert_eq!(path.drive(), 'C');
}
