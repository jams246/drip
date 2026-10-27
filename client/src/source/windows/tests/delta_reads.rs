use std::{
    fs::{self, OpenOptions},
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};

use super::{SourceError, SourceFile, TestDirectory};

#[tokio::test]
async fn mutation_during_range_reread_is_rejected_before_bytes_are_returned() {
    let directory = TestDirectory::new();
    let path = directory.path.join("range.bin");
    fs::write(&path, vec![0x17; 1024 * 1024]).unwrap();
    let initial_change_time = change_time(&path);
    let source = SourceFile::open(&path).unwrap();
    let original_time = source.last_write();
    let restored_time = UNIX_EPOCH
        + Duration::new(
            u64::try_from(original_time.unix_seconds()).unwrap(),
            original_time.nanos(),
        );

    let result = source
        .read_range_with_checkpoint(0, 1024 * 1024, move || {
            let replacement = vec![0x29; 1024 * 1024];
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                fs::write(&path, &replacement).unwrap();
                let file = OpenOptions::new().write(true).open(&path).unwrap();
                file.set_times(std::fs::FileTimes::new().set_modified(restored_time))
                    .unwrap();
                file.sync_all().unwrap();
                drop(file);
                if change_time(&path) != initial_change_time {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "filesystem did not advance native ChangeTime during the range reread mutation fixture"
                );
                std::thread::sleep(Duration::from_millis(25));
            }
        })
        .await;

    assert!(matches!(result, Err(SourceError::Changed)));
}

fn change_time(path: &Path) -> i64 {
    let file = OpenOptions::new().read(true).open(path).unwrap();
    super::super::native::file_info(&file)
        .unwrap()
        .basic
        .ChangeTime
}
