use crate::crash_dump::{crash, dump_worker, observe_exception};
use crate::diagnostics_win32::*;
use crate::native_abi::{StringHeader, read_string};
use std::fmt::{self, Write};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Diagnostics {
    pub(crate) log: isize,
    log_ready: isize,
    pub(crate) dump_path: Vec<u16>,
    pub(crate) request: isize,
    pub(crate) done: isize,
    pub(crate) module: usize,
}

pub(crate) static DIAGNOSTICS: OnceLock<Diagnostics> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
// Crash callbacks use an owned stack buffer, no heap allocation or application locks.
struct Line {
    bytes: [u8; 16_384],
    length: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            bytes: [0; 16_384],
            length: 0,
        }
    }

    fn flush(&mut self) {
        if !crate::log_writer::can_write() {
            return;
        }
        let Some(diagnostics) = DIAGNOSTICS.get() else {
            return;
        };
        self.bytes[self.length] = b'\n';
        let mut written = 0;
        unsafe {
            WriteFile(
                diagnostics.log,
                self.bytes.as_ptr(),
                (self.length + 1) as u32,
                &mut written,
                null_mut(),
            );
            SetEvent(diagnostics.log_ready);
            FlushFileBuffers(diagnostics.log);
        }
    }
}

impl Write for Line {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let mut count = value.len().min(self.bytes.len() - self.length - 1);
        while !value.is_char_boundary(count) {
            count -= 1;
        }
        self.bytes[self.length..self.length + count].copy_from_slice(&value.as_bytes()[..count]);
        self.length += count;
        Ok(())
    }
}

fn timestamp(line: &mut Line) {
    let mut time = 0u64;
    unsafe { GetSystemTimeAsFileTime(&mut time) };
    let milliseconds = time / 10_000 - 11_644_473_600_000;
    let _ = write!(
        line,
        "unix_ms={milliseconds} pid={} tid={} ",
        unsafe { GetCurrentProcessId() },
        unsafe { GetCurrentThreadId() }
    );
}

pub(crate) fn record(arguments: fmt::Arguments<'_>) {
    if DIAGNOSTICS.get().is_none() {
        return;
    }
    let mut line = Line::new();
    timestamp(&mut line);
    let _ = line.write_fmt(arguments);
    line.flush();
}

fn initialize() -> std::io::Result<()> {
    if READY.load(Ordering::Acquire) {
        return Ok(());
    }
    if DIAGNOSTICS.get().is_some() {
        return Err(std::io::Error::other(
            "Crash logging initialization did not finish.",
        ));
    }
    let executable = std::env::current_exe()?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| std::io::Error::other("Could not locate the executable directory."))?;
    let directory = executable_directory.join("logs");
    std::fs::create_dir_all(&directory)?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let session = format!("drip-{time}-{}", std::process::id());
    let log_path = directory.join(format!("{session}.log"));
    let dump_path = directory.join(format!("{session}.dmp"));
    use std::os::windows::io::IntoRawHandle;
    let (output, ready) = crate::log_writer::start(&log_path, &session)?;
    let log = output.into_raw_handle() as isize;
    let log_ready = ready.into_raw_handle() as isize;
    let request = unsafe { CreateEventW(null_mut(), 1, 0, std::ptr::null()) };
    let done = unsafe { CreateEventW(null_mut(), 1, 0, std::ptr::null()) };
    if request == 0 || done == 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(log);
            CloseHandle(log_ready);
            CloseHandle(request);
            CloseHandle(done);
        }
        return Err(error);
    }
    use std::os::windows::ffi::OsStrExt;
    let diagnostics = Diagnostics {
        log,
        log_ready,
        request,
        done,
        dump_path: dump_path.as_os_str().encode_wide().chain(Some(0)).collect(),
        module: unsafe { GetModuleHandleW(std::ptr::null()) } as usize,
    };
    let _ = DIAGNOSTICS.set(diagnostics);
    std::thread::Builder::new()
        .name("drip-crash-dump".into())
        .spawn(dump_worker)?;
    unsafe {
        if SetStdHandle(-12i32 as u32, log) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if AddVectoredExceptionHandler(1, observe_exception).is_null() {
            return Err(std::io::Error::last_os_error());
        }
        SetUnhandledExceptionFilter(crash);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record(format_args!("rust.panic {info}"));
        record(format_args!(
            "rust.backtrace {}",
            std::backtrace::Backtrace::force_capture()
        ));
        previous(info);
    }));
    record(format_args!(
        "session.start version={} executable={:?} log={:?} dump={:?}",
        env!("CARGO_PKG_VERSION"),
        executable,
        log_path,
        dump_path
    ));
    if let Ok(build) = std::fs::read_to_string(executable_directory.join("build-info.json")) {
        record(format_args!("session.build {build}"));
    }
    READY.store(true, Ordering::Release);
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_diagnostics_init() -> i32 {
    match initialize() {
        Ok(()) => 1,
        Err(error) => {
            eprintln!("Drip crash logging could not start: {error}");
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn js_drip_diagnostics_write(message: *const StringHeader) {
    let message = unsafe { read_string(message) };
    record(format_args!("{message}"));
}
