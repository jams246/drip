use std::io::{Read, Seek, SeekFrom};

use drip_core::SourcePath;

pub(crate) struct CountedReader {
    inner: std::fs::File,
    path: SourcePath,
    kind: crate::engine::test_metrics::ReadKind,
}

impl CountedReader {
    pub fn new(
        inner: std::fs::File,
        path: SourcePath,
        kind: crate::engine::test_metrics::ReadKind,
    ) -> Self {
        Self { inner, path, kind }
    }

    pub fn inner(&self) -> &std::fs::File {
        &self.inner
    }
}

impl Read for CountedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        crate::engine::test_metrics::source_read(&self.path, self.kind, count);
        Ok(count)
    }
}

impl Seek for CountedReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}
