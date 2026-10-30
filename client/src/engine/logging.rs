use std::{
    collections::VecDeque,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use drip_core::Timestamp;

use super::lifecycle::{EventKind, LifecycleEvent};

mod storage;
use storage::{open_log, read_log, rotate};

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const MAX_EVENT_TEXT_BYTES: usize = 4096;
const MAX_RECENT_EVENTS: usize = 1000;
const TRUNCATION_MARKER: &str = "... [truncated]";

/// Bounded JSON Lines activity log beside the executable.
pub struct ActivityLog {
    path: PathBuf,
    writer: Mutex<()>,
}

impl ActivityLog {
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join("drip.log"),
            writer: Mutex::new(()),
        }
    }

    pub fn append(&self, event: &LifecycleEvent) -> io::Result<()> {
        let _guard = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut event = event.clone();
        truncate_text(&mut event.message);
        let mut line = serde_json::to_vec(&event).map_err(io::Error::other)?;
        line.push(b'\n');
        if line.len() as u64 > MAX_BYTES {
            return Err(io::Error::other(
                "activity record exceeds the log size limit",
            ));
        }

        let _parents = crate::private_file::parents(&self.path)?;
        let mut file = open_log(&self.path)?;
        let size = file.metadata()?.len();
        if size > MAX_BYTES {
            return Err(io::Error::other("activity log exceeds its size limit"));
        }
        let incomplete = if size == 0 {
            false
        } else {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            last[0] != b'\n'
        };
        let rotated = self.path.with_file_name("drip.log.1");
        let extra = line.len() as u64 + u64::from(incomplete);
        let should_rotate = size.saturating_add(extra) > MAX_BYTES;
        if should_rotate {
            drop(file);
            rotate(&self.path, &rotated)?;
            file = open_log(&self.path)?;
        }
        file.seek(SeekFrom::End(0))?;
        if incomplete && !should_rotate {
            file.write_all(b"\n")?;
        }
        file.write_all(&line)?;
        file.flush()
    }

    /// Loads the newest 1,000 valid records and reports skipped records as transient error rows.
    pub fn load_recent(&self) -> io::Result<Vec<LifecycleEvent>> {
        let mut events = VecDeque::with_capacity(MAX_RECENT_EVENTS);
        let paths = [self.path.with_file_name("drip.log.1"), self.path.clone()];
        let mut skipped = Vec::new();
        for path in paths {
            let Some(bytes) = read_log(&path)? else {
                continue;
            };
            let (complete, incomplete) = complete_records(&bytes);
            let mut malformed = 0;
            if !complete.is_empty() {
                let records = complete.strip_suffix(b"\n").unwrap_or(complete);
                for line in records.split(|byte| *byte == b'\n') {
                    match serde_json::from_slice::<LifecycleEvent>(line) {
                        Ok(mut event) => {
                            truncate_text(&mut event.message);
                            push_recent(&mut events, event);
                        }
                        Err(_) => malformed += 1,
                    }
                }
            }
            if malformed > 0 || incomplete {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("log");
                skipped.push(format!(
                    "Skipped {malformed} malformed records and {} incomplete final record in {name}",
                    u8::from(incomplete)
                ));
            }
        }
        if !skipped.is_empty() {
            let timestamp = current_timestamp()?;
            for message in skipped {
                push_recent(&mut events, diagnostic_event(timestamp, message));
            }
        }
        Ok(events.into_iter().collect())
    }

    /// Rebuilds last reported status from the two bounded activity logs.
    pub fn snapshot(&self) -> io::Result<super::status::SyncSnapshot> {
        let mut snapshot = super::status::SyncSnapshot::default();
        for path in [self.path.with_file_name("drip.log.1"), self.path.clone()] {
            let Some(bytes) = read_log(&path)? else {
                continue;
            };
            let (complete, _) = complete_records(&bytes);
            if !complete.is_empty() {
                let records = complete.strip_suffix(b"\n").unwrap_or(complete);
                for line in records.split(|byte| *byte == b'\n') {
                    if let Ok(event) = serde_json::from_slice::<LifecycleEvent>(line) {
                        super::status::apply(&mut snapshot, &event);
                    }
                }
            }
        }
        Ok(snapshot)
    }
}

pub(super) fn truncate_text(text: &mut String) {
    if text.len() <= MAX_EVENT_TEXT_BYTES {
        return;
    }
    let mut boundary = MAX_EVENT_TEXT_BYTES - TRUNCATION_MARKER.len();
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text.push_str(TRUNCATION_MARKER);
}

pub(super) fn diagnostic_event(timestamp: Timestamp, message: impl Into<String>) -> LifecycleEvent {
    let mut message = message.into();
    truncate_text(&mut message);
    LifecycleEvent {
        timestamp,
        kind: EventKind::Error,
        selection_id: None,
        source_path: None,
        operation_id: "logging".into(),
        message,
        progress: None,
    }
}

fn push_recent(events: &mut VecDeque<LifecycleEvent>, event: LifecycleEvent) {
    if events.len() == MAX_RECENT_EVENTS {
        events.pop_front();
    }
    events.push_back(event);
}

fn current_timestamp() -> io::Result<Timestamp> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let seconds = i64::try_from(elapsed.as_secs()).map_err(io::Error::other)?;
    Timestamp::new(seconds, elapsed.subsec_nanos()).map_err(io::Error::other)
}

fn complete_records(bytes: &[u8]) -> (&[u8], bool) {
    if bytes.last() == Some(&b'\n') || bytes.is_empty() {
        (bytes, false)
    } else if let Some(index) = bytes.iter().rposition(|byte| *byte == b'\n') {
        (&bytes[..=index], true)
    } else {
        (&[], true)
    }
}

#[cfg(test)]
#[path = "logging/tests.rs"]
mod tests;
