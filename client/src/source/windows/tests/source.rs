use std::{
    fs::{self, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    time::{Duration, Instant, UNIX_EPOCH},
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

#[tokio::test]
async fn missing_range_reread_uses_independent_handle() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    let bytes = (0usize..1536 * 1024)
        .map(|index| (index.wrapping_mul(67) >> 7) as u8)
        .collect::<Vec<_>>();
    fs::write(&path, &bytes).unwrap();
    let source = SourceFile::open(&path).unwrap();
    let mut stream = drip_core::ChunkStream::new(source.chunk_reader().unwrap());
    let first = stream.next_chunk().unwrap().unwrap();
    let reread = source
        .read_range(first.descriptor.offset, first.descriptor.length)
        .await
        .unwrap();
    let next = stream.next_chunk().unwrap().unwrap();

    assert_eq!(reread, first.bytes);
    assert_eq!(next.descriptor.offset, u64::from(first.descriptor.length));
}

#[tokio::test]
async fn changed_source_before_missing_range_reread_is_rejected() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    fs::write(&path, vec![4; 128 * 1024]).unwrap();
    let source = SourceFile::open(&path).unwrap();
    fs::write(&path, vec![9; 128 * 1024 + 1]).unwrap();

    assert!(matches!(
        source.read_range(0, 64 * 1024).await,
        Err(SourceError::Changed)
    ));
}

#[tokio::test]
async fn same_size_mutation_with_restored_last_write_is_rejected() {
    let directory = TestDirectory::new();
    let path = directory.path.join("source.bin");
    fs::write(&path, b"original bytes").unwrap();
    let source = SourceFile::open(&path).unwrap();
    let original_time = source.last_write();
    let original_change_time = change_time(&path);
    let restored_time = UNIX_EPOCH
        + Duration::new(
            u64::try_from(original_time.unix_seconds()).unwrap(),
            original_time.nanos(),
        );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        fs::write(&path, b"mutated bytes!").unwrap();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(restored_time))
            .unwrap();
        file.sync_all().unwrap();
        if change_time(&path) != original_change_time {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "filesystem did not advance native ChangeTime during the fixture"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    assert!(matches!(source.verify().await, Err(SourceError::Changed)));
}

fn change_time(path: &std::path::Path) -> i64 {
    let file = OpenOptions::new().read(true).open(path).unwrap();
    super::super::native::file_info(&file)
        .unwrap()
        .basic
        .ChangeTime
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
