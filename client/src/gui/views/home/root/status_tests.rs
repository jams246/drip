use super::*;

fn root(state: SelectionState) -> RootStatus {
    RootStatus {
        selection_id: "root".into(),
        source_path: drip_core::SourcePath::new('C', vec!["folder".into()]).unwrap(),
        state,
        message: String::new(),
    }
}

#[test]
fn processing_spans_scanning_uploading_and_gaps_without_file_progress() {
    for state in [SelectionState::Scanning, SelectionState::Uploading] {
        assert_eq!(
            label(ConnectionState::Connected, false, false, Some(&root(state))),
            "Processing"
        );
    }
    assert_eq!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&root(SelectionState::Synced))
        ),
        "Up to date"
    );
}

#[test]
fn terminal_states_override_processing() {
    for (state, expected) in [
        (SelectionState::Paused, "Paused"),
        (SelectionState::Offline, "Waiting for connection"),
        (
            SelectionState::UnavailableRoot,
            "File or folder unavailable",
        ),
        (SelectionState::Cancelled, "Stopped"),
        (SelectionState::Error, "Needs attention"),
    ] {
        assert_eq!(
            label(ConnectionState::Connected, false, false, Some(&root(state))),
            expected
        );
    }
}

#[test]
fn connection_and_controls_override_active_or_stale_root_status() {
    for state in [
        SelectionState::Scanning,
        SelectionState::Uploading,
        SelectionState::Synced,
    ] {
        let root = root(state);
        for (connection, expected) in [
            (ConnectionState::Disconnected, "Waiting for connection"),
            (ConnectionState::Paused, "Paused"),
            (ConnectionState::Pausing, "Pausing..."),
        ] {
            assert_eq!(label(connection, false, false, Some(&root)), expected);
        }
        assert_eq!(
            label(ConnectionState::Connected, true, false, Some(&root)),
            "Pausing..."
        );
        assert_eq!(
            label(ConnectionState::Connected, false, true, Some(&root)),
            "Stopped"
        );
    }
    assert_eq!(
        label(ConnectionState::Connected, false, false, None),
        "Waiting"
    );
}
