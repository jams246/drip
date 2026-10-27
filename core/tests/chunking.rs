use std::io::{self, Cursor, Read};

use drip_core::protocol::UploadPlanRequest;
use drip_core::{
    Blake3Digest, CHUNK_MAX_SIZE, CHUNK_MIN_SIZE, ChunkConfig, ChunkData, ChunkDescriptor,
    ChunkStream, ChunkStreamError,
};

#[test]
fn stream_chunks_hash_independently_and_cover_the_input() {
    let source: Vec<_> = (0..2_000_000).map(|index| (index * 31) as u8).collect();
    let expected_digest = Blake3Digest::new(*blake3::hash(&source).as_bytes());
    let mut stream = ChunkStream::new(Cursor::new(source.clone()));
    let mut next_offset = 0_u64;
    let mut count = 0;

    while let Some(chunk) = stream.next_chunk().unwrap() {
        assert_chunk(&chunk);
        assert_eq!(chunk.descriptor.ordinal, count);
        assert_eq!(chunk.descriptor.offset, next_offset);
        assert_eq!(
            chunk.descriptor.digest,
            Blake3Digest::new(*blake3::hash(&chunk.bytes).as_bytes())
        );
        next_offset += u64::from(chunk.descriptor.length);
        count += 1;
    }

    assert_eq!(next_offset, source.len() as u64);
    assert!(count > 1);
    assert_eq!(stream.finish().unwrap(), expected_digest);
}

#[test]
fn empty_stream_has_no_chunks_and_hashes_empty_input() {
    let mut stream = ChunkStream::new(Cursor::new([]));

    assert!(stream.next_chunk().unwrap().is_none());
    assert_eq!(
        stream.finish().unwrap(),
        Blake3Digest::new(*blake3::hash(&[]).as_bytes())
    );
}

#[test]
fn stream_error_cannot_finalize_partial_file_digest() {
    let mut stream = ChunkStream::new(FailingReader);

    assert!(matches!(
        stream.next_chunk(),
        Err(ChunkStreamError::FastCdc(_))
    ));
    assert!(matches!(stream.next_chunk(), Err(ChunkStreamError::Failed)));
    assert!(matches!(stream.finish(), Err(ChunkStreamError::Failed)));
}

#[test]
fn empty_file_plan_has_no_descriptors() {
    assert!(plan(Vec::new()).validate(0, 0, 0).is_ok());
}

#[test]
fn plan_validation_accepts_final_short_chunk() {
    let request = plan(vec![descriptor(0, 0, 1_000)]);

    assert!(request.validate(1_000, 0, 0).is_ok());
}

#[test]
fn plan_validation_rejects_short_nonfinal_chunk() {
    let request = plan(vec![
        descriptor(0, 0, 1_000),
        descriptor(1, 1_000, CHUNK_MIN_SIZE),
    ]);

    assert!(request.validate(100_000, 0, 0).is_err());
}

#[test]
fn plan_validation_rejects_offset_gaps() {
    let request = plan(vec![descriptor(0, 1, CHUNK_MIN_SIZE)]);

    assert!(request.validate(u64::from(CHUNK_MIN_SIZE), 0, 0).is_err());
}

#[test]
fn plan_validation_rejects_range_overflow() {
    let request = plan(vec![descriptor(u64::MAX, u64::MAX - 1, 2)]);

    assert!(request.validate(u64::MAX, u64::MAX, u64::MAX - 1).is_err());
}

#[test]
fn plan_validation_rejects_unsupported_chunking_version() {
    let mut request = plan(vec![descriptor(0, 0, 1_000)]);
    request.config.chunking_version += 1;

    assert!(request.validate(1_000, 0, 0).is_err());
}

fn assert_chunk(chunk: &ChunkData) {
    assert!(chunk.descriptor.length > 0);
    assert!(chunk.descriptor.length <= CHUNK_MAX_SIZE);
    assert_eq!(chunk.descriptor.length as usize, chunk.bytes.len());
}

fn plan(descriptors: Vec<ChunkDescriptor>) -> UploadPlanRequest {
    UploadPlanRequest {
        config: ChunkConfig::V1,
        descriptors,
    }
}

fn descriptor(ordinal: u64, offset: u64, length: u32) -> ChunkDescriptor {
    ChunkDescriptor {
        ordinal,
        offset,
        length,
        digest: Blake3Digest::new([0; 32]),
    }
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("synthetic read failure"))
    }
}
