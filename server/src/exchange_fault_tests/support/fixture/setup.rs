use axum::body::Body;
use drip_core::protocol::{
    DirectoryCreateRequest, Entry, EntryKind, EntryPrecondition, UploadPrepareRequest,
};
use drip_core::{Blake3Digest, SourcePath, Timestamp};

use super::{ABANDONED_FILE, Fixture, OLD_FILE, OLD_TREE_FILE};

impl Fixture {
    pub(super) fn create_directory(
        &self,
        source_path: SourcePath,
        expected_entry: Option<EntryPrecondition>,
    ) {
        self.store()
            .create_directory(
                &self.credential,
                DirectoryCreateRequest {
                    selection_id: self.selection_id.clone(),
                    generation: self.generation,
                    source_path,
                    expected_entry,
                },
            )
            .unwrap();
    }

    pub(super) async fn prepare_directory_to_file(&mut self) {
        self.create_directory(self.nested.clone(), None);
        self.create_directory(self.empty.clone(), None);
        let old_upload = self.upload(&self.old_tree_file, OLD_TREE_FILE, None).await;
        self.store()
            .commit_upload(&self.credential, &old_upload)
            .unwrap();
        let root = self.inventory_entry(&self.selection_root);
        assert_eq!(root.kind, EntryKind::Directory);
        self.abandoned_session = self
            .upload(
                &self.target,
                super::NEW_FILE,
                Some(EntryPrecondition {
                    kind: EntryKind::Directory,
                    revision: root.revision,
                }),
            )
            .await;
    }

    pub(super) async fn prepare_file_to_directory(&mut self) {
        let old_upload = self.upload(&self.target, OLD_FILE, None).await;
        self.store()
            .commit_upload(&self.credential, &old_upload)
            .unwrap();
        let target = self.inventory_entry(&self.target);
        assert_eq!(target.kind, EntryKind::File);
        self.abandoned_session = self
            .upload(
                &super::child(&self.selection_root, "abandoned.bin"),
                ABANDONED_FILE,
                None,
            )
            .await;
        self.directory_request = Some(DirectoryCreateRequest {
            selection_id: self.selection_id.clone(),
            generation: self.generation,
            source_path: self.target.clone(),
            expected_entry: Some(EntryPrecondition {
                kind: EntryKind::File,
                revision: target.revision,
            }),
        });
    }

    async fn upload(
        &self,
        source_path: &SourcePath,
        bytes: &[u8],
        expected_entry: Option<EntryPrecondition>,
    ) -> String {
        let prepared = self
            .store()
            .prepare_upload(
                &self.credential,
                UploadPrepareRequest {
                    selection_id: self.selection_id.clone(),
                    generation: self.generation,
                    source_path: source_path.clone(),
                    expected_size: bytes.len() as u64,
                    digest: Blake3Digest::new(*blake3::hash(bytes).as_bytes()),
                    last_write: Timestamp::new(1_700_000_000, 123_456_789).unwrap(),
                    expected_entry,
                },
            )
            .unwrap();
        self.store()
            .put_upload_data(
                &self.credential,
                &prepared.upload_id,
                Body::from(bytes.to_vec()),
            )
            .await
            .unwrap();
        prepared.upload_id
    }

    fn inventory_entry(&self, source_path: &SourcePath) -> Entry {
        self.store()
            .list_entries(&self.credential, &self.selection_id, None)
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| &entry.source_path == source_path)
            .unwrap()
    }
}
