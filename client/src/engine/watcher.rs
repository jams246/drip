use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub(super) struct DirtySignal {
    dirty: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    wake: Arc<Notify>,
}

impl DirtySignal {
    pub fn mark(&self, overflow: bool) {
        if overflow {
            self.overflow.store(true, Ordering::Release);
        }
        self.dirty.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    pub fn take(&self) -> (bool, bool) {
        (
            self.dirty.swap(false, Ordering::AcqRel),
            self.overflow.swap(false, Ordering::AcqRel),
        )
    }

    pub async fn changed(&self) {
        self.wake.notified().await;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire) || self.overflow.load(Ordering::Acquire)
    }
}

pub(super) struct SourceWatcher {
    watcher: RecommendedWatcher,
    watched: Vec<PathBuf>,
}

impl SourceWatcher {
    pub fn new(signal: DirtySignal, app_directory: PathBuf) -> notify::Result<Self> {
        let watcher =
            notify::recommended_watcher(
                move |result: notify::Result<notify::Event>| match result {
                    Ok(event) if !matches!(event.kind, notify::EventKind::Access(_)) => {
                        let internal_only = !event.paths.is_empty()
                            && event
                                .paths
                                .iter()
                                .all(|path| internal_event(path, &app_directory));
                        if event.need_rescan() || !internal_only {
                            signal.mark(event.need_rescan());
                        }
                    }
                    Err(_) => signal.mark(true),
                    _ => {}
                },
            )?;
        Ok(Self {
            watcher,
            watched: Vec::new(),
        })
    }

    pub fn watch(&mut self, path: &Path) -> notify::Result<()> {
        if let Some(parent) = path.parent()
            && !self.watched.iter().any(|watched| watched == parent)
        {
            self.watcher.watch(parent, RecursiveMode::NonRecursive)?;
            self.watched.push(parent.to_owned());
        }
        if !self.watched.iter().any(|watched| watched == path) {
            self.watcher.watch(path, RecursiveMode::Recursive)?;
            self.watched.push(path.to_owned());
        }
        Ok(())
    }

    pub fn watch_settings(&mut self, directory: &Path) -> notify::Result<()> {
        if !self.watched.iter().any(|watched| watched == directory) {
            self.watcher.watch(directory, RecursiveMode::NonRecursive)?;
            self.watched.push(directory.to_owned());
        }
        Ok(())
    }
}

fn same_directory(left: &Path, right: &Path) -> bool {
    let parse = |path: &Path| {
        let path = path.to_str()?;
        drip_core::SourcePath::parse_absolute(path.strip_prefix(r"\\?\").unwrap_or(path)).ok()
    };
    match (parse(left), parse(right)) {
        (Some(left), Some(right)) => crate::source::same_path(&left, &right),
        _ => false,
    }
}

fn internal_event(path: &Path, app_directory: &Path) -> bool {
    path.parent()
        .is_some_and(|parent| same_directory(parent, app_directory))
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                !name.eq_ignore_ascii_case("drip.json")
                    && crate::source::is_application_data_name(name)
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturated_hints_preserve_explicit_overflow_flag() {
        let signal = DirtySignal::default();
        for _ in 0..10000 {
            signal.mark(false);
        }
        signal.mark(true);
        signal.mark(false);
        assert_eq!(signal.take(), (true, true));
        assert_eq!(signal.take(), (false, false));
    }

    #[test]
    fn state_replacements_and_logs_do_not_wake_watcher_but_saved_settings_do() {
        let app = Path::new(r"\\?\C:\app");
        assert!(internal_event(Path::new(r"C:\app\.drip.json.42.tmp"), app));
        assert!(internal_event(Path::new(r"C:\app\drip.log.1"), app));
        assert!(!internal_event(Path::new(r"C:\app\drip.json"), app));
        assert!(!internal_event(Path::new(r"C:\sources\drip.log"), app));
    }
}
