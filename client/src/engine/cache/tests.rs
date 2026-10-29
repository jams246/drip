use super::*;
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".verification/watch-isolation/cache-tests")
            .join(crate::source::random_id().unwrap());
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("file.txt"), b"old bytes").unwrap();
        Self(root)
    }

    fn source(&self) -> SourceFile {
        SourceFile::open(&self.0.join("file.txt")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn remember(cache: &FileCache, source: &SourceFile, selection: &str) -> Blake3Digest {
    let digest = Blake3Digest::new(*blake3::hash(b"old bytes").as_bytes());
    cache.remember(
        selection,
        source.protocol_path(),
        source.observation(),
        source.volume_identity(),
        digest,
    );
    digest
}

#[tokio::test]
async fn unchanged_source_reuses_digest_without_reading_contents() {
    let fixture = Fixture::new();
    let cache = FileCache::default();
    let source = fixture.source();
    let digest = remember(&cache, &source, "a");
    source.verify().await.unwrap();
    assert_eq!(cache.get("a", &source), Some(digest));
    assert_eq!(cache.get("b", &source), None);
}

#[test]
fn every_identity_and_metadata_field_must_match() {
    let fixture = Fixture::new();
    let cache = FileCache::default();
    let source = fixture.source();
    for field in 0..7 {
        remember(&cache, &source, "a");
        let mut entries = cache.entries();
        let entry = entries.values_mut().next().unwrap();
        match field {
            0 => entry.observation.volume ^= 1,
            1 => entry.observation.file_id[0] ^= 1,
            2 => entry.observation.size += 1,
            3 => entry.observation.last_write = drip_core::Timestamp::new(1, 0).unwrap(),
            4 => entry.observation.change_time += 1,
            5 => entry.volume_identity.push('x'),
            _ => entry.observation.change_time = 0,
        }
        drop(entries);
        assert_eq!(cache.get("a", &source), None);
    }
}

#[test]
fn replacement_and_same_size_edit_with_restored_write_time_miss_cache() {
    let fixture = Fixture::new();
    let cache = FileCache::default();
    let path = fixture.0.join("file.txt");
    let original_time = fs::metadata(&path).unwrap().modified().unwrap();
    let source = fixture.source();
    remember(&cache, &source, "a");
    drop(source);
    fs::write(&path, b"new bytes").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(original_time)
        .unwrap();
    let edited = fixture.source();
    assert_eq!(cache.get("a", &edited), None);
    remember(&cache, &edited, "a");
    drop(edited);
    fs::rename(&path, fixture.0.join("old.txt")).unwrap();
    fs::write(&path, b"new bytes").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(original_time)
        .unwrap();
    assert_eq!(cache.get("a", &fixture.source()), None);
}

#[test]
fn removed_changed_or_uncertain_selections_are_evicted_independently() {
    let fixture = Fixture::new();
    let cache = FileCache::default();
    let source = fixture.source();
    let a = SavedSelection {
        id: "a".into(),
        source: source.protocol_path().clone(),
        root_kind: drip_core::protocol::EntryKind::File,
        volume_identity: source.volume_identity().into(),
        active: true,
    };
    let mut b = a.clone();
    b.id = "b".into();
    let digest = remember(&cache, &source, "a");
    remember(&cache, &source, "b");
    cache.selections(&[a.clone(), b.clone()], std::slice::from_ref(&a));
    assert_eq!(cache.get("a", &source), Some(digest));
    assert_eq!(cache.get("b", &source), None);
    let mut changed = a.clone();
    changed.active = false;
    cache.selections(&[a], &[changed]);
    assert_eq!(cache.get("a", &source), None);
    remember(&cache, &source, "a");
    remember(&cache, &source, "b");
    cache.forget("a");
    assert_eq!(cache.get("a", &source), None);
    assert_eq!(cache.get("b", &source), Some(digest));
    cache.clear();
    assert_eq!(cache.get("b", &source), None);
}

#[test]
fn complete_scan_prunes_missing_files_only_within_that_selection() {
    let fixture = Fixture::new();
    let cache = FileCache::default();
    let source = fixture.source();
    remember(&cache, &source, "a");
    let digest = remember(&cache, &source, "b");
    cache.scanned("a", &[]);
    assert_eq!(cache.get("a", &source), None);
    assert_eq!(cache.get("b", &source), Some(digest));
}
