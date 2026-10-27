use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use drip_core::SourcePath;
use futures_util::TryStreamExt;
use tokio::sync::{Mutex, mpsc};

use super::{body_stream, finish};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn body_stream_sends_only_declared_bytes_in_bounded_chunks() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    File::create(&path)
        .unwrap()
        .write_all(b"bytes beyond declared length")
        .unwrap();
    let file = tokio::fs::File::from_std(File::open(path).unwrap());
    let (events, _receiver) = mpsc::channel(4);
    let stream = body_stream(
        Arc::new(Mutex::new(file)),
        5,
        SourcePath::new('C', vec!["source.bin".to_owned()]).unwrap(),
        events,
    );
    let chunks: Vec<Vec<u8>> = stream.try_collect().await.unwrap();

    assert_eq!(chunks.concat(), b"bytes");
    assert!(chunks.iter().all(|chunk| chunk.len() <= 64 * 1024));
}

#[tokio::test]
async fn body_stream_fails_when_open_file_ends_before_declared_size() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    File::create(&path).unwrap().write_all(b"short").unwrap();
    let file = tokio::fs::File::from_std(File::open(path).unwrap());
    let (events, _receiver) = mpsc::channel(4);
    let stream = body_stream(
        Arc::new(Mutex::new(file)),
        6,
        SourcePath::new('C', vec!["source.bin".to_owned()]).unwrap(),
        events,
    );
    let error = stream.try_collect::<Vec<_>>().await.unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
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
        let root = project.join(".verification/phase-01/upload-stream-tests");
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
