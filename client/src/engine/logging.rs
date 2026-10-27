use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::lifecycle::LifecycleEvent;

const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Bounded JSON Lines activity log beside the executable.
pub struct ActivityLog {
    path: PathBuf,
}

impl ActivityLog {
    pub fn new(directory: &Path) -> Self {
        Self {
            path: directory.join("drip.log"),
        }
    }

    pub fn append(&self, event: &LifecycleEvent) -> io::Result<()> {
        let mut line = serde_json::to_vec(event).map_err(io::Error::other)?;
        line.push(b'\n');
        if line.len() as u64 > MAX_BYTES {
            return Err(io::Error::other(
                "activity record exceeds the log size limit",
            ));
        }
        let size = match fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) => return Err(io::Error::other("activity log is not a regular file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if size.saturating_add(line.len() as u64) > MAX_BYTES {
            let rotated = self.path.with_file_name("drip.log.1");
            match fs::remove_file(&rotated) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(&self.path, rotated)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&line)?;
        file.flush()
    }

    /// Rebuilds last reported status from the two bounded activity logs.
    pub fn snapshot(&self) -> io::Result<super::status::SyncSnapshot> {
        use std::io::Read;
        let mut snapshot = super::status::SyncSnapshot::default();
        for path in [self.path.with_file_name("drip.log.1"), self.path.clone()] {
            let file = match fs::File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(io::Error::other("activity log exceeds its size limit"));
            }
            for line in bytes.split(|byte| *byte == b'\n') {
                if let Ok(event) = serde_json::from_slice::<LifecycleEvent>(line) {
                    super::status::apply(&mut snapshot, &event);
                }
            }
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::lifecycle::EventKind;

    #[test]
    fn rotation_keeps_both_logs_bounded_and_valid_json_lines() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.verification/phase-03/log-tests")
            .join(crate::source::random_id().unwrap());
        fs::create_dir_all(&directory).unwrap();
        let log = ActivityLog::new(&directory);
        let event = LifecycleEvent {
            timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
            kind: EventKind::Error,
            selection_id: Some("selection".into()),
            source_path: None,
            operation_id: "operation".into(),
            message: "synthetic".repeat(1024),
            progress: None,
        };
        for _ in 0..700 {
            log.append(&event).unwrap();
        }
        for name in ["drip.log", "drip.log.1"] {
            let path = directory.join(name);
            assert!(fs::metadata(&path).unwrap().len() <= MAX_BYTES);
            for line in fs::read_to_string(path).unwrap().lines() {
                serde_json::from_str::<serde_json::Value>(line).unwrap();
            }
        }
    }
}
