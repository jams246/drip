use std::{collections::BTreeMap, path::Path};

use serde_json::Value;

use super::{
    checks::{
        assert_file_metrics, assert_interrupted_metrics, assert_only_file_changed,
        assert_remote_file_matches, corrupt_server_copy,
    },
    deterministic,
    process::{cache_contains_path, count, read_metrics, run_stage},
};

pub(super) fn run_release_scenarios(
    source_root: &Path,
    sizes: &BTreeMap<String, u64>,
    summary: &mut Value,
) {
    let variant = std::env::var("DRIP_PERSISTENCE_SCENARIO_VARIANT")
        .unwrap_or_else(|_| "standard".to_owned());
    let small = source_root.join("small-000.bin");
    let small_size = sizes["small-000.bin"];
    let edit_seed = format!("persistent-cache:release-edit:{variant}");
    deterministic::write_file(&small, small_size, &edit_seed, false);
    let edited = run_stage("release-edit", None);
    let edited_metrics = read_metrics("release-edit");
    assert_eq!(count(&edited, "synced"), 1);
    assert_only_file_changed(&edited_metrics, &small, small_size, true);
    assert_remote_file_matches(&small);

    corrupt_server_copy(&small, b"release-server-only-change");
    let repair = run_stage("release-repair", None);
    let repair_metrics = read_metrics("release-repair");
    assert_eq!(count(&repair, "uploadcompleted"), 1);
    assert_only_file_changed(&repair_metrics, &small, small_size, false);
    assert_remote_file_matches(&small);

    let interrupted_file = source_root.join("large-01.bin");
    let interrupted_size = sizes["large-01.bin"];
    deterministic::write_file(
        &interrupted_file,
        interrupted_size,
        &format!("persistent-cache:release-interrupted-upload:{variant}"),
        false,
    );
    let interrupted = run_stage("release-interrupt", Some(&interrupted_file));
    assert!(
        interrupted.interrupted,
        "no release file upload was interrupted"
    );
    assert_eq!(interrupted.rows_before_exit, sizes.len() as u64 - 1);
    assert!(!cache_contains_path(&interrupted_file));
    assert_interrupted_metrics(
        &read_metrics("release-interrupt"),
        &interrupted_file,
        interrupted_size,
    );

    let retry = run_stage("release-retry", Some(&interrupted_file));
    assert_eq!(count(&retry, "uploadcompleted"), 1);
    assert_eq!(retry.rows_before_exit, sizes.len() as u64);
    let retry_metrics = read_metrics("release-retry");
    assert_only_file_changed(&retry_metrics, &interrupted_file, interrupted_size, true);

    let audit = run_stage("audit", None);
    let audit_metrics = read_metrics("audit");
    assert!(audit.audit_requested);
    assert_eq!(audit.audit_hash_paths, sizes.len());
    assert!(audit.audit_synced_after_hashing);
    assert_eq!(audit.rows_before_exit, sizes.len() as u64);
    assert_file_metrics(source_root, sizes, &audit_metrics, true, false);
    summary["release_stopped_edit"] = serde_json::json!({
        "result": edited,
        "metrics": edited_metrics
    });
    summary["release_server_only_repair"] = serde_json::json!({
        "result": repair,
        "metrics": repair_metrics
    });
    summary["release_interrupted_upload"] = serde_json::json!({
        "result": interrupted,
        "metrics": read_metrics("release-interrupt")
    });
    summary["release_interrupted_retry"] = serde_json::json!({
        "result": retry,
        "metrics": retry_metrics
    });
    summary["release_explicit_audit"] = serde_json::json!({
        "result": audit,
        "metrics": audit_metrics
    });
    summary["release_scenario_variant"] = serde_json::Value::String(variant);
}
