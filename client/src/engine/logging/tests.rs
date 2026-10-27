use super::*;

fn directory() -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.verification/phase-04/log-tests")
        .join(crate::source::random_id().unwrap());
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn event(message: impl Into<String>) -> LifecycleEvent {
    LifecycleEvent {
        timestamp: Timestamp::new(1, 0).unwrap(),
        kind: EventKind::Error,
        selection_id: Some("selection".into()),
        source_path: None,
        operation_id: "operation".into(),
        message: message.into(),
        progress: None,
    }
}

#[test]
fn append_truncates_event_text_before_writing() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    log.append(&event("é".repeat(MAX_BYTES as usize * 2)))
        .unwrap();

    let bytes = fs::read(directory.join("drip.log")).unwrap();
    let loaded: LifecycleEvent = serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap();
    assert!(bytes.len() as u64 <= MAX_BYTES);
    assert!(loaded.message.len() <= MAX_EVENT_TEXT_BYTES);
    assert!(loaded.message.ends_with(TRUNCATION_MARKER));
}

#[test]
fn load_recent_keeps_only_newest_one_thousand_rows() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    for index in 0..1050 {
        log.append(&event(format!("event-{index}"))).unwrap();
    }

    let events = log.load_recent().unwrap();
    assert_eq!(events.len(), MAX_RECENT_EVENTS);
    assert_eq!(events.first().unwrap().message, "event-50");
    assert_eq!(events.last().unwrap().message, "event-1049");
}

#[test]
fn load_recent_reports_malformed_and_incomplete_records() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    let mut bytes = serde_json::to_vec(&event("retained")).unwrap();
    bytes.extend_from_slice(b"\nnot-json\n{\"kind\":\"error\"");
    fs::write(directory.join("drip.log"), bytes).unwrap();

    let events = log.load_recent().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].message, "retained");
    assert!(events[1].message.contains("1 malformed records"));
    assert!(events[1].message.contains("1 incomplete final record"));
}

#[test]
fn append_rotates_before_either_log_exceeds_five_mib() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    for _ in 0..1500 {
        log.append(&event("synthetic".repeat(1024))).unwrap();
    }

    assert!(directory.join("drip.log.1").is_file());
    for name in ["drip.log", "drip.log.1"] {
        let path = directory.join(name);
        assert!(fs::metadata(&path).unwrap().len() <= MAX_BYTES);
        for line in fs::read_to_string(path).unwrap().lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }
}

#[test]
fn append_preserves_interrupted_final_record_for_reporting() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    fs::write(directory.join("drip.log"), b"interrupted record").unwrap();

    log.append(&event("next event")).unwrap();

    let bytes = fs::read(directory.join("drip.log")).unwrap();
    assert!(bytes.starts_with(b"interrupted record\n"));
    let events = log.load_recent().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("1 malformed records"))
    );
    assert!(events.iter().any(|event| event.message == "next event"));
}

#[test]
fn failed_rotation_preserves_current_log() {
    let directory = directory();
    let log = ActivityLog::new(&directory);
    let mut retained = vec![b'x'; MAX_BYTES as usize - 1];
    *retained.last_mut().unwrap() = b'\n';
    fs::write(directory.join("drip.log"), &retained).unwrap();
    fs::create_dir(directory.join("drip.log.1")).unwrap();

    assert!(log.append(&event("cannot rotate")).is_err());
    assert_eq!(fs::read(directory.join("drip.log")).unwrap(), retained);
    assert!(directory.join("drip.log.1").is_dir());
}
