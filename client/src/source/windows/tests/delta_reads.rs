use std::fs;

use super::{SourceError, SourceFile, TestDirectory};

#[tokio::test]
async fn mutation_during_range_reread_is_rejected_before_bytes_are_returned() {
    let directory = TestDirectory::new();
    let path = directory.path.join("range.bin");
    fs::write(&path, vec![0x17; 1024 * 1024]).unwrap();
    let source = SourceFile::open(&path).unwrap();

    let result = source
        .read_range_with_checkpoint(0, 1024 * 1024, move || {
            fs::write(path, vec![0x29; 1024 * 1024]).unwrap();
        })
        .await;

    assert!(matches!(result, Err(SourceError::Changed)));
}
