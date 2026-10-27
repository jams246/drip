#![expect(
    unsafe_code,
    reason = "Windows token and SID APIs require audited pointer boundaries"
)]

use std::{fs, io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows::{
    Win32::{
        Foundation::{HANDLE, HLOCAL, LocalFree},
        Security::Authorization::ConvertSidToStringSidW,
        Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
    core::{PCWSTR, PWSTR},
};

pub(super) struct InstanceKey {
    pub mutex_name: String,
    pub pipe_name: String,
}

pub(super) fn instance_key(directory: &Path) -> Result<InstanceKey, super::InstanceError> {
    let sid = current_user_sid().map_err(super::InstanceError::CurrentUser)?;
    let directory =
        fs::canonicalize(directory).map_err(|source| super::InstanceError::DataDirectory {
            path: directory.display().to_string(),
            source,
        })?;
    if !directory.is_dir() {
        return Err(super::InstanceError::DataDirectory {
            path: directory.display().to_string(),
            source: io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
        });
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(&wide_bytes(directory.as_os_str()));
    let hash = hasher.finalize().to_hex();
    Ok(InstanceKey {
        mutex_name: format!("Global\\Drip-{sid}-{hash}"),
        pipe_name: format!(r"\\.\pipe\Drip-{sid}-{hash}"),
    })
}

fn wide_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

pub(super) fn current_user_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: the current process pseudo-handle is valid and the output handle is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(io::Error::other)?;
    let token = TokenHandle(token);

    let mut size = 0;
    // SAFETY: the null buffer requests the required size; `size` is writable.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut size) };
    if size == 0 {
        return Err(io::Error::other("Windows returned an empty user token"));
    }
    let words = (size as usize).div_ceil(std::mem::size_of::<u64>());
    let mut buffer = vec![0_u64; words];
    // SAFETY: the aligned buffer is sized from the first query and `size` is writable.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(io::Error::other)?;
    // SAFETY: the successful query filled a valid TOKEN_USER at the start of `buffer`.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = PWSTR(ptr::null_mut());
    // SAFETY: Windows returned a valid SID pointer in TOKEN_USER and `sid` is writable.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }.map_err(io::Error::other)?;
    let sid = SidString(sid);
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated string in live local memory.
    unsafe { PCWSTR(sid.0.0).to_string() }.map_err(io::Error::other)
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the token handle returned by OpenProcessToken.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

struct SidString(PWSTR);

impl Drop for SidString {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: ConvertSidToStringSidW allocated this string for LocalFree.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0.cast())));
            }
        }
    }
}
