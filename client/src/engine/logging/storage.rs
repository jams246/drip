use std::{
    fs::{self, File},
    io,
    path::Path,
};

pub(super) fn open_log(path: &Path) -> io::Result<File> {
    match crate::private_file::open(path, true, false) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            crate::private_file::open(path, true, true)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn rotate(current: &Path, rotated: &Path) -> io::Result<()> {
    match crate::private_file::open(rotated, false, false) {
        Ok(file) => {
            drop(file);
            fs::remove_file(rotated)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(current, rotated)
}

pub(super) fn read_log(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match crate::private_file::read(path, super::MAX_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
