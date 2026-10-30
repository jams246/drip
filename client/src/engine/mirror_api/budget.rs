use drip_core::{
    SourcePath,
    protocol::{Entry, EntryPage},
};

use super::EngineError;

const MAX_ENTRIES: usize = 1_000_000;
const MAX_METADATA: usize = 128 * 1024 * 1024;

#[derive(Default)]
pub(in crate::engine) struct InventoryBudget {
    pages: usize,
    entries: usize,
    metadata: usize,
}

impl InventoryBudget {
    pub(in crate::engine) fn begin_page(&mut self) -> Result<(), EngineError> {
        self.pages += 1;
        if self.pages > 1024 {
            return Err(EngineError::InvalidResponse(
                "inventory exceeds the 1024-page limit",
            ));
        }
        Ok(())
    }

    pub(in crate::engine) fn accept(&mut self, page: &EntryPage) -> Result<(), EngineError> {
        self.entries = self.entries.saturating_add(page.entries.len());
        for entry in &page.entries {
            let path = &entry.source_path;
            // Account for retained entries, duplicate-path detection, and collection spare capacity.
            let bytes = 2
                * (std::mem::size_of::<Entry>()
                    + std::mem::size_of::<SourcePath>()
                    + std::mem::size_of_val(path.components())
                    + path
                        .components()
                        .iter()
                        .map(String::capacity)
                        .sum::<usize>());
            self.metadata = self.metadata.saturating_add(bytes);
        }
        if let Some(cursor) = &page.next_cursor {
            self.metadata = self.metadata.saturating_add(cursor.capacity() * 2 + 64);
        }
        if self.entries > MAX_ENTRIES || self.metadata > MAX_METADATA {
            return Err(EngineError::InvalidResponse(
                "inventory exceeds its aggregate safety limits",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip_core::protocol::EntryKind;

    fn page() -> EntryPage {
        EntryPage {
            selection_id: "selection".into(),
            entries: vec![Entry {
                source_path: SourcePath::parse_absolute(r"C:\selected\file").unwrap(),
                kind: EntryKind::File,
                size: 0,
                digest: None,
                revision: 1,
                last_write: None,
            }],
            next_cursor: Some("next".into()),
        }
    }

    #[test]
    fn inventory_limits_reject_before_retaining_an_excess_page() {
        let mut count = InventoryBudget {
            entries: MAX_ENTRIES - 1,
            metadata: 0,
            ..InventoryBudget::default()
        };
        assert!(count.accept(&page()).is_ok());
        assert!(count.accept(&page()).is_err());
        let mut memory = InventoryBudget {
            entries: 0,
            metadata: MAX_METADATA,
            ..InventoryBudget::default()
        };
        assert!(memory.accept(&page()).is_err());
    }

    #[test]
    fn inventory_budget_accounts_for_retained_paths_and_cursors() {
        let mut budget = InventoryBudget::default();
        budget.accept(&page()).unwrap();
        assert_eq!(budget.entries, 1);
        assert!(budget.metadata > std::mem::size_of::<Entry>());
    }

    #[test]
    fn pagination_stops_before_requesting_an_excess_page() {
        let mut budget = InventoryBudget::default();
        for _ in 0..1024 {
            budget.begin_page().unwrap();
        }
        assert!(matches!(
            budget.begin_page(),
            Err(EngineError::InvalidResponse(_))
        ));
    }
}
