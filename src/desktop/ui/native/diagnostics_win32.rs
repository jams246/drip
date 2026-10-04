use std::ffi::c_void;

#[repr(C)]
pub(crate) struct ExceptionRecord {
    pub code: u32,
    pub flags: u32,
    pub nested: *mut ExceptionRecord,
    pub address: *mut c_void,
    pub parameter_count: u32,
    pub parameters: [usize; 15],
}

#[repr(C)]
pub(crate) struct ExceptionPointers {
    pub record: *mut ExceptionRecord,
    pub context: *mut c_void,
}

// DbgHelp declares this structure with four-byte packing, including on x64.
#[repr(C, packed(4))]
pub(crate) struct DumpException {
    pub thread: u32,
    pub pointers: *mut ExceptionPointers,
    pub client_pointers: i32,
}

pub(crate) type ExceptionHandler = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn AddVectoredExceptionHandler(first: u32, handler: ExceptionHandler)
    -> *mut c_void;
    pub(crate) fn SetUnhandledExceptionFilter(
        handler: ExceptionHandler,
    ) -> Option<ExceptionHandler>;
    pub(crate) fn GetCurrentProcess() -> isize;
    pub(crate) fn GetCurrentProcessId() -> u32;
    pub(crate) fn GetCurrentThreadId() -> u32;
    pub(crate) fn GetModuleHandleW(name: *const u16) -> isize;
    pub(crate) fn GetSystemTimeAsFileTime(time: *mut u64);
    pub(crate) fn GetLastError() -> u32;
    pub(crate) fn SetStdHandle(kind: u32, handle: isize) -> i32;
    pub(crate) fn WriteFile(
        handle: isize,
        data: *const u8,
        size: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    pub(crate) fn FlushFileBuffers(handle: isize) -> i32;
    pub(crate) fn ReadFile(
        handle: isize,
        data: *mut u8,
        size: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    pub(crate) fn CreateNamedPipeW(
        name: *const u16,
        access: u32,
        mode: u32,
        instances: u32,
        output_size: u32,
        input_size: u32,
        timeout: u32,
        attributes: *mut c_void,
    ) -> isize;
    pub(crate) fn ConnectNamedPipe(pipe: isize, overlapped: *mut c_void) -> i32;
    pub(crate) fn DisconnectNamedPipe(pipe: isize) -> i32;
    pub(crate) fn PeekNamedPipe(
        pipe: isize,
        data: *mut u8,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        remaining: *mut u32,
    ) -> i32;
    pub(crate) fn CreateFileW(
        path: *const u16,
        access: u32,
        sharing: u32,
        attributes: *mut c_void,
        disposition: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    pub(crate) fn CreateEventW(
        attributes: *mut c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> isize;
    pub(crate) fn SetEvent(event: isize) -> i32;
    pub(crate) fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
    pub(crate) fn CloseHandle(handle: isize) -> i32;
}

#[link(name = "dbghelp")]
unsafe extern "system" {
    pub(crate) fn MiniDumpWriteDump(
        process: isize,
        process_id: u32,
        file: isize,
        kind: u32,
        exception: *const DumpException,
        streams: *const c_void,
        callback: *const c_void,
    ) -> i32;
}
