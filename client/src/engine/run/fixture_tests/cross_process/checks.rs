use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use drip_core::SourcePath;
use serde_json::Value;

use super::FileMetrics;

pub fn verify_owned_fixture() {
    let container = env("DRIP_TEST_CONTAINER");
    let mirror_volume = env("DRIP_TEST_MIRROR_VOLUME");
    let expected_image = env("DRIP_TEST_SERVER_IMAGE_ID");
    let inspection: Value =
        serde_json::from_slice(&docker(&["inspect", &container]).stdout).unwrap();
    assert_eq!(
        inspection[0]["Config"]["Labels"]["drip.verification"],
        "persistent-cache"
    );
    assert_eq!(inspection[0]["Image"], expected_image);
    assert!(
        inspection[0]["Mounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mount| { mount["Name"] == mirror_volume && mount["Destination"] == "/mirror" })
    );
    assert!(
        inspection[0]["NetworkSettings"]["Networks"]
            .get("drip-persistent-cache-net")
            .is_some()
    );
}

pub fn assert_file_metrics(
    root: &Path,
    files: &BTreeMap<String, u64>,
    metrics: &BTreeMap<String, FileMetrics>,
    require_hashing: bool,
    require_upload: bool,
) {
    assert_eq!(metrics.len(), files.len());
    for (name, size) in files {
        let path = root.join(name);
        let key = SourcePath::try_from_path(&path).unwrap().to_windows_path();
        let item = metrics
            .get(&key)
            .unwrap_or_else(|| panic!("missing metrics for {name}"));
        assert_eq!(
            item.hashing_bytes,
            if require_hashing { *size } else { 0 },
            "{name}"
        );
        if require_upload {
            assert_eq!(item.chunking_bytes, *size, "{name}");
            assert_eq!(item.range_reread_bytes, *size, "{name}");
            assert!(item.upload_plan_requests > 0, "{name}");
            assert_eq!(item.upload_payload_bytes, *size, "{name}");
        } else {
            assert_eq!(item.chunking_bytes, 0, "{name}");
            assert_eq!(item.range_reread_bytes, 0, "{name}");
            assert_eq!(item.upload_plan_requests, 0, "{name}");
            assert_eq!(item.upload_payload_bytes, 0, "{name}");
        }
    }
}

pub fn assert_interrupted_metrics(
    metrics: &BTreeMap<String, FileMetrics>,
    target: &Path,
    size: u64,
) {
    let key = SourcePath::try_from_path(target).unwrap().to_windows_path();
    assert!(
        metrics.contains_key(&key),
        "interrupted source has no metrics"
    );
    for (path, item) in metrics {
        if path == &key {
            assert_eq!(item.hashing_bytes, size);
            assert!(item.chunking_bytes <= size);
            assert!(item.range_reread_bytes <= size);
            assert!(item.upload_payload_bytes <= size);
        } else {
            assert_eq!(item.hashing_bytes, 0, "unrelated interrupted hash: {path}");
            assert_eq!(
                item.chunking_bytes, 0,
                "unrelated interrupted chunk: {path}"
            );
            assert_eq!(
                item.range_reread_bytes, 0,
                "unrelated interrupted reread: {path}"
            );
            assert_eq!(
                item.upload_plan_requests, 0,
                "unrelated interrupted plan: {path}"
            );
            assert_eq!(
                item.upload_payload_bytes, 0,
                "unrelated interrupted payload: {path}"
            );
        }
    }
}

pub fn assert_no_content_work(metrics: &BTreeMap<String, FileMetrics>, target: &Path) {
    let key = SourcePath::try_from_path(target).unwrap().to_windows_path();
    assert!(metrics.contains_key(&key), "cached source has no metrics");
    for (path, item) in metrics {
        assert_eq!(item.hashing_bytes, 0, "cached hash read: {path}");
        assert_eq!(item.chunking_bytes, 0, "cached chunk read: {path}");
        assert_eq!(item.range_reread_bytes, 0, "cached range read: {path}");
        assert_eq!(item.upload_plan_requests, 0, "cached upload plan: {path}");
        assert_eq!(
            item.upload_payload_bytes, 0,
            "cached upload payload: {path}"
        );
    }
}

pub fn assert_only_file_changed(
    metrics: &BTreeMap<String, FileMetrics>,
    target: &Path,
    size: u64,
    require_hashing: bool,
) {
    let key = SourcePath::try_from_path(target).unwrap().to_windows_path();
    for (path, item) in metrics {
        if path == &key {
            assert_eq!(item.hashing_bytes, if require_hashing { size } else { 0 });
            assert_eq!(item.chunking_bytes, size);
            assert_eq!(item.range_reread_bytes, size);
            assert!(item.upload_plan_requests > 0);
            assert_eq!(item.upload_payload_bytes, size);
        } else {
            assert_eq!(item.hashing_bytes, 0, "unrelated hash read: {path}");
            assert_eq!(item.chunking_bytes, 0, "unrelated chunk read: {path}");
            assert_eq!(item.range_reread_bytes, 0, "unrelated range read: {path}");
            assert_eq!(item.upload_plan_requests, 0, "unrelated plan: {path}");
            assert_eq!(item.upload_payload_bytes, 0, "unrelated payload: {path}");
        }
    }
    assert!(metrics.contains_key(&key), "changed source has no metrics");
}

pub fn mirror_file_count(source_root: &Path) -> usize {
    let pairing = pairing();
    let root = remote_path(&pairing, source_root);
    let count = docker(&[
        "exec",
        &env("DRIP_TEST_CONTAINER"),
        "sh",
        "-c",
        &format!("find '{}' -type f | wc -l", root.replace('\'', "'\\''")),
    ]);
    String::from_utf8(count.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

pub fn corrupt_server_copy(source: &Path, bytes: &[u8]) {
    let pairing = pairing();
    let path = remote_path(&pairing, source);
    let mut process = Command::new("wsl")
        .args([
            "-d",
            "Docker",
            "--",
            "docker",
            "exec",
            "-i",
            &env("DRIP_TEST_CONTAINER"),
            "tee",
            &path,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "synthetic server edit failed");
}

pub fn assert_remote_file_matches(source: &Path) {
    let pairing = pairing();
    let path = remote_path(&pairing, source);
    let remote = docker(&["exec", &env("DRIP_TEST_CONTAINER"), "cat", &path]);
    assert!(remote.status.success(), "synthetic mirror file is missing");
    let local = fs::read(source).unwrap();
    let remote = remote.stdout;
    assert_eq!(blake3::hash(&local), blake3::hash(&remote));
}

fn pairing() -> crate::credentials::Pairing {
    crate::credentials::CredentialStore::beside_executable()
        .unwrap()
        .load()
        .unwrap()
}

fn remote_path(pairing: &crate::credentials::Pairing, source: &Path) -> String {
    let source = SourcePath::try_from_path(source).unwrap();
    format!(
        "/mirror/{}/{}/{}",
        pairing.mirror_directory(),
        source.drive(),
        source.components().join("/")
    )
}

fn docker(arguments: &[&str]) -> Output {
    let output = Command::new("wsl")
        .args(["-d", "Docker", "--", "docker"])
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "synthetic Docker fixture failed");
    output
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}
