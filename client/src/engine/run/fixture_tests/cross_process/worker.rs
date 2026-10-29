use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::sync::{mpsc, watch};

use crate::engine::{Engine, EventKind, EventSink, RunControl, test_metrics};

#[derive(Serialize)]
struct StageResult {
    stage: String,
    elapsed_ms: u64,
    first_sync_ms: Option<u64>,
    event_counts: BTreeMap<String, u64>,
    rows_before_exit: u64,
    process_cpu_100ns: u64,
    peak_working_set_bytes: u64,
    interrupted: bool,
    target_file_checked: bool,
    audit_requested: bool,
    audit_hash_paths: usize,
    audit_synced_after_hashing: bool,
}

pub async fn run(stage: &str) {
    let started = Instant::now();
    let source_root = path_env("DRIP_PERSISTENCE_SOURCE_ROOT");
    let engine = Engine::beside_executable().unwrap();
    if stage == "initial" {
        let token = fs::read_to_string(path_env("DRIP_TEST_TOKEN_FILE")).unwrap();
        engine
            .pair(
                &std::env::var("DRIP_TEST_SERVER_URL").unwrap(),
                "Persistent cache acceptance",
                token.trim(),
            )
            .await
            .unwrap();
        engine.add(&source_root).unwrap();
    }
    let selection = engine
        .status()
        .unwrap()
        .selections
        .into_iter()
        .find(|selection| selection.active)
        .unwrap();
    let target = std::env::var_os("DRIP_PERSISTENCE_INTERRUPT_FILE")
        .map(PathBuf::from)
        .map(|path| drip_core::SourcePath::try_from_path(&path).unwrap());
    let (sender, mut events) = mpsc::channel(4096);
    let (progress, _) = watch::channel(None);
    let sink = EventSink::new(sender, progress);
    let (commands, control) = watch::channel(RunControl::default());
    let worker = tokio::spawn(async move { engine.run(control, sink).await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4 * 60 * 60);
    let mut event_counts = BTreeMap::new();
    let mut synced = 0u64;
    let mut first_sync_ms = None;
    let mut paused = false;
    let mut resumed = false;
    let mut stopped = false;
    let mut interrupted = false;
    let mut target_file_checked = false;
    let mut audit_requested = false;
    let mut audit_hash_paths = HashSet::new();
    let mut audit_synced_after_hashing = false;
    let audit_expected_paths = if matches!(stage, "audit" | "release-audit") {
        cache_rows(&path_env("DRIP_TEST_ARTIFACT_ROOT").join("drip.state.sqlite3"))
    } else {
        0
    };
    loop {
        if stopped && worker.is_finished() {
            break;
        }
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("worker acceptance timed out");
        let Some(event) = event else {
            assert!(stopped, "worker event channel closed before completion");
            break;
        };
        *event_counts
            .entry(event_key(event.kind).to_owned())
            .or_default() += 1;
        if audit_requested
            && event.kind == EventKind::Hashing
            && let Some(path) = &event.source_path
        {
            audit_hash_paths.insert(path.clone());
        }
        if event.kind == EventKind::FileChecked && event.source_path.as_ref() == target.as_ref() {
            target_file_checked = true;
        }
        if event.kind == EventKind::Synced
            && event.selection_id.as_deref() == Some(&selection.id)
            && audit_requested
            && u64::try_from(audit_hash_paths.len()).unwrap_or(u64::MAX) >= audit_expected_paths
        {
            audit_synced_after_hashing = true;
        }
        if event.selection_id.as_deref() == Some(&selection.id) && event.kind == EventKind::Synced {
            synced += 1;
            first_sync_ms.get_or_insert(elapsed_ms(started));
        }
        match stage {
            "warm" if synced >= 3 && !paused => {
                paused = true;
                commands.send_modify(|control| control.paused = true);
            }
            "audit" | "release-audit" if synced == 1 && !audit_requested => {
                commands.send_modify(|control| control.reconcile += 1);
                audit_requested = true;
            }
            "interrupt" | "release-interrupt"
                if event.kind == EventKind::Chunking
                    && event.source_path.as_ref() == target.as_ref()
                    && !stopped =>
            {
                interrupted = true;
                stop(&commands, &mut stopped);
            }
            "cache-race" if event.kind == EventKind::UnavailableRoot && !stopped => {
                stop(&commands, &mut stopped);
            }
            _ => {}
        }
        if event.kind == EventKind::Paused && paused && !resumed {
            commands.send_modify(|control| control.paused = false);
            resumed = true;
        }
        if event.kind == EventKind::Synced && event.selection_id.as_deref() == Some(&selection.id) {
            let finish = match stage {
                "warm" => resumed && synced >= 4,
                "audit" | "release-audit" => audit_synced_after_hashing,
                "interrupt" | "release-interrupt" => false,
                _ => synced >= 1,
            };
            if finish && !stopped {
                stop(&commands, &mut stopped);
            }
        }
        if stopped && worker.is_finished() {
            break;
        }
        if stopped && matches!(stage, "initial" | "edit" | "repair" | "retry" | "audit") {
            break;
        }
    }
    tokio::time::timeout(Duration::from_secs(300), worker)
        .await
        .expect("worker did not stop after its test control")
        .unwrap()
        .unwrap();
    assert!(
        !matches!(stage, "interrupt" | "release-interrupt") || interrupted,
        "no unfinished file was reached"
    );
    let metrics_path = path_env("DRIP_SOURCE_READ_METRICS_PATH");
    test_metrics::write(&metrics_path).unwrap();
    let resources = test_metrics::process_resources().unwrap();
    let rows_before_exit =
        cache_rows(&path_env("DRIP_TEST_ARTIFACT_ROOT").join("drip.state.sqlite3"));
    let result = StageResult {
        stage: stage.to_owned(),
        elapsed_ms: elapsed_ms(started),
        first_sync_ms,
        event_counts,
        rows_before_exit,
        process_cpu_100ns: resources.cpu_100ns,
        peak_working_set_bytes: resources.peak_working_set_bytes,
        interrupted,
        target_file_checked,
        audit_requested,
        audit_hash_paths: audit_hash_paths.len(),
        audit_synced_after_hashing,
    };
    fs::write(
        path_env("DRIP_STAGE_RESULT_PATH"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}

fn stop(commands: &watch::Sender<RunControl>, stopped: &mut bool) {
    commands.send_modify(|control| control.stop = true);
    *stopped = true;
}

fn cache_rows(path: &std::path::Path) -> u64 {
    let database = rusqlite::Connection::open(path).unwrap();
    let count: i64 = database
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    u64::try_from(count).unwrap()
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn path_env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

fn event_key(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Scanning => "scanning",
        EventKind::UploadStarted => "uploadstarted",
        EventKind::UploadCompleted => "uploadcompleted",
        EventKind::FileChecked => "filechecked",
        EventKind::Hashing => "hashing",
        EventKind::Chunking => "chunking",
        EventKind::Uploading => "uploading",
        EventKind::Synced => "synced",
        EventKind::Paused => "paused",
        EventKind::Resumed => "resumed",
        _ => "other",
    }
}
