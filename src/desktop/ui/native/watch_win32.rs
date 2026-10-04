use std::ffi::c_void;

pub const INVALID_HANDLE: isize = -1;
pub const ABORTED: u32 = 995;
pub const ENUM_DIR: u32 = 1022;

#[repr(C)]
#[derive(Default)]
pub struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: isize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    pub fn CloseHandle(handle: isize) -> i32;
    pub fn GetLastError() -> u32;
    pub fn CreateIoCompletionPort(file: isize, port: isize, key: usize, threads: u32) -> isize;
    pub fn GetQueuedCompletionStatus(
        port: isize,
        bytes: *mut u32,
        key: *mut usize,
        operation: *mut *mut Overlapped,
        timeout: u32,
    ) -> i32;
    pub fn PostQueuedCompletionStatus(
        port: isize,
        bytes: u32,
        key: usize,
        operation: *mut Overlapped,
    ) -> i32;
    pub fn CancelIoEx(handle: isize, operation: *mut Overlapped) -> i32;
    pub fn ReadDirectoryChangesW(
        directory: isize,
        buffer: *mut c_void,
        capacity: u32,
        recursive: i32,
        filter: u32,
        bytes: *mut u32,
        operation: *mut Overlapped,
        callback: *const c_void,
    ) -> i32;
    pub fn GetFileInformationByHandleEx(
        file: isize,
        class: i32,
        info: *mut c_void,
        size: u32,
    ) -> i32;
    pub fn GetFileAttributesW(path: *const u16) -> u32;
    pub fn GetVolumePathNameW(path: *const u16, volume: *mut u16, capacity: u32) -> i32;
    pub fn GetDriveTypeW(root: *const u16) -> u32;
}

#[cfg(test)]
pub fn wide_path(path: &str) -> Vec<u16> {
    path.encode_utf16().chain(Some(0)).collect()
}

pub fn open_path(path: &[u16], access: u32, flags: u32) -> Result<isize, u32> {
    if path.last() != Some(&0) || path[..path.len() - 1].contains(&0) {
        return Err(123);
    }
    let handle = unsafe { CreateFileW(path.as_ptr(), access, 7, std::ptr::null(), 3, flags, 0) };
    if handle == INVALID_HANDLE {
        return Err(unsafe { GetLastError() });
    }
    Ok(handle)
}

pub fn close(handle: isize) {
    unsafe { CloseHandle(handle) };
}
