use crate::diagnostics::{DIAGNOSTICS, record};
use crate::diagnostics_win32::*;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

static CRASHING: AtomicBool = AtomicBool::new(false);
static EXCEPTION: AtomicPtr<ExceptionPointers> = AtomicPtr::new(null_mut());
static FAULT_THREAD: AtomicU32 = AtomicU32::new(0);
static DUMP_THREAD: AtomicU32 = AtomicU32::new(0);

unsafe fn record_exception(pointers: *mut ExceptionPointers, event: &str) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let Some(pointers) = (unsafe { pointers.as_ref() }) else {
        return;
    };
    let Some(exception) = (unsafe { pointers.record.as_ref() }) else {
        return;
    };
    let address = exception.address as usize;
    let access = if exception.parameter_count >= 1 {
        exception.parameters[0]
    } else {
        0
    };
    let target = if exception.parameter_count >= 2 {
        exception.parameters[1]
    } else {
        0
    };
    let fault_thread = if event == "native.crash" {
        FAULT_THREAD.load(Ordering::SeqCst)
    } else {
        unsafe { GetCurrentThreadId() }
    };
    record(format_args!(
        "{event} code=0x{:08X} address=0x{address:X} module_base=0x{:X} rva=0x{:X} access={} target=0x{:X} fault_tid={}",
        exception.code,
        diagnostics.module,
        address.saturating_sub(diagnostics.module),
        access,
        target,
        fault_thread,
    ));
}

pub(crate) unsafe extern "system" fn observe_exception(pointers: *mut ExceptionPointers) -> i32 {
    if !pointers.is_null() && !unsafe { (*pointers).record }.is_null() {
        let code = unsafe { (*(*pointers).record).code };
        if matches!(code, 0xc000_0005 | 0xc000_001d | 0xc000_0094) {
            unsafe { record_exception(pointers, "native.exception") };
        }
    }
    0 // Continue normal exception handling, including handled first-chance exceptions.
}

pub(crate) unsafe extern "system" fn crash(pointers: *mut ExceptionPointers) -> i32 {
    crate::log_writer::mark_faulting_thread();
    if CRASHING.swap(true, Ordering::SeqCst) {
        if let Some(diagnostics) = DIAGNOSTICS.get()
            && unsafe { GetCurrentThreadId() } != DUMP_THREAD.load(Ordering::SeqCst)
        {
            unsafe { WaitForSingleObject(diagnostics.done, 15_000) };
        }
        return 0;
    }
    if let Some(diagnostics) = DIAGNOSTICS.get() {
        FAULT_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
        EXCEPTION.store(pointers, Ordering::SeqCst);
        unsafe {
            SetEvent(diagnostics.request);
            WaitForSingleObject(diagnostics.done, 15_000);
        }
    }
    0 // Preserve Windows Error Reporting and process termination.
}

pub(crate) fn dump_worker() {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    unsafe {
        DUMP_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
        WaitForSingleObject(diagnostics.request, u32::MAX);
        // The faulting thread stays in the filter until this thread finishes.
        record_exception(EXCEPTION.load(Ordering::SeqCst), "native.crash");
        let file = CreateFileW(
            diagnostics.dump_path.as_ptr(),
            0x4000_0000,
            1,
            null_mut(),
            2,
            0x80,
            0,
        );
        if file == -1 {
            record(format_args!(
                "native.dump.failed error=0x{:08X}",
                GetLastError()
            ));
        } else {
            let exception = DumpException {
                thread: FAULT_THREAD.load(Ordering::SeqCst),
                pointers: EXCEPTION.load(Ordering::SeqCst),
                client_pointers: 0,
            };
            // Include heap memory referenced by stacks and registers to diagnose corrupt pointers.
            let success = MiniDumpWriteDump(
                GetCurrentProcess(),
                GetCurrentProcessId(),
                file,
                0x1000 | 0x40,
                &exception,
                null_mut(),
                null_mut(),
            );
            let error = if success == 0 { GetLastError() } else { 0 };
            FlushFileBuffers(file);
            CloseHandle(file);
            record(format_args!(
                "native.dump.complete success={success} error=0x{error:08X}"
            ));
        }
        SetEvent(diagnostics.done);
    }
}
