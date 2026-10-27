use std::{
    io::{Cursor, Read},
    sync::{Arc, atomic::AtomicBool},
};

use drip_core::{ChunkStream, SourcePath};
use tokio::sync::mpsc;

use super::next_page;

fn page<R: Read>(reader: R, size: u64) -> Result<super::ChunkPage<R>, crate::source::SourceError> {
    let (events, _) = mpsc::channel(1);
    next_page(
        ChunkStream::new(reader),
        size,
        0,
        0,
        super::ChunkingContext {
            source_hashed_bytes: 0,
            planned_payload_bytes: 0,
            file_payload_sent: 0,
            source_path: SourcePath::new('C', vec!["file.bin".into()]).unwrap(),
            events,
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    )
}

#[test]
fn empty_source_has_an_empty_plan_and_empty_digest() {
    let page = page(Cursor::new(Vec::new()), 0).unwrap();

    assert!(page.descriptors.is_empty());
    assert_eq!(page.source_chunked_bytes, 0);
    assert_eq!(page.chunk_count, 0);
    assert_eq!(
        page.digest,
        Some(drip_core::Blake3Digest::new(*blake3::hash(b"").as_bytes()))
    );
    assert!(page.stream.is_none());
}

#[test]
fn page_tracks_only_descriptors_and_matches_full_stream_digest() {
    let bytes = (0usize..3 * 1024 * 1024)
        .map(|index| (index.wrapping_mul(31) >> 8) as u8)
        .collect::<Vec<_>>();
    let expected = drip_core::Blake3Digest::new(*blake3::hash(&bytes).as_bytes());
    let page = page(Cursor::new(bytes.clone()), bytes.len() as u64).unwrap();

    assert_eq!(page.source_chunked_bytes, bytes.len() as u64);
    assert_eq!(page.chunk_count, page.descriptors.len() as u64);
    assert_eq!(page.digest, Some(expected));
    assert!(page.stream.is_none());
    assert_eq!(page.descriptors.first().unwrap().offset, 0);
    assert!(
        page.descriptors
            .windows(2)
            .all(|pair| pair[0].offset + u64::from(pair[0].length) == pair[1].offset)
    );
}

#[test]
fn source_ending_before_expected_size_is_rejected() {
    assert!(matches!(
        page(Cursor::new(vec![7; 65_536]), 65_537),
        Err(crate::source::SourceError::Changed)
    ));
}
