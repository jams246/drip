use drip_core::{SensitiveString, protocol::EnrollmentRequest};

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
