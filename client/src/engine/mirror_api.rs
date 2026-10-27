use std::collections::HashSet;

use drip_core::{
    SourcePath,
    protocol::{Entry, EntryPage},
};
use reqwest::Method;

use crate::transport::HttpTransport;

use super::{EngineError, api, selection::ActiveSelection};

pub(super) async fn inventory(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
) -> Result<Vec<Entry>, EngineError> {
    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut paths = HashSet::new();
    let generation = active.generation.to_string();
    loop {
        let mut query = vec![
            ("selection_id", active.selection.id.as_str()),
            ("generation", generation.as_str()),
        ];
        if let Some(cursor) = cursor.as_deref() {
            query.push(("cursor", cursor));
        }
        let page: EntryPage = api::json::<_, ()>(
            transport,
            Method::GET,
            "v1/entries",
            Some(credential),
            &query,
            None,
        )
        .await?;
        if page.selection_id != active.selection.id || page.entries.len() > 1024 {
            return Err(EngineError::InvalidResponse(
                "invalid inventory selection or page size",
            ));
        }
        for entry in page.entries {
            if !contains(&active.selection.source, &entry.source_path)
                || !paths.insert(entry.source_path.clone())
            {
                return Err(EngineError::InvalidResponse(
                    "inventory path is outside its selection or repeated",
                ));
            }
            entries.push(entry);
        }
        let Some(next) = page.next_cursor else {
            return Ok(entries);
        };
        if next.is_empty() || next.len() > 4096 || !cursors.insert(next.clone()) {
            return Err(EngineError::InvalidResponse(
                "invalid or repeated inventory cursor",
            ));
        }
        cursor = Some(next);
    }
}

pub(super) fn contains(parent: &SourcePath, child: &SourcePath) -> bool {
    parent.drive() == child.drive() && child.components().starts_with(parent.components())
}
