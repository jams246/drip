use crate::native_abi::{StringHeader, read_path, write_string};
use crate::watch_win32::*;

#[repr(C)]
#[derive(Default)]
struct AttributeTag {
    attributes: u32,
    reparse_tag: u32,
}

pub fn query_wide(path: &[u16]) -> String {
    let mut volume = [0u16; 32768];
    let drive_type =
        if unsafe { GetVolumePathNameW(path.as_ptr(), volume.as_mut_ptr(), 32768) } != 0 {
            unsafe { GetDriveTypeW(volume.as_ptr()) }
        } else {
            0
        };
    let mut attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == u32::MAX {
        return result(0, 0, false, drive_type, unsafe { GetLastError() });
    }
    let handle = match open_path(path, 0, 0x0220_0000) {
        Ok(handle) => handle,
        Err(error) => return result(attributes, 0, false, drive_type, error),
    };
    let mut tag = AttributeTag::default();
    let mut error = 0;
    if unsafe { GetFileInformationByHandleEx(handle, 9, (&mut tag as *mut AttributeTag).cast(), 8) }
        != 0
    {
        attributes = tag.attributes;
    } else {
        error = unsafe { GetLastError() };
    }
    let mut case_flags = 0u32;
    if attributes & 0x10 != 0 {
        let supported = unsafe {
            GetFileInformationByHandleEx(handle, 23, (&mut case_flags as *mut u32).cast(), 4)
        };
        if supported == 0 {
            let case_error = unsafe { GetLastError() };
            if ![1, 50, 87].contains(&case_error) {
                error = case_error;
            }
        }
    }
    close(handle);
    result(
        attributes,
        tag.reparse_tag,
        case_flags & 1 != 0,
        drive_type,
        error,
    )
}

fn result(attributes: u32, tag: u32, case_sensitive: bool, drive: u32, error: u32) -> String {
    format!(
        "{{\"attributes\":{attributes},\"reparseTag\":{tag},\"caseSensitive\":{case_sensitive},\"driveType\":{drive},\"error\":{error}}}"
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn js_drip_query_path(path: *const StringHeader) -> *mut StringHeader {
    let output = match unsafe { read_path(path) } {
        Ok(path) => query_wide(&path),
        Err(error) => result(0, 0, false, 0, error),
    };
    write_string(&output)
}
