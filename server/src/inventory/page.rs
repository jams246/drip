use drip_core::protocol::{Entry, EntryPage};

use crate::{StoreError, StoreResult};

use super::cursor::{MAX_PATH_RECORD_BYTES, encode};

const MAX_PAGE_BYTES: usize = 3 * 1024 * 1024;

pub(super) fn bounded(
    selection_id: String,
    mut entries: Vec<Entry>,
    scan_has_more: bool,
) -> StoreResult<EntryPage> {
    let empty = EntryPage {
        selection_id: selection_id.clone(),
        entries: Vec::new(),
        next_cursor: None,
    };
    let envelope_bytes = serde_json::to_vec(&empty)
        .map_err(StoreError::Serialization)?
        .len();
    let mut entries_bytes = 0;
    let mut accepted = 0;
    for (index, entry) in entries.iter().enumerate() {
        let entry_bytes = serde_json::to_vec(entry)
            .map_err(StoreError::Serialization)?
            .len();
        if entry_bytes > MAX_PATH_RECORD_BYTES {
            return Err(StoreError::UnsupportedEntry);
        }
        let has_more = scan_has_more || index + 1 < entries.len();
        let cursor_bytes = if has_more {
            serde_json::to_vec(&encode(&entry.source_path)?)
                .map_err(StoreError::Serialization)?
                .len()
        } else {
            4
        };
        let projected_bytes = envelope_bytes
            .saturating_sub(4)
            .saturating_add(entries_bytes)
            .saturating_add(usize::from(accepted > 0))
            .saturating_add(entry_bytes)
            .saturating_add(cursor_bytes);
        if projected_bytes > MAX_PAGE_BYTES {
            if accepted == 0 {
                return Err(StoreError::UnsupportedEntry);
            }
            break;
        }
        entries_bytes = entries_bytes
            .saturating_add(entry_bytes)
            .saturating_add(usize::from(accepted > 0));
        accepted += 1;
    }

    let has_more = scan_has_more || accepted < entries.len();
    let next_cursor = if has_more {
        Some(encode(
            &entries
                .get(accepted.saturating_sub(1))
                .ok_or(StoreError::Conflict)?
                .source_path,
        )?)
    } else {
        None
    };
    entries.truncate(accepted);
    let page = EntryPage {
        selection_id,
        entries,
        next_cursor,
    };
    if serde_json::to_vec(&page)
        .map_err(StoreError::Serialization)?
        .len()
        > MAX_PAGE_BYTES
    {
        return Err(StoreError::Conflict);
    }
    Ok(page)
}
