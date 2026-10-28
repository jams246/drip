use std::fmt::Display;

use drip_client::{
    EngineError,
    credentials::CredentialError,
    settings::{SelectionError, SettingsError},
    transport::TransportError,
};

use crate::gui::model::{Feedback, FeedbackSeverity};

pub(super) fn pairing(error: &EngineError) -> Feedback {
    let summary = match error {
        EngineError::InvalidEnrollmentToken => "Check the one-time connection token and try again.",
        EngineError::Api {
            code: drip_core::protocol::ApiErrorCode::InvalidCredential,
            ..
        } => "The server did not accept this one-time token.",
        EngineError::Transport(TransportError::ServerUrl(_)) => {
            "Enter a valid HTTPS server address."
        }
        EngineError::Transport(_) => {
            "Could not reach the server. Check the address and connection."
        }
        EngineError::UnsupportedProtocol(_) => "This server uses an incompatible Drip version.",
        EngineError::Credential(_) => "Registration could not be saved.",
        _ => "Could not connect. Check the server address and one-time token.",
    };
    failure(
        &format!("{summary} Your previous registration stays unchanged."),
        error,
    )
}

pub(super) fn add_selection(error: &EngineError) -> Feedback {
    let summary = match error {
        EngineError::Settings(SettingsError::Selection(SelectionError::Overlap(_))) => {
            "This file or folder overlaps another watched item."
        }
        EngineError::Source(_)
        | EngineError::Settings(SettingsError::Selection(SelectionError::Source(_))) => {
            "Could not watch this item. Choose a supported local file or folder and check its permissions."
        }
        _ => "Could not add this file or folder. Check Activity for details.",
    };
    failure(summary, error)
}

pub(super) fn remove_selection(error: &EngineError) -> Feedback {
    let summary = if uncertain_commit(error) {
        "Stop watching is pending. Your server copy is kept."
    } else {
        "Could not confirm stop watching. Your server copy is kept."
    };
    Feedback::new(summary, FeedbackSeverity::Warning, Some(error.to_string()))
}

pub(super) fn startup(error: &SettingsError) -> Feedback {
    let summary = match error {
        SettingsError::StartupConflict => "Another Drip installation controls the sign-in setting.",
        SettingsError::StartupPathTooLong => "Move Drip to a shorter folder, then try again.",
        SettingsError::StartupRollback { .. } => {
            "The sign-in setting could not be safely restored. Check Activity for details."
        }
        _ => "Could not update the sign-in setting. Check Activity for details.",
    };
    failure(summary, error)
}

pub(super) fn credential_load(error: &CredentialError) -> Feedback {
    match error {
        CredentialError::NotPaired => Feedback::new(
            "Connect this computer to your server.",
            FeedbackSeverity::Info,
            None,
        ),
        _ => failure(
            "Saved registration needs attention. Check Activity for details.",
            error,
        ),
    }
}

pub(super) fn settings_load(error: &EngineError) -> Feedback {
    failure(
        "Settings could not be loaded. Preserve drip.json and check Activity for details.",
        error,
    )
}

pub(super) fn sync(error: &EngineError, registration_required: bool) -> Feedback {
    if registration_required {
        return failure(
            "Registration needs attention. Connect again with a valid one-time token.",
            error,
        );
    }
    if matches!(error, EngineError::Cancelled) {
        return Feedback::new("Sync stopped.", FeedbackSeverity::Info, None);
    }
    let summary = match error {
        EngineError::CommitUncertain | EngineError::AbortFailed { .. } => {
            "Upload outcome is uncertain. Check Activity and your server before retrying."
        }
        _ => "Sync needs attention. Open Activity for details.",
    };
    failure(summary, error)
}

pub(super) fn diagnostic(detail: impl Into<String>) -> Feedback {
    Feedback::new(
        "Drip encountered a problem. Open Activity for technical details.",
        FeedbackSeverity::Error,
        Some(detail.into()),
    )
}

pub(super) fn stopping() -> Feedback {
    Feedback::new(
        "Stopping Drip safely. Please wait.",
        FeedbackSeverity::Info,
        Some("Waiting for safe engine shutdown and the server mutation barrier.".into()),
    )
}

pub(super) fn action(
    summary: impl Into<String>,
    severity: FeedbackSeverity,
    detail: Option<String>,
) -> Feedback {
    Feedback::new(summary, severity, detail)
}

pub(super) fn failure(summary: &str, error: &impl Display) -> Feedback {
    Feedback::new(summary, FeedbackSeverity::Error, Some(error.to_string()))
}

fn uncertain_commit(error: &EngineError) -> bool {
    match error {
        EngineError::Transport(TransportError::Request(_))
        | EngineError::ResponseJson(_)
        | EngineError::ResponseTooLarge
        | EngineError::CommitUncertain
        | EngineError::AbortFailed { .. } => true,
        EngineError::UnexpectedStatus(status) | EngineError::Api { status, .. } => {
            status.is_server_error()
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
