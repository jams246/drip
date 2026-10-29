use super::*;
use drip_client::settings::SavedSelection;
use drip_core::{SourcePath, protocol::EntryKind};

fn selection(id: &str, active: bool) -> SavedSelection {
    SavedSelection {
        id: id.into(),
        source: SourcePath::new('C', vec![format!("{id}.txt")]).unwrap(),
        root_kind: EntryKind::File,
        volume_identity: "synthetic".into(),
        root_identity: None,
        active,
    }
}

#[test]
fn watched_rows_preserve_active_order_without_erasing_retained_records() {
    let mut state = UiState::default();
    state.settings.selections = vec![
        selection("first", true),
        selection("retained", false),
        selection("last", true),
    ];
    assert_eq!(
        state
            .active_selections()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "last"]
    );
    assert_eq!(state.settings.selections.len(), 3);
}

#[test]
fn retained_only_watch_list_is_empty() {
    let mut state = UiState::default();
    state.settings.selections = vec![selection("retained", false)];
    assert!(state.active_selections().next().is_none());
}

#[test]
fn persisted_deactivation_hides_a_row_even_while_acknowledgement_is_pending() {
    let mut state = UiState::default();
    state.settings.selections = vec![selection("first", true)];
    let mut reloaded = state.settings.clone();
    reloaded.selections[0].active = false;
    reloaded.pending_update = Some(drip_client::settings::PendingSelectionUpdate {
        expected_generation: 1,
        selections: vec![],
    });
    state.settings = reloaded;
    assert!(state.active_selections().next().is_none());
    assert!(state.settings.pending_update.is_some());
}
