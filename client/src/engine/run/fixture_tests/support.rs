use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use tokio::sync::{mpsc, watch};

use crate::{
    credentials::Pairing,
    engine::{
        Engine, EventSink, LifecycleEvent, RunControl,
        cache::FileCache,
        pass::PassContext,
        run::setup::PassState,
        watcher::{ChangeEpoch, DirtySignal},
    },
    transport::HttpTransport,
};

mod failure;
mod server;

pub(super) struct Fixture {
    pub engine: Engine,
    pub sources: PathBuf,
    pub signal: DirtySignal,
    pairing: Pairing,
    transport: HttpTransport,
    cache: FileCache,
    events: EventSink,
    received: mpsc::Receiver<LifecycleEvent>,
    control: watch::Receiver<RunControl>,
    _commands: watch::Sender<RunControl>,
    state: PassState,
    barrier: bool,
}

impl Fixture {
    pub fn restart_worker(&mut self) {
        self.state = PassState::default();
        self.cache = FileCache::beside_settings(&self.engine.settings);
        self.signal.overflow();
        self.barrier = true;
    }

    pub async fn new() -> Self {
        let root = std::env::var_os("DRIP_TEST_ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join(".verification/watch-isolation")
            });
        assert!(
            std::env::current_exe()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(root.canonicalize().unwrap())
        );
        let url = std::env::var("DRIP_TEST_SERVER_URL").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert!(matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")));
        let token_path = std::env::var_os("DRIP_TEST_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("enrollment-token.txt"));
        let token = fs::read_to_string(token_path).unwrap();
        let engine = Engine::beside_executable().unwrap();
        let pairing = engine
            .pair(&url, "Watch isolation fixture", token.trim())
            .await
            .unwrap();
        let transport = HttpTransport::new(&url).unwrap();
        let (send, received) = mpsc::channel(4096);
        let (progress, _) = watch::channel(None);
        let events = EventSink::new(send, progress);
        let (commands, control) = watch::channel(RunControl::default());
        let sources = root.join(crate::source::random_id().unwrap());
        fs::create_dir_all(&sources).unwrap();
        let cache = FileCache::beside_settings(&engine.settings);
        Self {
            engine,
            sources,
            signal: DirtySignal::default(),
            pairing,
            transport,
            cache,
            events,
            received,
            control,
            _commands: commands,
            state: PassState::default(),
            barrier: true,
        }
    }

    pub async fn attempt(&mut self, all: bool, force: bool) -> (bool, Vec<LifecycleEvent>) {
        if force {
            self.signal.audit();
        } else if all {
            self.signal.sweep();
        }
        let context = PassContext {
            cache: &self.cache,
            store: &self.engine.settings,
            transport: &self.transport,
            pairing: &self.pairing,
            events: &self.events,
            signal: &self.signal,
            control: &self.control,
        };
        let successful = self
            .engine
            .continuous_pass(&context, &mut self.state, self.barrier)
            .await
            .unwrap();
        self.barrier = false;
        let mut events = Vec::new();
        while let Ok(event) = self.received.try_recv() {
            events.push(event);
        }
        (successful, events)
    }

    pub async fn pass(&mut self, all: bool, force: bool) -> Vec<LifecycleEvent> {
        let mut events = Vec::new();
        for attempt in 0..8 {
            let (successful, next) = self
                .attempt(all && attempt == 0, force && attempt == 0)
                .await;
            events.extend(next);
            tokio::time::sleep(Duration::from_millis(100)).await;
            if successful && !self.signal.pending() {
                return events;
            }
        }
        panic!(
            "synthetic fixture did not settle: {}",
            self.failure_summary(&events)
        );
    }

    pub async fn changed(&self, id: &str, epoch: ChangeEpoch) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !self.signal.changed_since(id, epoch) {
                self.signal.changed().await;
            }
        })
        .await
        .expect("native watcher did not report the source change");
    }

    pub fn change_server(&self, source: &Path) {
        let container = test_container();
        let label = docker(&[
            "inspect",
            &container,
            "--format",
            "{{index .Config.Labels \"drip.verification\"}}",
        ]);
        assert_eq!(label.trim(), test_label());
        let path = self.remote_path(source);
        use std::{io::Write, process::Stdio};
        let mut process = Command::new("wsl")
            .args([
                "-d", "Docker", "--", "docker", "exec", "-i", &container, "tee", &path,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        process
            .stdin
            .take()
            .unwrap()
            .write_all(b"server-change")
            .unwrap();
        let result = process.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

fn docker(args: &[&str]) -> String {
    let output = Command::new("wsl")
        .args(["-d", "Docker", "--", "docker"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

pub(super) fn test_container() -> String {
    std::env::var("DRIP_TEST_CONTAINER")
        .unwrap_or_else(|_| "drip-watch-isolation-server".to_owned())
}

fn test_label() -> String {
    std::env::var("DRIP_TEST_LABEL").unwrap_or_else(|_| "watch-isolation".to_owned())
}
