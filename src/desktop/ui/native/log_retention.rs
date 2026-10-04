use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

/// Requires an exclusively owned read/write file opened without append mode.
/// After pruning, the file cursor points to the end of the retained records.
pub(crate) fn prune_oldest(file: &mut File, max_bytes: u64, keep_bytes: u64) -> io::Result<()> {
    if keep_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Log retention cannot exceed the maximum log size.",
        ));
    }
    let length = file.metadata()?.len();
    if length <= max_bytes {
        return Ok(());
    }

    let mut buffer = [0u8; 64 * 1024];
    let mut start = length.saturating_sub(keep_bytes);
    if start > 0 {
        file.seek(SeekFrom::Start(start - 1))?;
        file.read_exact(&mut buffer[..1])?;
        if buffer[0] != b'\n' {
            file.seek(SeekFrom::Start(start))?;
            while start < length {
                let count = (length - start).min(buffer.len() as u64) as usize;
                file.read_exact(&mut buffer[..count])?;
                if let Some(newline) = buffer[..count].iter().position(|&byte| byte == b'\n') {
                    start += newline as u64 + 1;
                    break;
                }
                start += count as u64;
            }
        }
    }

    let mut read_position = start;
    let mut write_position = 0;
    while read_position < length {
        let count = (length - read_position).min(buffer.len() as u64) as usize;
        file.seek(SeekFrom::Start(read_position))?;
        file.read_exact(&mut buffer[..count])?;
        file.seek(SeekFrom::Start(write_position))?;
        file.write_all(&buffer[..count])?;
        read_position += count as u64;
        write_position += count as u64;
    }
    file.set_len(write_position)?;
    file.seek(SeekFrom::Start(write_position))?;
    Ok(())
}
