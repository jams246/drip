use cap_std::fs::Dir;
use drip_core::SourcePath;

use crate::{StoreError, StoreResult, database};

pub(crate) const MAX_DEPTH: usize = 256;
pub(crate) const MAX_CHILDREN: usize = 100_000;
pub(crate) const MAX_ALIASES: usize = 256;
const MAX_VISITS: usize = 1_000_000;
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Budget {
    visits: usize,
    payload: usize,
}

impl Budget {
    pub(crate) fn path(&mut self, path: &SourcePath) -> StoreResult<()> {
        if path.components().len() > MAX_DEPTH {
            return Err(StoreError::UnsupportedEntry);
        }
        self.payload(path_bytes(path))
    }

    pub(crate) fn child(&mut self, parent: &SourcePath, name: &str) -> StoreResult<SourcePath> {
        self.child_payload(parent.components(), name)?;
        let mut components = parent.components().to_vec();
        components.push(name.to_owned());
        SourcePath::new(parent.drive(), components).map_err(|_| StoreError::UnsupportedEntry)
    }

    pub(crate) fn child_payload(&mut self, components: &[String], name: &str) -> StoreResult<()> {
        if components.len() >= MAX_DEPTH {
            return Err(StoreError::UnsupportedEntry);
        }
        self.payload(component_bytes(components).saturating_add(name.len() + size_of::<String>()))
    }

    pub(crate) fn names(
        &mut self,
        directory: &Dir,
        skip_nonunicode: bool,
    ) -> StoreResult<Vec<String>> {
        let mut names = Vec::new();
        for (index, entry) in directory
            .read_dir(".")
            .map_err(database::io_error)?
            .enumerate()
        {
            self.visit(index)?;
            let name = entry.map_err(database::io_error)?.file_name();
            self.payload(name.as_encoded_bytes().len() + size_of::<String>())?;
            match name.into_string() {
                Ok(name) => names.push(name),
                Err(_) if skip_nonunicode => {}
                Err(_) => return Err(StoreError::UnsupportedEntry),
            }
        }
        names.sort_unstable();
        Ok(names)
    }

    fn visit(&mut self, index: usize) -> StoreResult<()> {
        if index >= MAX_CHILDREN || self.visits >= MAX_VISITS {
            return Err(StoreError::UnsupportedEntry);
        }
        self.visits += 1;
        Ok(())
    }

    fn payload(&mut self, bytes: usize) -> StoreResult<()> {
        self.payload = self.payload.saturating_add(bytes);
        if self.payload > MAX_PAYLOAD {
            Err(StoreError::UnsupportedEntry)
        } else {
            Ok(())
        }
    }
}

fn path_bytes(path: &SourcePath) -> usize {
    component_bytes(path.components())
}

fn component_bytes(components: &[String]) -> usize {
    components.iter().fold(0usize, |bytes, part| {
        bytes.saturating_add(part.len() + size_of::<String>())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_limits_accept_boundary_then_reject_more_work() {
        let mut visits = Budget {
            visits: MAX_VISITS - 1,
            payload: 0,
        };
        assert!(visits.visit(0).is_ok());
        assert!(matches!(visits.visit(1), Err(StoreError::UnsupportedEntry)));
        assert!(Budget::default().visit(MAX_CHILDREN - 1).is_ok());
        assert!(matches!(
            Budget::default().visit(MAX_CHILDREN),
            Err(StoreError::UnsupportedEntry)
        ));
        let mut payload = Budget::default();
        assert!(payload.payload(MAX_PAYLOAD).is_ok());
        assert!(matches!(
            payload.payload(1),
            Err(StoreError::UnsupportedEntry)
        ));
    }

    #[test]
    fn depth_is_checked_before_child_path_allocation() {
        let path = SourcePath::new('C', vec!["x".into(); MAX_DEPTH]).unwrap();
        let mut budget = Budget::default();
        assert!(budget.path(&path).is_ok());
        assert!(matches!(
            budget.child(&path, "x"),
            Err(StoreError::UnsupportedEntry)
        ));
    }

    #[tokio::test]
    async fn directory_enumeration_does_not_return_a_partial_result() {
        let fixture = crate::upload_race_test_common::Fixture::new().await;
        let mut budget = Budget {
            visits: 0,
            payload: MAX_PAYLOAD,
        };
        assert!(matches!(
            budget.names(&fixture.store.mirror, false),
            Err(StoreError::UnsupportedEntry)
        ));
    }
}
