use super::*;

#[test]
fn control_cancellation_is_a_cancelled_event() {
    assert_eq!(
        entry_error_kind(&EngineError::Cancelled),
        EventKind::Cancelled
    );
}

#[test]
fn selection_change_is_a_cancelled_event() {
    assert_eq!(
        entry_error_kind(&EngineError::SelectionChanged),
        EventKind::Cancelled
    );
}

#[test]
fn unauthorized_api_failure_remains_an_error_event() {
    let error = EngineError::Api {
        status: reqwest::StatusCode::UNAUTHORIZED,
        code: drip_core::protocol::ApiErrorCode::InvalidCredential,
        message: "registration rejected".into(),
        retryable: false,
    };

    assert_eq!(entry_error_kind(&error), EventKind::Error);
}

#[test]
fn uncertain_abort_failure_remains_an_error_event() {
    let error = EngineError::AbortFailed {
        transfer: Box::new(EngineError::UploadAborted),
        abort: Box::new(EngineError::UploadAborted),
    };

    assert_eq!(entry_error_kind(&error), EventKind::Error);
}
