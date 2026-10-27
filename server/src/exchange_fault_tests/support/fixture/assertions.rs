use std::{
    fs,
    path::{Path, PathBuf},
};

use drip_core::protocol::{Entry, EntryKind};
use drip_core::{Blake3Digest, SourcePath};

use super::{Direction, FaultPoint, Fixture, NEW_FILE, OLD_FILE, OLD_TREE_FILE};

impl Fixture {
    pub(crate) fn assert_filesystem(&self, point: FaultPoint) {
        let target = self.mirror_path(&self.target);
        match (self.direction, point.new_entry_is_visible()) {
            (Direction::DirectoryToFile, false) => self.assert_old_tree(&target),
            (Direction::DirectoryToFile, true) => assert_file(&target, NEW_FILE),
            (Direction::FileToDirectory, false) => assert_file(&target, OLD_FILE),
            (Direction::FileToDirectory, true) => {
                assert_directory(&target);
                assert!(fs::read_dir(target).unwrap().next().is_none());
            }
        }
    }

    pub(crate) fn assert_inventory(&self, point: FaultPoint) {
        self.assert_filesystem(point);
        let first = self
            .store()
            .list_entries(&self.credential, &self.selection_id, None)
            .unwrap();
        assert!(first.next_cursor.is_none());
        let entries = &first.entries;
        match (self.direction, point.new_entry_is_visible()) {
            (Direction::DirectoryToFile, false) => {
                assert_eq!(entries.len(), 4);
                assert_directory_entry(entries, &self.selection_root);
                assert_directory_entry(entries, &self.nested);
                assert_directory_entry(entries, &self.empty);
                assert_file_entry(entries, &self.old_tree_file, OLD_TREE_FILE);
            }
            (Direction::DirectoryToFile, true) => {
                assert_eq!(entries.len(), 1);
                assert_file_entry(entries, &self.target, NEW_FILE);
            }
            (Direction::FileToDirectory, false) => {
                assert_eq!(entries.len(), 2);
                assert_directory_entry(entries, &self.selection_root);
                assert_file_entry(entries, &self.target, OLD_FILE);
            }
            (Direction::FileToDirectory, true) => {
                assert_eq!(entries.len(), 2);
                assert_directory_entry(entries, &self.selection_root);
                assert_directory_entry(entries, &self.target);
            }
        }
        assert_eq!(
            self.store()
                .list_entries(&self.credential, &self.selection_id, None)
                .unwrap()
                .entries,
            first.entries
        );
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn add_sentinel_symlink(&self) -> PathBuf {
        use std::os::unix::fs::symlink;

        let staging = self.mirror_dir.join(".drip-staging");
        let stages = fs::read_dir(&staging)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(stages.len(), 1);
        assert_directory(&stages[0]);
        let sentinel_dir = self.state_dir.join("synthetic-state");
        fs::create_dir(&sentinel_dir).unwrap();
        let sentinel = sentinel_dir.join("state.sqlite");
        fs::write(&sentinel, b"synthetic state file").unwrap();
        symlink(sentinel_dir, stages[0].join("outside-state")).unwrap();
        sentinel
    }

    fn assert_old_tree(&self, target: &Path) {
        assert_directory(target);
        assert_eq!(
            fs::read(self.mirror_path(&self.old_tree_file)).unwrap(),
            OLD_TREE_FILE
        );
        let empty = self.mirror_path(&self.empty);
        assert_directory(&empty);
        assert!(fs::read_dir(empty).unwrap().next().is_none());
    }

    fn mirror_path(&self, source: &SourcePath) -> PathBuf {
        let mut path = self
            .mirror_dir
            .join(&self.device_directory)
            .join(source.drive().to_string());
        for component in source.components() {
            path.push(component);
        }
        path
    }
}

fn assert_file(path: &Path, bytes: &[u8]) {
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_file());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

fn assert_directory(path: &Path) {
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_dir());
}

fn assert_file_entry(entries: &[Entry], source: &SourcePath, bytes: &[u8]) {
    let entry = entries
        .iter()
        .find(|entry| &entry.source_path == source)
        .unwrap();
    assert_eq!(entry.kind, EntryKind::File);
    assert_eq!(entry.size, bytes.len() as u64);
    assert_eq!(
        entry.digest,
        Some(Blake3Digest::new(*blake3::hash(bytes).as_bytes()))
    );
}

fn assert_directory_entry(entries: &[Entry], source: &SourcePath) {
    let entry = entries
        .iter()
        .find(|entry| &entry.source_path == source)
        .unwrap();
    assert_eq!(entry.kind, EntryKind::Directory);
    assert_eq!(entry.size, 0);
    assert_eq!(entry.digest, None);
    assert_eq!(entry.last_write, None);
}
