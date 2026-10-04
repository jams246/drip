use crate::bounded_logs::BoundedLogs;
use crate::diagnostics_win32::*;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const MAX_LOG_BYTES: u64 = 50_000_000;
// Leave room for new records after pruning so each write does not copy the whole tail.
const RETAIN_LOG_BYTES: u64 = 40_000_000;
const PIPE_BUFFER_BYTES: usize = 64 * 1024;
static WRITER_THREAD: AtomicU32 = AtomicU32::new(0);
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

fn disable() {
    UNAVAILABLE.store(true, Ordering::Release);
    if let Some(diagnostics) = crate::diagnostics::DIAGNOSTICS.get() {
        // Release pending stderr writes if their reader can no longer drain the pipe.
        unsafe { DisconnectNamedPipe(diagnostics.log) };
    }
}

pub(crate) fn mark_faulting_thread() {
    if unsafe { GetCurrentThreadId() } == WRITER_THREAD.load(Ordering::Acquire) {
        disable();
    }
}

pub(crate) fn can_write() -> bool {
    // A fault on the writer must not make a crash callback wait for itself.
    mark_faulting_thread();
    !UNAVAILABLE.load(Ordering::Acquire)
}

unsafe fn own(handle: isize) -> io::Result<OwnedHandle> {
    if handle == 0 || handle == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as *mut _) })
}

pub(crate) fn start(log_path: &Path, session: &str) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let logs = BoundedLogs::open(log_path, MAX_LOG_BYTES, RETAIN_LOG_BYTES)?;
    let name: Vec<u16> = format!("\\\\.\\pipe\\drip-log-{session}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let output = unsafe {
        own(CreateNamedPipeW(
            name.as_ptr(),
            2 | 0x80000,
            4 | 2 | 8,
            1,
            PIPE_BUFFER_BYTES as u32,
            0,
            0,
            null_mut(),
        ))?
    };
    let input = unsafe {
        own(CreateFileW(
            name.as_ptr(),
            0x8000_0000,
            0,
            null_mut(),
            3,
            0x80,
            0,
        ))?
    };
    if unsafe { ConnectNamedPipe(output.as_raw_handle() as isize, null_mut()) } == 0
        && unsafe { GetLastError() } != 535
    {
        return Err(io::Error::last_os_error());
    }
    let ready = unsafe { own(CreateEventW(null_mut(), 0, 0, std::ptr::null()))? };
    let signal = ready.try_clone()?;
    std::thread::Builder::new()
        .name("drip-log-writer".into())
        .spawn(move || collect(input, signal, logs))?;
    Ok((output, ready))
}

fn collect(input: OwnedHandle, ready: OwnedHandle, mut logs: BoundedLogs) {
    WRITER_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    let pipe = input.as_raw_handle() as isize;
    let mut buffer = [0u8; PIPE_BUFFER_BYTES];
    loop {
        let mut count = 0;
        if unsafe {
            PeekNamedPipe(
                pipe,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut count,
                null_mut(),
                null_mut(),
            )
        } == 0
        {
            disable();
            return;
        }
        if count == 0 {
            unsafe { WaitForSingleObject(ready.as_raw_handle() as isize, 25) };
            continue;
        }
        // Persist peeked bytes before consuming them. Server-side FlushFileBuffers then
        // acknowledges disk persistence, including records written by a faulting thread.
        if logs.append(&buffer[..count as usize]).is_err() {
            disable();
            return;
        }
        let mut read = 0;
        let success = unsafe { ReadFile(pipe, buffer.as_mut_ptr(), count, &mut read, null_mut()) };
        if read != count || (success == 0 && unsafe { GetLastError() } != 234) {
            disable();
            return;
        }
    }
}
