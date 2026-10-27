#[path = "fixture/assertions.rs"]
mod assertions;
#[path = "fixture/setup.rs"]
mod setup;

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use drip_core::SourcePath;

use crate::{Store, StoreError};

use super::PanicAtCheckpoint;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
pub(super) const OLD_FILE: &[u8] = b"old file payload";
pub(super) const OLD_TREE_FILE: &[u8] = b"old nonempty tree payload";
pub(super) const NEW_FILE: &[u8] = b"new verified file payload";
pub(super) const ABANDONED_FILE: &[u8] = b"abandoned staged upload";

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    DirectoryToFile,
    FileToDirectory,
}

impl Direction {
    pub(crate) const ALL: [Self; 2] = [Self::DirectoryToFile, Self::FileToDirectory];
}

#[derive(Clone, Copy)]
pub(crate) enum FaultPoint {
    BeforeExchange,
    AfterExchange,
    BeforeCleanup,
}

impl FaultPoint {
    pub(crate) const ALL: [Self; 3] = [
        Self::BeforeExchange,
        Self::AfterExchange,
        Self::BeforeCleanup,
    ];

    fn checkpoint(self) -> &'static str {
        match self {
            Self::BeforeExchange => "before_type_exchange",
            Self::AfterExchange => "after_type_exchange",
            Self::BeforeCleanup => "before_retired_cleanup",
        }
    }

    fn new_entry_is_visible(self) -> bool {
        !matches!(self, Self::BeforeExchange)
    }
}

pub(crate) struct Fixture {
    pub(super) direction: Direction,
    root: PathBuf,
    state_dir: PathBuf,
    mirror_dir: PathBuf,
    device_directory: String,
    store: Option<Store>,
    credential: String,
    selection_id: String,
    generation: u64,
    selection_root: SourcePath,
    target: SourcePath,
    nested: SourcePath,
    empty: SourcePath,
    old_tree_file: SourcePath,
    abandoned_session: String,
    directory_request: Option<drip_core::protocol::DirectoryCreateRequest>,
}

impl Fixture {
    pub(crate) async fn new(direction: Direction) -> Self {
        let root = fixture_root();
        let state_dir = root.join("state");
        let mirror_dir = root.join("mirror");
        let store = Store::open(&state_dir, &mirror_dir).unwrap();
        let token = store.create_enrollment_token().unwrap();
        let device = store
            .enroll(drip_core::protocol::EnrollmentRequest {
                token: drip_core::SensitiveString::new(token),
                label: "Exchange fault".to_owned(),
            })
            .unwrap();
        let selection_id = "e".repeat(32);
        let selection_root = SourcePath::new('C', vec!["selected".to_owned()]).unwrap();
        let target = match direction {
            Direction::DirectoryToFile => selection_root.clone(),
            Direction::FileToDirectory => child(&selection_root, "entry.bin"),
        };
        let nested = child(&selection_root, "nested");
        let empty = child(&selection_root, "empty");
        let old_tree_file = child(&nested, "old.bin");
        let credential = device.device_credential.expose_secret().to_owned();
        let generation = store
            .update_selections(
                &credential,
                drip_core::protocol::SelectionUpdate {
                    expected_generation: 0,
                    selections: vec![drip_core::protocol::Selection {
                        id: selection_id.clone(),
                        source: selection_root.clone(),
                        root_kind: drip_core::protocol::EntryKind::Directory,
                        active: true,
                    }],
                },
            )
            .unwrap()
            .generation;
        let mut fixture = Self {
            direction,
            root,
            state_dir,
            mirror_dir,
            device_directory: device.mirror_directory,
            store: Some(store),
            credential,
            selection_id,
            generation,
            selection_root,
            target,
            nested,
            empty,
            old_tree_file,
            abandoned_session: String::new(),
            directory_request: None,
        };
        fixture.create_directory(fixture.selection_root.clone(), None);
        match direction {
            Direction::DirectoryToFile => fixture.prepare_directory_to_file().await,
            Direction::FileToDirectory => fixture.prepare_file_to_directory().await,
        }
        fixture
    }

    pub(crate) fn interrupt(&self, point: FaultPoint) {
        let store = self.store().clone();
        let credential = self.credential.clone();
        let hook = PanicAtCheckpoint::new(point.checkpoint());
        match self.direction {
            Direction::DirectoryToFile => {
                let upload_id = self.abandoned_session.clone();
                hook.interrupt(move || {
                    let _ = store.commit_upload(&credential, &upload_id);
                });
            }
            Direction::FileToDirectory => {
                let request = self.directory_request.clone().unwrap();
                hook.interrupt(move || {
                    let _ = store.create_directory(&credential, request);
                });
            }
        }
    }

    pub(crate) fn restart(&mut self) {
        drop(self.store.take());
        let store = Store::open(&self.state_dir, &self.mirror_dir).unwrap();
        store.prepare_daemon().unwrap();
        assert!(
            fs::read_dir(self.mirror_dir.join(".drip-staging"))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(matches!(
            store.get_upload_state(&self.credential, &self.abandoned_session),
            Err(StoreError::NotFound)
        ));
        self.store = Some(store);
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture_root() -> PathBuf {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".verification/phase-02/server-tests");
    fs::create_dir_all(&parent).unwrap();
    let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = parent.join(format!("{}-exchange-fault-{suffix}", std::process::id()));
    fs::create_dir(&root).unwrap();
    root
}

fn child(parent: &SourcePath, name: &str) -> SourcePath {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).unwrap()
}
