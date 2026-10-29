use std::{collections::BTreeMap, fs, path::Path};

#[path = "checks.rs"]
mod checks;
mod process;

use serde::{Deserialize, Serialize};

use super::deterministic;
use checks::{
    assert_file_metrics, assert_interrupted_metrics, assert_no_content_work,
    assert_only_file_changed, assert_remote_file_matches, corrupt_server_copy, mirror_file_count,
    verify_owned_fixture,
};
use process::{
    cache_contains_path, cache_digest, cache_row_count, count, create_dataset, path_env,
    read_metrics, run_cache_race_exit, run_commit_exit, run_precommit_exit, run_stage,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct FileMetrics {
    hashing_bytes: u64,
    chunking_bytes: u64,
    range_reread_bytes: u64,
    upload_plan_requests: u64,
    upload_payload_bytes: u64,
}

pub async fn run() {
    verify_owned_fixture();
    let root = path_env("DRIP_TEST_ARTIFACT_ROOT");
    let source_root = path_env("DRIP_PERSISTENCE_SOURCE_ROOT");
    assert!(std::env::current_exe().unwrap().starts_with(&root));
    let profile = std::env::var("DRIP_PERSISTENCE_DATASET").unwrap();
    let sizes = create_dataset(&profile, &source_root);
    let initial = run_stage("initial", None);
    let initial_metrics = read_metrics("initial");
    assert_eq!(initial.rows_before_exit, sizes.len() as u64);
    assert_eq!(count(&initial, "synced"), 1);
    assert_eq!(count(&initial, "uploadcompleted"), sizes.len() as u64);
    assert_file_metrics(&source_root, &sizes, &initial_metrics, true, true);
    assert_eq!(mirror_file_count(&source_root), sizes.len());
    let warm = run_stage("warm", None);
    let warm_metrics = read_metrics("warm");
    assert_eq!(warm.rows_before_exit, sizes.len() as u64);
    assert!(count(&warm, "synced") >= 4);
    assert_eq!(count(&warm, "paused"), 2);
    assert_eq!(count(&warm, "resumed"), 1);
    assert!(count(&warm, "scanning") >= 4);
    assert_file_metrics(&source_root, &sizes, &warm_metrics, false, false);
    let db_size = fs::metadata(root.join("drip.state.sqlite3")).unwrap().len();
    let mut summary = serde_json::json!({
        "profile": profile,
        "generator": deterministic::GENERATOR,
        "file_count": sizes.len(),
        "dataset_bytes": sizes.values().sum::<u64>(),
        "database_bytes": db_size,
        "initial": initial,
        "warm_restart": warm,
        "initial_metrics": initial_metrics,
        "warm_metrics": warm_metrics
    });
    if profile == "routine" {
        run_routine_scenarios(&source_root, &sizes, &mut summary);
    }
    let output = root.join(format!("acceptance-{profile}.json"));
    fs::write(output, serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
}

fn run_routine_scenarios(
    source_root: &Path,
    sizes: &BTreeMap<String, u64>,
    summary: &mut serde_json::Value,
) {
    let small = source_root.join("small-000.bin");
    let small_size = sizes["small-000.bin"];
    deterministic::write_file(&small, small_size, "persistent-cache:edit", false);
    let edited = run_stage("edit", None);
    let edited_metrics = read_metrics("edit");
    assert_only_file_changed(&edited_metrics, &small, small_size, true);
    assert_remote_file_matches(&small);

    corrupt_server_copy(&small, b"server-only-change");
    let repair = run_stage("repair", None);
    let repair_metrics = read_metrics("repair");
    assert_only_file_changed(&repair_metrics, &small, small_size, false);
    assert_remote_file_matches(&small);

    let interrupted_file = source_root.join("large-16.bin");
    let interrupted_size = sizes["large-16.bin"];
    deterministic::write_file(
        &interrupted_file,
        interrupted_size,
        "persistent-cache:interrupted-upload",
        false,
    );
    let interrupted = run_stage("interrupt", Some(&interrupted_file));
    assert!(interrupted.interrupted, "no active upload was interrupted");
    assert_eq!(interrupted.rows_before_exit, sizes.len() as u64 - 1);
    assert!(!cache_contains_path(&interrupted_file));
    assert_interrupted_metrics(
        &read_metrics("interrupt"),
        &interrupted_file,
        interrupted_size,
    );
    let retry = run_stage("retry", None);
    let retry_metrics = read_metrics("retry");
    assert_only_file_changed(&retry_metrics, &interrupted_file, interrupted_size, true);
    assert_remote_file_matches(&interrupted_file);

    let audit = run_stage("audit", None);
    let audit_metrics = read_metrics("audit");
    assert_file_metrics(source_root, sizes, &audit_metrics, true, false);
    let cached_digest = cache_digest(&small).expect("completed audit did not persist the file");
    run_cache_race_exit(&small);
    assert_eq!(cache_row_count(), sizes.len() as u64);
    assert_eq!(cache_digest(&small), Some(cached_digest));
    assert_ne!(
        cached_digest,
        *blake3::hash(&fs::read(&small).unwrap()).as_bytes()
    );
    let cache_race_metrics = read_metrics("cache-race");
    assert_no_content_work(&cache_race_metrics, &small);

    let cache_race_retry = run_stage("cache-race-retry", Some(&small));
    assert_eq!(count(&cache_race_retry, "uploadcompleted"), 1);
    let cache_race_retry_metrics = read_metrics("cache-race-retry");
    assert_only_file_changed(&cache_race_retry_metrics, &small, small_size, true);
    assert_remote_file_matches(&small);

    let precommit_file = source_root.join("small-002.bin");
    let precommit_size = sizes["small-002.bin"];
    deterministic::write_file(
        &precommit_file,
        precommit_size,
        "persistent-cache:transaction-before-commit",
        false,
    );
    run_precommit_exit(&precommit_file);
    assert_eq!(cache_row_count(), sizes.len() as u64 - 1);
    assert!(!cache_contains_path(&precommit_file));
    let precommit_retry = run_stage("transaction-exit-retry", Some(&precommit_file));
    assert!(precommit_retry.target_file_checked);
    let precommit_metrics = read_metrics("transaction-exit-retry");
    assert_only_file_hashed(&precommit_metrics, &precommit_file, precommit_size);
    assert_remote_file_matches(&precommit_file);

    let committed_file = source_root.join("small-001.bin");
    let committed_size = sizes["small-001.bin"];
    deterministic::write_file(
        &committed_file,
        committed_size,
        "persistent-cache:commit-before-exit",
        false,
    );
    run_commit_exit(&committed_file);
    let committed_digest = cache_digest(&committed_file).expect("commit checkpoint lost its row");
    assert_eq!(
        committed_digest,
        *blake3::hash(&fs::read(&committed_file).unwrap()).as_bytes()
    );
    let committed_retry = run_stage("commit-exit-retry", Some(&committed_file));
    let committed_metrics = read_metrics("commit-exit-retry");
    assert!(committed_retry.target_file_checked);
    assert_eq!(committed_retry.rows_before_exit, sizes.len() as u64);
    assert_file_metrics(source_root, sizes, &committed_metrics, false, false);
    summary["edited_file"] = serde_json::json!({"result": edited, "metrics": edited_metrics});
    summary["server_only_repair"] =
        serde_json::json!({"result": repair, "metrics": repair_metrics});
    summary["interrupted_upload"] = serde_json::json!({"result": interrupted});
    summary["interrupted_retry"] = serde_json::json!({"result": retry, "metrics": retry_metrics});
    summary["explicit_audit"] = serde_json::json!({"result": audit, "metrics": audit_metrics});
    summary["cached_verification_race"] = serde_json::json!({
        "child_exit_code": 85,
        "metrics": cache_race_metrics,
        "retry": cache_race_retry,
        "retry_metrics": cache_race_retry_metrics
    });
    summary["commit_before_process_exit"] = serde_json::json!({
        "target": committed_file,
        "reopened": committed_retry,
        "metrics": committed_metrics
    });
    summary["transaction_before_commit_exit"] = serde_json::json!({
        "target": precommit_file,
        "rows_after_exit": cache_row_count(),
        "retry": precommit_retry,
        "retry_metrics": precommit_metrics
    });
}

fn assert_only_file_hashed(metrics: &BTreeMap<String, FileMetrics>, target: &Path, size: u64) {
    let key = drip_core::SourcePath::try_from_path(target)
        .unwrap()
        .to_windows_path();
    assert!(metrics.contains_key(&key), "changed source has no metrics");
    for (path, item) in metrics {
        if path == &key {
            assert_eq!(item.hashing_bytes, size);
        } else {
            assert_eq!(item.hashing_bytes, 0, "unrelated hash read: {path}");
        }
        assert_eq!(item.chunking_bytes, 0, "unexpected chunk read: {path}");
        assert_eq!(item.range_reread_bytes, 0, "unexpected range read: {path}");
        assert_eq!(
            item.upload_plan_requests, 0,
            "unexpected upload plan: {path}"
        );
        assert_eq!(item.upload_payload_bytes, 0, "unexpected payload: {path}");
    }
}
