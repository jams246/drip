use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde::{Deserialize, Serialize};

use super::super::deterministic;
use super::FileMetrics;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct StageResult {
    pub stage: String,
    pub elapsed_ms: u64,
    pub first_sync_ms: Option<u64>,
    pub event_counts: BTreeMap<String, u64>,
    pub rows_before_exit: u64,
    pub process_cpu_100ns: u64,
    pub peak_working_set_bytes: u64,
    pub interrupted: bool,
    pub target_file_checked: bool,
    pub audit_requested: bool,
    pub audit_hash_paths: usize,
    pub audit_synced_after_hashing: bool,
}

pub(super) fn run_stage(stage: &str, target: Option<&Path>) -> StageResult {
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let stage_dir = root.join("stages");
    fs::create_dir_all(&stage_dir).unwrap();
    let result = stage_dir.join(format!("{stage}-result.json"));
    let output = worker_command(stage, target).output().unwrap();
    write_log(&stage_dir, stage, &output);
    assert!(
        output.status.success(),
        "worker stage {stage} failed: {}",
        String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
    );
    serde_json::from_slice(&fs::read(result).unwrap()).unwrap()
}

pub(super) fn run_commit_exit(target: &Path) {
    run_expected_exit("commit-exit", target, 86);
}

pub(super) fn run_precommit_exit(target: &Path) {
    run_expected_exit("transaction-exit-before", target, 87);
}

pub(super) fn run_cache_race_exit(target: &Path) {
    run_expected_exit("cache-race", target, 85);
}

pub(super) fn cache_row_count() -> u64 {
    let database =
        rusqlite::Connection::open(path_env("DRIP_TEST_ARTIFACT_ROOT").join("drip.state.sqlite3"))
            .unwrap();
    let count: i64 = database
        .query_row("SELECT count(*) FROM cached_files", [], |row| row.get(0))
        .unwrap();
    u64::try_from(count).unwrap()
}

fn run_expected_exit(stage: &str, target: &Path, code: i32) {
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let stage_dir = root.join("stages");
    fs::create_dir_all(&stage_dir).unwrap();
    let output = worker_command(stage, Some(target)).output().unwrap();
    write_log(&stage_dir, stage, &output);
    assert_eq!(output.status.code(), Some(code));
    assert!(!stage_dir.join(format!("{stage}-result.json")).exists());
}

pub(super) fn create_dataset(profile: &str, root: &Path) -> BTreeMap<String, u64> {
    assert!(!root.exists(), "synthetic source root already exists");
    fs::create_dir_all(root).unwrap();
    let files = dataset_sizes(profile);
    for (name, size) in &files {
        let seed = format!("{}:{profile}:{name}", deterministic::SEED);
        deterministic::write_file(&root.join(name), *size, &seed, false);
    }
    files
}

pub(super) fn dataset_sizes(profile: &str) -> BTreeMap<String, u64> {
    let mut files = BTreeMap::new();
    for index in 0..200 {
        files.insert(format!("small-{index:03}.bin"), 64 * 1024);
    }
    let large: &[(&str, u64)] = match profile {
        "routine" => &[
            ("large-04.bin", 4),
            ("large-08.bin", 8),
            ("large-16.bin", 16),
        ],
        "release" => &[
            ("large-01.bin", 1024),
            ("large-02.bin", 2048),
            ("large-10.bin", 10_240),
        ],
        _ => panic!("dataset profile must be routine or release"),
    };
    for (name, mebibytes) in large {
        files.insert((*name).to_owned(), mebibytes * 1024 * 1024);
    }
    files
}

pub(super) fn read_metrics(stage: &str) -> BTreeMap<String, FileMetrics> {
    let path = path_env("DRIP_TEST_ARTIFACT_ROOT")
        .join("stages")
        .join(format!("{stage}-metrics.json"));
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

pub(super) fn cache_contains_path(path: &Path) -> bool {
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let source = drip_core::SourcePath::try_from_path(path).unwrap();
    let source_json = serde_json::to_string(&source).unwrap();
    let connection = rusqlite::Connection::open(root.join("drip.state.sqlite3")).unwrap();
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cached_files WHERE source_json = ?1)",
            [source_json],
            |row| row.get(0),
        )
        .unwrap()
}

pub(super) fn cache_digest(path: &Path) -> Option<[u8; 32]> {
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let source = drip_core::SourcePath::try_from_path(path).unwrap();
    let source_json = serde_json::to_string(&source).unwrap();
    let connection = rusqlite::Connection::open(root.join("drip.state.sqlite3")).unwrap();
    let bytes: Vec<u8> = connection
        .query_row(
            "SELECT digest FROM cached_files WHERE source_json = ?1",
            [source_json],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .ok()?;
    bytes.try_into().ok()
}

pub(super) fn path_env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

pub(super) fn count(result: &StageResult, key: &str) -> u64 {
    result.event_counts.get(key).copied().unwrap_or_default()
}

fn worker_command(stage: &str, target: Option<&Path>) -> Command {
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let stage_dir = root.join("stages");
    fs::create_dir_all(&stage_dir).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "engine::run::fixture_tests::cross_process::persistent_cache_process_restart_acceptance",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("DRIP_PERSISTENCE_STAGE", stage)
        .env(
            "DRIP_SOURCE_READ_METRICS_PATH",
            stage_dir.join(format!("{stage}-metrics.json")),
        )
        .env(
            "DRIP_STAGE_RESULT_PATH",
            stage_dir.join(format!("{stage}-result.json")),
        );
    if stage == "warm" {
        command.env("DRIP_TEST_OVERFLOW_AFTER_SYNC", "1");
    } else {
        command.env_remove("DRIP_TEST_OVERFLOW_AFTER_SYNC");
    }
    if let Some(path) = target {
        command.env("DRIP_PERSISTENCE_INTERRUPT_FILE", path);
        match stage {
            "cache-race" => {
                command.env("DRIP_TEST_MUTATE_AFTER_CACHE_LOOKUP", path);
            }
            "commit-exit" => {
                command.env("DRIP_TEST_EXIT_AFTER_CACHE_COMMIT", path);
            }
            "transaction-exit-before" => {
                command.env("DRIP_TEST_EXIT_BEFORE_CACHE_COMMIT", path);
            }
            _ => {
                command.env_remove("DRIP_TEST_MUTATE_AFTER_CACHE_LOOKUP");
                command.env_remove("DRIP_TEST_EXIT_AFTER_CACHE_COMMIT");
                command.env_remove("DRIP_TEST_EXIT_BEFORE_CACHE_COMMIT");
            }
        };
    } else {
        command.env_remove("DRIP_PERSISTENCE_INTERRUPT_FILE");
        command.env_remove("DRIP_TEST_MUTATE_AFTER_CACHE_LOOKUP");
        command.env_remove("DRIP_TEST_EXIT_AFTER_CACHE_COMMIT");
        command.env_remove("DRIP_TEST_EXIT_BEFORE_CACHE_COMMIT");
    }
    command
}

fn write_log(stage_dir: &Path, stage: &str, output: &Output) {
    fs::write(
        stage_dir.join(format!("{stage}.log")),
        [output.stdout.as_slice(), output.stderr.as_slice()].concat(),
    )
    .unwrap();
}
