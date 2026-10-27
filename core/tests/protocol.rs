use drip_core::{
    SensitiveString,
    protocol::{EnrollmentRequest, EntryKind, Selection},
};

#[test]
fn enrollment_token_is_redacted_in_debug_output() {
    let request = EnrollmentRequest {
        token: SensitiveString::new("synthetic-secret"),
        label: "test-pc".to_owned(),
    };

    assert!(!format!("{request:?}").contains("synthetic-secret"));
}

#[test]
fn enrollment_token_uses_plain_string_json_field() {
    let request = EnrollmentRequest {
        token: SensitiveString::new("synthetic-secret"),
        label: "test-pc".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(request).unwrap()["token"],
        "synthetic-secret"
    );
}

#[test]
fn selections_without_root_kind_default_to_file_roots() {
    let selection: Selection = serde_json::from_value(serde_json::json!({
        "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "source": {"drive": "C", "components": ["file.txt"]},
        "active": true
    }))
    .unwrap();

    assert_eq!(selection.root_kind, EntryKind::File);
}
