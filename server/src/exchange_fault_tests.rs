mod support;

use support::{Direction, FaultPoint, Fixture};

#[tokio::test]
async fn exchange_faults_restart_with_complete_old_or_new_entries() {
    for direction in Direction::ALL {
        for point in FaultPoint::ALL {
            let mut fixture = Fixture::new(direction).await;
            fixture.interrupt(point);
            fixture.assert_filesystem(point);
            fixture.restart();
            fixture.assert_inventory(point);
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn retired_directory_cleanup_does_not_follow_inserted_symlink() {
    let mut fixture = Fixture::new(Direction::DirectoryToFile).await;
    fixture.interrupt(FaultPoint::BeforeCleanup);
    fixture.assert_filesystem(FaultPoint::BeforeCleanup);
    let sentinel = fixture.add_sentinel_symlink();
    fixture.restart();
    assert_eq!(std::fs::read(sentinel).unwrap(), b"synthetic state file");
    fixture.assert_inventory(FaultPoint::BeforeCleanup);
}
