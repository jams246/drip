use drip_client::{EventKind, SelectionProgress, SyncSnapshot};

use super::{LogRow, UiState};

impl UiState {
    pub(in crate::gui) fn worker_started(&mut self, generation: u64) {
        self.worker_generation = generation;
        self.status = SyncSnapshot::default();
        self.progress = None;
        self.last_file.clear();
        self.finished_operations.clear();
    }

    pub(in crate::gui) fn progress(
        &mut self,
        generation: u64,
        progress: Option<SelectionProgress>,
    ) {
        if generation != self.worker_generation {
            return;
        }
        if progress
            .as_ref()
            .is_some_and(|progress| self.finished_operations.contains(&progress.operation_id))
        {
            return;
        }
        self.progress = progress;
    }

    pub(in crate::gui) fn status(&mut self, generation: u64, status: SyncSnapshot) {
        if generation == self.worker_generation {
            self.status = status;
        }
    }

    pub(in crate::gui) fn activity(&mut self, row: LogRow) {
        let event = &row.event;
        if matches!(
            event.kind,
            EventKind::UploadCompleted
                | EventKind::FileChecked
                | EventKind::Cancelled
                | EventKind::Error
                | EventKind::Retry
        ) {
            if self
                .progress
                .as_ref()
                .is_some_and(|progress| progress.operation_id == event.operation_id)
            {
                self.progress = None;
            }
            if !self.finished_operations.contains(&event.operation_id) {
                if self.finished_operations.len() == 1000 {
                    self.finished_operations.pop_front();
                }
                self.finished_operations
                    .push_back(event.operation_id.clone());
            }
            if let Some(selection_id) = &event.selection_id
                && event.source_path.is_some()
            {
                self.last_file.insert(selection_id.clone(), event.clone());
            }
        }
        if self.logs.len() == 1000 {
            self.logs.pop_front();
        }
        self.logs.push_back(row);
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
