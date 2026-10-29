use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Output},
};

use drip_client::credentials::Pairing;
use drip_core::{Blake3Digest, SourcePath};

#[path = "../common/deterministic.rs"]
mod deterministic;

pub const GENERATOR: &str = deterministic::GENERATOR;
pub const SEED: &str = deterministic::SEED;
const FULL_SIZE: u64 = 10 * 1024 * 1024 * 1024;

pub struct Fixture {
    pub server: String,
    pub source: PathBuf,
    pub size: u64,
    container: String,
    mirror_volume: String,
    hash_image: String,
    traffic_file: PathBuf,
    run_id: String,
}

impl Fixture {
    pub fn prepare() -> Self {
        let root = std::env::var_os("DRIP_TEST_ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join(".verification/phase-03")
            });
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert!(executable.starts_with(root.canonicalize().unwrap()));
        let server = std::env::var("DRIP_TEST_SERVER_URL").expect("DRIP_TEST_SERVER_URL required");
        let traffic_file = std::env::var_os("DRIP_TEST_TRAFFIC_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("proxy-traffic.jsonl"));
        assert!(traffic_file.starts_with(&root));
        let url = url::Url::parse(&server).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]")
        ));
        let size = std::env::var("DRIP_TEST_SIZE_BYTES")
            .map(|value| value.parse::<u64>().unwrap())
            .unwrap_or(FULL_SIZE);
        assert!(matches!(size, FULL_SIZE | 67_108_864));
        let container = std::env::var("DRIP_TEST_CONTAINER").unwrap_or("drip-phase02".into());
        let mirror_volume =
            std::env::var("DRIP_TEST_MIRROR_VOLUME").unwrap_or("drip-phase02-mirror".into());
        let inspection: serde_json::Value =
            serde_json::from_slice(&docker(&["inspect", &container]).stdout).unwrap();
        let label = inspection[0]["Config"]["Labels"]["drip.verification"].as_str();
        assert!(
            matches!(label, Some("phase02" | "phase03")),
            "not an owned fixture"
        );
        assert!(
            inspection[0]["Mounts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|mount| {
                    mount["Name"] == mirror_volume && mount["Destination"] == "/mirror"
                })
        );
        let hash_image = docker(&["image", "inspect", "drip:hash", "--format={{.Id}}"]);
        let hash_image = String::from_utf8(hash_image.stdout)
            .unwrap()
            .trim()
            .to_owned();
        assert!(hash_image.starts_with("sha256:"));
        let directory = root.join("large-fixture");
        fs::create_dir_all(&directory).unwrap();
        let run_id = format!("large-delta-{size}-{}", std::process::id());
        fs::write(
            root.join("proxy-control.json"),
            serde_json::to_vec(&serde_json::json!({"run_id": run_id})).unwrap(),
        )
        .unwrap();
        Self {
            server,
            source: directory.join(format!("fixture-{size}.bin")),
            size,
            container,
            mirror_volume,
            hash_image,
            traffic_file,
            run_id,
        }
    }

    pub fn token(&self) -> String {
        let output = docker(&[
            "exec",
            &self.container,
            "/usr/local/bin/server",
            "token",
            "create",
        ]);
        let token = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        assert!(token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        token
    }

    pub fn traffic(&self) -> (u64, u64, u64) {
        let traffic = BufReader::new(File::open(&self.traffic_file).unwrap());
        let mut payload = 0;
        let mut metadata = 0;
        let mut plans = 0;
        for line in traffic.lines() {
            let value: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            if value["run_id"] == self.run_id && value["event"] == "traffic" {
                payload += value["file_payload"].as_u64().unwrap();
                metadata += value["request_metadata"].as_u64().unwrap()
                    + value["response_metadata"].as_u64().unwrap()
                    + value["request_header_bytes"].as_u64().unwrap()
                    + value["response_header_bytes"].as_u64().unwrap();
                plans += u64::from(value["route"] == "plan");
            }
        }
        (payload, metadata, plans)
    }

    pub fn memory(&self) -> serde_json::Value {
        let command = format!(
            "$p=Get-Process -Id {}; [ordered]@{{peak_working_set_bytes=$p.PeakWorkingSet64; \
             private_bytes=$p.PrivateMemorySize64}} | ConvertTo-Json -Compress",
            std::process::id()
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .output()
            .unwrap();
        assert!(output.status.success());
        let client: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let status = docker(&["exec", &self.container, "cat", "/proc/1/status"]);
        let status = String::from_utf8(status.stdout).unwrap();
        let peak_kib = status
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmHWM:").map(|value| {
                    value
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                })
            })
            .unwrap();
        serde_json::json!({"client": client, "server_peak_rss_bytes": peak_kib * 1024,
            "scope": "whole baseline and delta procedure; operating system page cache excluded"})
    }

    pub fn generate(&self, insertion: bool) -> Blake3Digest {
        deterministic::write_file(&self.source, self.size, SEED, insertion)
    }

    pub fn require_mirror(&self, pairing: &Pairing, digest: Blake3Digest, size: u64) {
        let source = SourcePath::try_from_path(&self.source).unwrap();
        let mirror = format!(
            "/mirror/{}/{}/{}",
            pairing.mirror_directory(),
            source.drive(),
            source.components().join("/")
        );
        let mount = format!(
            "type=volume,source={},target=/mirror,readonly",
            self.mirror_volume
        );
        let output = docker(&["run", "--rm", "--mount", &mount, &self.hash_image, &mirror]);
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            actual.split_whitespace().next(),
            Some(digest.to_hex().as_str())
        );
        let output = docker(&[
            "run",
            "--rm",
            "--mount",
            &mount,
            "--entrypoint",
            "stat",
            &self.hash_image,
            "--format=%s",
            &mirror,
        ]);
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .parse::<u64>()
                .unwrap(),
            size
        );
    }
}

fn docker(arguments: &[&str]) -> Output {
    let mut command = Command::new("wsl.exe");
    command.args(["-d", "Docker", "--", "docker", arguments[0]]);
    if arguments[0] == "run" {
        command.arg("--pull=never");
    }
    let output = command.args(&arguments[1..]).output().unwrap();
    assert!(
        output.status.success(),
        "synthetic Docker fixture command failed"
    );
    output
}
