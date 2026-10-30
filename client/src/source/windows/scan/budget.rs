use drip_core::SourcePath;

use super::SourceError;

const MAX_DEPTH: usize = 256;
const MAX_CHILDREN: usize = 100_000;
const MAX_ENTRIES: usize = 1_000_000;
const MAX_METADATA: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScanBudget {
    entries: usize,
    metadata: usize,
    exceeded: bool,
}

impl Default for ScanBudget {
    fn default() -> Self {
        Self {
            entries: 1,
            metadata: 0,
            exceeded: false,
        }
    }
}

impl ScanBudget {
    pub(super) fn visit(&mut self, depth: usize, children: usize) -> Result<(), SourceError> {
        self.entries = self.entries.saturating_add(1);
        self.exceeded |= depth > MAX_DEPTH || children > MAX_CHILDREN || self.entries > MAX_ENTRIES;
        self.check()
    }

    pub(super) fn reserve(&mut self, bytes: usize) -> bool {
        self.metadata = self.metadata.saturating_add(bytes);
        self.exceeded |= self.metadata > MAX_METADATA;
        !self.exceeded
    }

    pub(super) fn check(&self) -> Result<(), SourceError> {
        if self.exceeded {
            Err(SourceError::ScanLimit)
        } else {
            Ok(())
        }
    }
}

pub(super) fn path_bytes(path: &SourcePath) -> usize {
    std::mem::size_of_val(path.components())
        + path
            .components()
            .iter()
            .map(String::capacity)
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_budget_accepts_boundaries_and_rejects_each_excess() {
        for (depth, children, entries) in [
            (MAX_DEPTH + 1, 0, 0),
            (0, MAX_CHILDREN + 1, 0),
            (0, 0, MAX_ENTRIES),
        ] {
            let mut budget = ScanBudget {
                entries,
                ..ScanBudget::default()
            };
            assert!(matches!(
                budget.visit(depth, children),
                Err(SourceError::ScanLimit)
            ));
        }
        let mut budget = ScanBudget {
            entries: MAX_ENTRIES - 1,
            ..ScanBudget::default()
        };
        assert!(budget.visit(MAX_DEPTH, MAX_CHILDREN).is_ok());
        assert!(budget.reserve(MAX_METADATA));
        assert!(!budget.reserve(1));
        assert!(matches!(budget.check(), Err(SourceError::ScanLimit)));
    }

    #[test]
    fn exhausted_scan_never_retains_complete_deletion_authority() {
        use super::super::{ScannedEntry, SourceScan};
        use drip_core::protocol::EntryKind;
        let mut scan = SourceScan {
            root_kind: EntryKind::Directory,
            complete: true,
            entries: Vec::new(),
            issues: Vec::new(),
            protected_paths: Vec::new(),
            budget: ScanBudget {
                metadata: MAX_METADATA,
                ..ScanBudget::default()
            },
        };
        scan.entry(ScannedEntry {
            source_path: SourcePath::parse_absolute(r"C:\selected\file").unwrap(),
            local_path: r"C:\selected\file".into(),
            kind: EntryKind::File,
            size: 0,
            last_write: None,
        });
        assert!(!scan.complete);
        assert!(scan.entries.is_empty());
        assert!(matches!(scan.budget.check(), Err(SourceError::ScanLimit)));
    }
}
