use std::{
    io::{Cursor, Read},
    sync::{Arc, atomic::AtomicBool},
};

use drip_core::{ChunkStream, SourcePath};
use tokio::sync::mpsc;

use super::next_page;

fn page<R: Read>(reader: R, size: u64) -> Result<super::ChunkPage<R>, crate::source::SourceError> {
    let (events, _) = mpsc::channel(1);
    next_page(
        ChunkStream::new(reader),
        size,
        0,
        0,
        super::ChunkingContext {
            source_hashed_bytes: 0,
            planned_payload_bytes: 0,
            file_payload_sent: 0,
            source_path: SourcePath::new('C', vec!["file.bin".into()]).unwrap(),
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    )
}

#[test]
fn empty_source_has_an_empty_plan_and_empty_digest() {
    let page = page(Cursor::new(Vec::new()), 0).unwrap();

    assert!(page.descriptors.is_empty());
    assert_eq!(page.source_chunked_bytes, 0);
    assert_eq!(page.chunk_count, 0);
    assert_eq!(
        page.digest,
        Some(drip_core::Blake3Digest::new(*blake3::hash(b"").as_bytes()))
    );
    assert!(page.stream.is_none());
}

#[test]
fn page_tracks_only_descriptors_and_matches_full_stream_digest() {
    let bytes = (0usize..3 * 1024 * 1024)
        .map(|index| (index.wrapping_mul(31) >> 8) as u8)
        .collect::<Vec<_>>();
    let expected = drip_core::Blake3Digest::new(*blake3::hash(&bytes).as_bytes());
    let page = page(Cursor::new(bytes.clone()), bytes.len() as u64).unwrap();

    assert_eq!(page.source_chunked_bytes, bytes.len() as u64);
    assert_eq!(page.chunk_count, page.descriptors.len() as u64);
    assert_eq!(page.digest, Some(expected));
    assert!(page.stream.is_none());
    assert_eq!(page.descriptors.first().unwrap().offset, 0);
    assert!(
        page.descriptors
            .windows(2)
            .all(|pair| pair[0].offset + u64::from(pair[0].length) == pair[1].offset)
    );
}

#[test]
fn source_ending_before_expected_size_is_rejected() {
    assert!(matches!(
        page(Cursor::new(vec![7; 65_536]), 65_537),
        Err(crate::source::SourceError::Changed)
    ));
}

#[tokio::test]
async fn mutation_after_chunk_scanning_starts_changes_digest_and_source_observation() {
    use std::io::{Seek, SeekFrom, Write};
    use std::sync::mpsc as blocking;
    use std::time::Duration;
    #[cfg(windows)]
    use std::time::{Instant, UNIX_EPOCH};

    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".verification/phase-04/mutation-calibration");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "chunk-scan-{}.bin",
        crate::source::random_id().unwrap()
    ));
    let mut bytes = vec![0; 8 * 1024 * 1024];
    blake3::Hasher::new_derive_key("Drip chunk scan mutation fixture v1")
        .finalize_xof()
        .fill(&mut bytes);
    std::fs::write(&path, &bytes).unwrap();
    let expected = drip_core::Blake3Digest::new(*blake3::hash(&bytes).as_bytes());
    let source = crate::source::SourceFile::open(&path).unwrap();
    let (entered, observed) = blocking::sync_channel(1);
    let (release, resume) = blocking::sync_channel(1);
    let reader = GatedReader {
        file: source.chunk_reader().unwrap(),
        entered: Some(entered),
        resume,
        reads: 0,
    };
    let size = source.size();
    #[cfg(windows)]
    let initial_change_time = native_change_time(&path);
    #[cfg(windows)]
    let original_time = source.last_write();
    #[cfg(windows)]
    let restored_time = UNIX_EPOCH
        + Duration::new(
            u64::try_from(original_time.unix_seconds()).unwrap(),
            original_time.nanos(),
        );
    let worker = std::thread::spawn(move || page(reader, size));
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let changed_offset = 2 * 1024 * 1024;
    let changed_byte = bytes[changed_offset] ^ 0xff;
    writer.seek(SeekFrom::Start(changed_offset as u64)).unwrap();
    writer.write_all(&[changed_byte]).unwrap();
    #[cfg(windows)]
    writer
        .set_times(std::fs::FileTimes::new().set_modified(restored_time))
        .unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(2);
        while native_change_time(&path) == initial_change_time {
            assert!(
                Instant::now() < deadline,
                "filesystem did not advance native ChangeTime during the chunk scan mutation fixture"
            );
            let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            writer.seek(SeekFrom::Start(changed_offset as u64)).unwrap();
            writer.write_all(&[changed_byte]).unwrap();
            writer
                .set_times(std::fs::FileTimes::new().set_modified(restored_time))
                .unwrap();
            writer.sync_all().unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    release.send(()).unwrap();
    let scanned = worker.join().unwrap().unwrap();
    assert_ne!(scanned.digest, Some(expected));
    assert!(matches!(
        source.verify().await,
        Err(crate::source::SourceError::Changed)
    ));
    std::fs::remove_file(path).unwrap();
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows native ChangeTime is needed to calibrate this fixture"
)]
fn native_change_time(path: &std::path::Path) -> i64 {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx},
    };

    let file = std::fs::File::open(path).unwrap();
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: The file handle stays open and basic is a correctly sized, writable buffer.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).unwrap(),
        )
        .unwrap();
    }
    basic.ChangeTime
}

struct GatedReader {
    file: std::fs::File,
    entered: Option<std::sync::mpsc::SyncSender<()>>,
    resume: std::sync::mpsc::Receiver<()>,
    reads: usize,
}

impl Read for GatedReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.reads == 1
            && let Some(entered) = self.entered.take()
        {
            entered.send(()).map_err(std::io::Error::other)?;
            self.resume
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(std::io::Error::other)?;
        }
        self.reads += 1;
        self.file.read(bytes)
    }
}
