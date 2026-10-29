use rusqlite::Connection;
use std::process::Command;

use super::support::Fixture;
use crate::engine::cache::database::prepare_journal_for_test;

#[tokio::test]
async fn reparse_database_target_is_rejected_without_touching_target() {
    let fixture = Fixture::new();
    let target = fixture.directory.join("redirected-store");
    std::fs::create_dir(&target).unwrap();
    let sentinel = target.join("sentinel");
    std::fs::write(&sentinel, b"outside state").unwrap();
    let junction = fixture.database();
    let command = format!(
        "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
        junction.display(),
        target.display()
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let warning = fixture
        .cache()
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;

    assert!(warning.is_some());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"outside state");
}

#[tokio::test]
async fn database_and_rollback_journal_use_the_existing_acl_protector() {
    let fixture = Fixture::new();
    let cache = fixture.cache();
    assert!(
        cache
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );

    prepare_journal_for_test(&fixture.database()).unwrap();

    let metadata = std::fs::metadata(fixture.database()).unwrap();
    assert!(metadata.is_file());
    let journal = fixture.directory.join("drip.state.sqlite3-journal");
    assert!(std::fs::metadata(journal).unwrap().is_file());
}

#[tokio::test]
async fn corrupt_database_and_journal_are_preserved_before_rebuild() {
    let fixture = Fixture::new();
    let settings = fixture.directory.join("drip.json");
    let credentials = fixture.directory.join("drip.credentials");
    std::fs::write(&settings, b"settings sentinel").unwrap();
    std::fs::write(&credentials, b"credentials sentinel").unwrap();
    std::fs::write(fixture.database(), b"not a sqlite database").unwrap();
    std::fs::write(
        fixture.directory.join("drip.state.sqlite3-journal"),
        b"synthetic journal",
    )
    .unwrap();

    let cache = fixture.cache();
    let warning = cache
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;

    assert!(warning.is_some());
    assert_eq!(std::fs::read(settings).unwrap(), b"settings sentinel");
    assert_eq!(std::fs::read(credentials).unwrap(), b"credentials sentinel");
    let version = Connection::open(fixture.database())
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(version, 1, "warning={warning:?}");
    let preserved: Vec<_> = std::fs::read_dir(&fixture.directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("drip.state.corrupt.")
        })
        .collect();
    assert_eq!(
        preserved.len(),
        2,
        "warning={warning:?}; preserved={:?}",
        preserved
            .iter()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn newer_schema_stays_byte_for_byte_intact_and_disables_reuse() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.database()).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);
    let original = std::fs::read(fixture.database()).unwrap();

    let warning = fixture
        .cache()
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;

    assert!(warning.is_some());
    assert_eq!(std::fs::read(fixture.database()).unwrap(), original);
}

#[tokio::test]
async fn unsupported_fingerprint_format_stays_intact_and_disables_reuse() {
    let fixture = Fixture::new();
    let first = fixture.cache();
    assert!(
        first
            .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
            .await
            .is_none()
    );
    drop(first);
    let connection = Connection::open(fixture.database()).unwrap();
    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE state_binding SET fingerprint_format = 'future-v9';",
        )
        .unwrap();
    drop(connection);
    let original = std::fs::read(fixture.database()).unwrap();

    let warning = fixture
        .cache()
        .reconcile(&fixture.pairing, std::slice::from_ref(&fixture.selection))
        .await;

    assert!(warning.is_some());
    assert_eq!(std::fs::read(fixture.database()).unwrap(), original);
}
