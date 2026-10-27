use std::{
    fs::{self, OpenOptions},
    os::windows::fs::OpenOptionsExt,
};

use tokio::sync::mpsc;

use super::{SourceError, SourceFile, TestDirectory};

#[tokio::test]
async fn pathname_replacement_while_original_handle_is_open_is_rejected() {
    let directory = TestDirectory::new();
    let source_path = directory.path.join("source.bin");
    let old_path = directory.path.join("old.bin");
    fs::write(&source_path, b"original contents").unwrap();
    let source = SourceFile::open(&source_path).unwrap();
    let (events, _receiver) = mpsc::channel(2);
    let digest = source.hash(&events).await.unwrap();
    assert_eq!(
        digest,
        drip_core::Blake3Digest::new(*blake3::hash(b"original contents").as_bytes())
    );

    fs::rename(&source_path, old_path).unwrap();
    fs::write(&source_path, b"replacement contents").unwrap();

    assert!(matches!(source.verify().await, Err(SourceError::Replaced)));
}

#[test]
fn hash_future_does_not_embed_a_chunk_buffer() {
    let directory = TestDirectory::new();
    let path = directory.path.join("empty.bin");
    fs::write(&path, []).unwrap();
    let source = SourceFile::open(&path).unwrap();
    let (events, _receiver) = mpsc::channel(1);

    assert!(std::mem::size_of_val(&source.hash(&events)) < 64 * 1024);
}

#[tokio::test]
async fn changed_open_source_is_rejected() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    fs::write(&path, b"original bytes").unwrap();
    let source = SourceFile::open(&path).unwrap();
    fs::write(&path, b"mutated file with new size").unwrap();

    assert!(matches!(source.verify().await, Err(SourceError::Changed)));
}

#[test]
fn locked_source_file_is_rejected() {
    let directory = TestDirectory::new();
    let path = directory.path.join("locked.bin");
    fs::write(&path, b"synthetic").unwrap();
    let _lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();

    assert!(matches!(SourceFile::open(&path), Err(SourceError::Io(_))));
}
