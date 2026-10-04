use crate::log_retention::prune_oldest;
use std::fs::{self, File, FileTimes, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

struct OldLog {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

pub(crate) struct BoundedLogs {
    file: File,
    old_logs: Vec<OldLog>,
    total_bytes: u64,
    max_bytes: u64,
    keep_bytes: u64,
}

impl BoundedLogs {
    /// The caller exclusively owns writes to the log directory.
    pub(crate) fn open(log_path: &Path, max_bytes: u64, keep_bytes: u64) -> io::Result<Self> {
        if keep_bytes > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Log retention cannot exceed the maximum log size.",
            ));
        }
        let directory = log_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Log path has no parent directory.",
            )
        })?;
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(log_path)?;
        let mut old_logs = Vec::new();
        let mut total_bytes = 0u64;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_name() == log_path.file_name().unwrap_or_default()
                || !entry.file_type()?.is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
            {
                continue;
            }
            let metadata = entry.metadata()?;
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| io::Error::other("Total log size overflowed."))?;
            old_logs.push(OldLog {
                path,
                bytes: metadata.len(),
                modified: metadata.modified()?,
            });
        }
        old_logs.sort_by(|left, right| {
            right
                .modified
                .cmp(&left.modified)
                .then_with(|| right.path.file_name().cmp(&left.path.file_name()))
        });
        let mut logs = Self {
            file,
            old_logs,
            total_bytes,
            max_bytes,
            keep_bytes,
        };
        logs.prune_to(max_bytes)?;
        Ok(logs)
    }

    pub(crate) fn append(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if bytes.len() as u64 > self.max_bytes {
            let end = bytes
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(0, |newline| newline + 1);
            let cut = end.saturating_sub(self.keep_bytes as usize);
            let start = if cut == 0 || bytes[cut - 1] == b'\n' {
                cut
            } else {
                bytes[cut..end]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .map_or(end, |newline| cut + newline + 1)
            };
            bytes = if start < end { &bytes[start..end] } else { &[] };
        }
        let incoming = bytes.len() as u64;
        if incoming > self.max_bytes - self.total_bytes {
            self.prune_to(self.keep_bytes.min(self.max_bytes - incoming))?;
        }
        self.file.seek(SeekFrom::End(0))?;
        if let Err(error) = self.file.write_all(bytes) {
            let old_bytes: u64 = self.old_logs.iter().map(|log| log.bytes).sum();
            self.total_bytes = old_bytes + self.file.metadata()?.len();
            return Err(error);
        }
        self.total_bytes += incoming;
        self.file.sync_data()
    }

    fn prune_to(&mut self, target: u64) -> io::Result<()> {
        while self.total_bytes > target {
            let Some(oldest) = self.old_logs.last_mut() else {
                prune_oldest(&mut self.file, target, target)?;
                self.total_bytes = self.file.metadata()?.len();
                return self.file.sync_data();
            };
            let newer_bytes = self.total_bytes - oldest.bytes;
            if newer_bytes > target {
                fs::remove_file(&oldest.path)?;
                self.old_logs.pop();
                self.total_bytes = newer_bytes;
                continue;
            }
            let keep = target - newer_bytes;
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&oldest.path)?;
            prune_oldest(&mut file, keep, keep)?;
            file.set_times(FileTimes::new().set_modified(oldest.modified))?;
            file.sync_data()?;
            oldest.bytes = file.metadata()?.len();
            self.total_bytes = newer_bytes + oldest.bytes;
        }
        Ok(())
    }
}
