use std::path::Path;

use drip_core::SourcePath;
use notify::{Event, EventKind, event::ModifyKind};

use super::DirtySignal;
use crate::settings::SavedSelection;

pub(super) fn route(
    signal: &DirtySignal,
    roots: &[SavedSelection],
    app: &Path,
    result: notify::Result<Event>,
) {
    let event = match result {
        Ok(event) => event,
        Err(error) => {
            uncertain(signal, roots, &error.paths);
            return;
        }
    };
    if event.need_rescan() {
        uncertain(signal, roots, &event.paths);
        return;
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    if event.paths.is_empty() {
        signal.overflow();
        return;
    }
    let structural = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    for path in &event.paths {
        // Child events carry content changes; app metadata includes our own state writes.
        if internal_event(path, app) || (!structural && same_directory(path, app)) {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("drip.json"))
            && path
                .parent()
                .is_some_and(|parent| same_directory(parent, app))
        {
            signal.refresh();
            continue;
        }
        let Some(path) = parse(path) else {
            signal.overflow();
            continue;
        };
        for root in roots {
            if path.has_windows_prefix(&root.source)
                || (structural && root.source.has_windows_prefix(&path))
            {
                let root_changed = structural && root.source.has_windows_prefix(&path);
                signal.mark(&root.id, root_changed);
            }
        }
    }
}

fn uncertain(signal: &DirtySignal, roots: &[SavedSelection], paths: &[std::path::PathBuf]) {
    let mut matched = false;
    for path in paths.iter().filter_map(|path| parse(path)) {
        for root in roots {
            if path.has_windows_prefix(&root.source) || root.source.has_windows_prefix(&path) {
                signal.mark(&root.id, true);
                matched = true;
            }
        }
    }
    if !matched {
        signal.overflow();
    }
}

fn parse(path: &Path) -> Option<SourcePath> {
    let path = path.to_str()?;
    SourcePath::parse_absolute(path.strip_prefix(r"\\?\").unwrap_or(path)).ok()
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (parse(left), parse(right)) {
        (Some(left), Some(right)) => crate::source::same_path(&left, &right),
        _ => false,
    }
}

pub(super) fn internal_event(path: &Path, app: &Path) -> bool {
    let (Some(path), Some(app)) = (parse(path), parse(app)) else {
        return false;
    };
    path.has_windows_prefix(&app)
        && path
            .components()
            .get(app.components().len())
            .is_some_and(|name| {
                !(name.eq_ignore_ascii_case("drip.json")
                    && path.components().len() == app.components().len() + 1)
                    && crate::source::is_application_data_name(name)
            })
}
