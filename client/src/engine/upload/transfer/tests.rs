use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use futures_util::TryStreamExt;
use tokio::sync::mpsc;

use super::{
    finish,
    requests::{self, ChunkProgress},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn body_stream_sends_one_chunk_in_bounded_buffers() {
    let sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (events, _receiver) = mpsc::channel(4);
    let bytes = vec![12; 160 * 1024];
    let stream = requests::body_stream(
        bytes.clone(),
        ChunkProgress {
            sent: Arc::clone(&sent),
            report_clock: Arc::new(tokio::sync::Mutex::new(std::time::Instant::now())),
            events,
            source_path: drip_core::SourcePath::new('C', vec!["source.bin".into()]).unwrap(),
            source_hashed_bytes: bytes.len() as u64,
            source_chunked_bytes: bytes.len() as u64,
            chunk_count: 1,
            planned_payload_bytes: bytes.len() as u64,
            total_bytes: Some(bytes.len() as u64),
        },
    );
    let chunks: Vec<Vec<u8>> = stream.try_collect().await.unwrap();

    assert_eq!(chunks.concat(), bytes);
    assert!(chunks.iter().all(|chunk| chunk.len() <= 64 * 1024));
    assert_eq!(
        sent.load(Ordering::Relaxed),
        chunks.iter().map(|chunk| chunk.len() as u64).sum::<u64>()
    );
}

#[tokio::test]
async fn replaced_source_after_server_response_cannot_report_upload_completed() {
    use crate::{
        engine::{EngineError, UploadResult},
        source::{SourceError, SourceFile},
    };

    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    fs::write(&path, b"old bytes").unwrap();
    let source = SourceFile::open(&path).unwrap();
    let published = UploadResult {
        source_path: source.protocol_path().clone(),
        size: source.size(),
        digest: drip_core::Blake3Digest::new(*blake3::hash(b"old bytes").as_bytes()),
    };
    fs::rename(&path, directory.path.join("original.bin")).unwrap();
    fs::write(&path, b"new bytes").unwrap();
    let (events, mut receiver) = mpsc::channel(2);

    let outcome = finish(&source, published, &events).await;

    assert!(matches!(
        outcome,
        Err(EngineError::Source(SourceError::Replaced))
    ));
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let root = project.join(".verification/phase-03/client-upload-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let count = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("test-{}-{count}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { root, path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create verification directory: {error}"),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.path.parent() == Some(self.root.as_path()) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
