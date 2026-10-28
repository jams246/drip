use super::*;
use drip_client::transport::ServerUrlError;

#[test]
fn invalid_token_has_plain_language_summary_and_technical_detail() {
    let feedback = pairing(&EngineError::InvalidEnrollmentToken);
    assert_eq!(
        feedback.summary,
        "Check the one-time connection token and try again. Your previous registration stays unchanged."
    );
    assert_eq!(
        feedback.detail.as_deref(),
        Some("enrollment token must be 64 lowercase hexadecimal characters")
    );
}

#[test]
fn invalid_server_address_does_not_expose_backend_text_in_summary() {
    let error = EngineError::Transport(TransportError::ServerUrl(ServerUrlError::HttpsRequired));
    let feedback = pairing(&error);
    assert!(
        feedback
            .summary
            .starts_with("Enter a valid HTTPS server address.")
    );
    assert!(feedback.detail.unwrap().contains("HTTPS"));
}

#[test]
fn overlap_error_maps_without_parsing_its_display_text() {
    let error = EngineError::Settings(SettingsError::Selection(SelectionError::Overlap(
        r"C:\Users\Ada\Documents".into(),
    )));
    let feedback = add_selection(&error);
    assert_eq!(
        feedback.summary,
        "This file or folder overlaps another watched item."
    );
    assert!(feedback.detail.unwrap().contains("Documents"));
}

#[test]
fn failed_removal_keeps_copy_warning_and_uncertain_detail() {
    let error = EngineError::SelectionChanged;
    let feedback = remove_selection(&error);
    assert_eq!(feedback.severity, FeedbackSeverity::Warning);
    assert!(feedback.summary.contains("server copy is kept"));
    assert!(feedback.detail.unwrap().contains("selection changed"));
}

#[test]
fn uncertain_upload_outcome_asks_user_to_check_activity_and_server() {
    let feedback = sync(&EngineError::CommitUncertain, false);
    assert_eq!(feedback.severity, FeedbackSeverity::Error);
    assert!(feedback.summary.contains("outcome is uncertain"));
}

#[test]
fn stopping_summary_hides_barrier_details_for_activity_routing() {
    let feedback = stopping();
    assert_eq!(feedback.summary, "Stopping Drip safely. Please wait.");
    assert_eq!(feedback.severity, FeedbackSeverity::Info);
    assert!(!feedback.summary.contains("mutation barrier"));
    assert!(feedback.detail.unwrap().contains("server mutation barrier"));
}

#[test]
fn pending_removal_keeps_the_server_copy_after_confirmation_too() {
    let feedback = remove_selection(&EngineError::CommitUncertain);
    assert_eq!(
        feedback.summary,
        "Stop watching is pending. Your server copy is kept."
    );
}

#[test]
fn failed_local_removal_does_not_claim_a_change_is_pending() {
    let feedback = remove_selection(&EngineError::Settings(SettingsError::Selection(
        SelectionError::NotFound("missing".into()),
    )));
    assert!(!feedback.summary.contains("pending"));
}

#[test]
fn unsupported_source_has_an_actionable_summary() {
    let error = drip_client::source::capture_root(std::path::Path::new(r"\\synthetic\share\item"))
        .err()
        .unwrap();
    let feedback = add_selection(&EngineError::from(error));
    assert_eq!(
        feedback.summary,
        "Could not watch this item. Choose a supported local file or folder and check its permissions."
    );
    assert!(feedback.detail.is_some());
}

#[tokio::test]
async fn connection_failure_keeps_transport_detail_out_of_the_summary() {
    let error = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .unwrap()
        .get("https://127.0.0.1:0/")
        .send()
        .await
        .unwrap_err();
    let feedback = pairing(&EngineError::Transport(TransportError::Request(error)));
    assert!(feedback.summary.starts_with("Could not reach the server."));
    assert!(!feedback.summary.contains("127.0.0.1"));
    assert!(feedback.detail.is_some());
}
