use std::{fs, path::PathBuf};

use crate::{
    credentials::Pairing,
    engine::cache::FileCache,
    settings::{SavedSelection, SettingsStore},
    source::{SourceFile, random_id},
};

pub(super) struct Fixture {
    pub directory: PathBuf,
    pub file: PathBuf,
    pub store: SettingsStore,
    pub selection: SavedSelection,
    pub pairing: Pairing,
}

impl Fixture {
    pub fn new() -> Self {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".verification/persistent-cache/cache-tests")
            .join(random_id().unwrap());
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("file.txt");
        fs::write(&file, b"old bytes").unwrap();
        let source = SourceFile::open(&file).unwrap();
        let pc_id = random_id().unwrap();
        Self {
            store: SettingsStore::open(&directory),
            directory,
            file,
            selection: SavedSelection {
                id: random_id().unwrap(),
                source: source.protocol_path().clone(),
                root_kind: drip_core::protocol::EntryKind::File,
                volume_identity: source.volume_identity().to_owned(),
                root_identity: None,
                active: true,
            },
            pairing: Pairing::new(
                "https://cache-test.example",
                pc_id.clone(),
                format!("mirror-{pc_id}"),
                "a".repeat(64),
            )
            .unwrap(),
        }
    }

    pub fn cache(&self) -> FileCache {
        FileCache::beside_settings(&self.store)
    }

    pub fn source(&self) -> SourceFile {
        SourceFile::open(&self.file).unwrap()
    }

    pub fn database(&self) -> PathBuf {
        self.directory.join("drip.state.sqlite3")
    }

    pub async fn seed(
        &self,
        cache: &FileCache,
        selection: &SavedSelection,
    ) -> drip_core::Blake3Digest {
        assert!(
            cache
                .reconcile(&self.pairing, std::slice::from_ref(selection))
                .await
                .is_none()
        );
        let source = self.source();
        let (events, _received) = tokio::sync::mpsc::channel(8);
        let digest = source.hash(&events).await.unwrap();
        assert!(
            cache
                .remember(
                    selection,
                    source.protocol_path(),
                    source.observation(),
                    source.volume_identity(),
                    digest,
                )
                .await
                .is_none()
        );
        digest
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
