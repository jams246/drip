use std::{path::Path, sync::Arc, time::Duration};

use drip_core::{Blake3Digest, SourcePath, Timestamp};
use tokio::{
    fs::File as TokioFile,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
    sync::Mutex,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{
    error::SourceError,
    native::{
        HandleInfo, file_info, is_disk_file, open_file, reject_application_data,
        validate_directories, validate_drive, validate_final_path,
    },
    root::file_volume_identity,
};
use crate::engine::activity::{ActivityEvent, ProgressSnapshot, UploadPhase};

mod read;

const CHUNK_BYTES: usize = 64 * 1024;
const REPORT_INTERVAL: Duration = Duration::from_millis(200);
const WINDOWS_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

/// Opened regular source file with stable Windows identity observations.
pub struct SourceFile {
    path: std::path::PathBuf,
    protocol_path: SourcePath,
    file: Arc<Mutex<TokioFile>>,
    initial: Observation,
    volume_identity: String,
}

impl SourceFile {
    /// Opens an absolute local file without following its final reparse point.
    pub fn open(path: &Path) -> Result<Self, SourceError> {
        let protocol_path =
            SourcePath::try_from_path(path).map_err(|_| SourceError::InvalidPath)?;
        validate_drive(protocol_path.drive())?;
        reject_application_data(&protocol_path)?;
        validate_directories(&protocol_path)?;
        let file = open_file(path)?;
        let info = file_info(&file)?;
        if !is_disk_file(&file)
            || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || info.standard.Directory
            || info.standard.EndOfFile < 0
        {
            return Err(SourceError::NotRegular);
        }
        validate_final_path(&file, &protocol_path)?;
        let initial = info.observation()?;
        let volume_identity = file_volume_identity(&file)?;
        Ok(Self {
            path: path.to_owned(),
            protocol_path,
            file: Arc::new(Mutex::new(TokioFile::from_std(file))),
            initial,
            volume_identity,
        })
    }

    /// Returns the original drive and component spelling.
    pub fn protocol_path(&self) -> &SourcePath {
        &self.protocol_path
    }

    /// Returns the length recorded from the opened file.
    pub fn size(&self) -> u64 {
        self.initial.size
    }

    /// Returns the recorded Windows last-write timestamp.
    pub fn last_write(&self) -> Timestamp {
        self.initial.last_write
    }

    /// Returns the native volume identity captured from the opened handle.
    pub fn volume_identity(&self) -> &str {
        &self.volume_identity
    }

    /// Hashes the opened file using bounded memory and restores its position.
    pub async fn hash(
        &self,
        events: &tokio::sync::mpsc::Sender<ActivityEvent>,
    ) -> Result<Blake3Digest, SourceError> {
        let reader = self.reader();
        let mut file = reader.lock().await;
        file.seek(SeekFrom::Start(0)).await?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0; CHUNK_BYTES];
        let mut processed = 0u64;
        let mut last_report = std::time::Instant::now();
        let mut last_reported = 0;
        while processed < self.initial.size {
            let remaining =
                usize::try_from((self.initial.size - processed).min(CHUNK_BYTES as u64))
                    .map_err(|_| SourceError::Changed)?;
            let count = file.read(&mut buffer[..remaining]).await?;
            if count == 0 {
                return Err(SourceError::Changed);
            }
            hasher.update(&buffer[..count]);
            processed = processed
                .checked_add(count as u64)
                .ok_or(SourceError::Changed)?;
            if processed == self.initial.size && processed != last_reported {
                let _ = events
                    .send(ActivityEvent::Progress(ProgressSnapshot {
                        phase: UploadPhase::Hashing,
                        source_path: self.protocol_path.clone(),
                        processed_bytes: processed,
                        total_bytes: Some(self.initial.size),
                        source_hashed_bytes: processed,
                        source_chunked_bytes: 0,
                        chunk_count: 0,
                        planned_payload_bytes: 0,
                        file_payload_sent: 0,
                    }))
                    .await;
                last_reported = processed;
            } else if last_report.elapsed() >= REPORT_INTERVAL {
                let _ = events.try_send(ActivityEvent::Progress(ProgressSnapshot {
                    phase: UploadPhase::Hashing,
                    source_path: self.protocol_path.clone(),
                    processed_bytes: processed,
                    total_bytes: Some(self.initial.size),
                    source_hashed_bytes: processed,
                    source_chunked_bytes: 0,
                    chunk_count: 0,
                    planned_payload_bytes: 0,
                    file_payload_sent: 0,
                }));
                last_report = std::time::Instant::now();
                last_reported = processed;
            }
        }
        file.seek(SeekFrom::Start(0)).await?;
        drop(file);
        self.verify().await?;
        Ok(Blake3Digest::new(*hasher.finalize().as_bytes()))
    }

    /// Rechecks opened-handle observations and current pathname identity.
    pub async fn verify(&self) -> Result<(), SourceError> {
        let file = self.file.lock().await;
        let opened = file_info(&*file)?.observation()?;
        validate_drive(self.protocol_path.drive())?;
        validate_directories(&self.protocol_path)?;
        let resolved = open_file(&self.path)?;
        if !is_disk_file(&resolved) {
            return Err(SourceError::NotRegular);
        }
        let current = file_info(&resolved)?.observation()?;
        validate_final_path(&resolved, &self.protocol_path)?;
        if current.volume != self.initial.volume
            || current.file_id != self.initial.file_id
            || file_volume_identity(&resolved)? != self.volume_identity
        {
            return Err(SourceError::Replaced);
        }
        if opened != self.initial || current != self.initial {
            return Err(SourceError::Changed);
        }
        Ok(())
    }

    pub(crate) fn reader(&self) -> Arc<Mutex<TokioFile>> {
        Arc::clone(&self.file)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Observation {
    volume: u64,
    file_id: [u8; 16],
    size: u64,
    last_write: Timestamp,
    change_time: i64,
}

impl HandleInfo {
    fn observation(&self) -> Result<Observation, SourceError> {
        let size = u64::try_from(self.standard.EndOfFile).map_err(|_| SourceError::Changed)?;
        let ticks = self
            .basic
            .LastWriteTime
            .checked_sub(WINDOWS_EPOCH_TICKS)
            .ok_or(SourceError::InvalidTimestamp)?;
        let seconds = ticks.div_euclid(10_000_000);
        let nanos = (ticks.rem_euclid(10_000_000) * 100) as u32;
        Ok(Observation {
            volume: self.id.VolumeSerialNumber,
            file_id: self.id.FileId.Identifier,
            size,
            last_write: Timestamp::new(seconds, nanos)
                .map_err(|_| SourceError::InvalidTimestamp)?,
            change_time: self.basic.ChangeTime,
        })
    }
}
