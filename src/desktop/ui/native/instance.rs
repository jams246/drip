use crate::shell_win32::*;
use std::cell::Cell;

thread_local! {
    static INSTANCE: Cell<isize> = const { Cell::new(0) };
}

pub(crate) fn activation_message() -> u32 {
    unsafe { RegisterWindowMessageW(wide("DRIP.Desktop.Activate.v1").as_ptr()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_instance_claim() -> i32 {
    if INSTANCE.with(|instance| instance.get() != 0) {
        return 1;
    }
    let name = wide("Local\\DRIP.Desktop.Watcher.v1");
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle == 0 {
        return 0;
    }
    if unsafe { GetLastError() } == 183 {
        unsafe { CloseHandle(handle) };
        let window =
            unsafe { FindWindowW(wide("PerryMainWindow").as_ptr(), wide("DRIP").as_ptr()) };
        if window != 0 {
            unsafe { PostMessageW(window, activation_message(), 0, 0) };
        }
        return 0;
    }
    INSTANCE.with(|instance| instance.set(handle));
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_instance_release() {
    let handle = INSTANCE.with(|instance| instance.replace(0));
    if handle != 0 {
        unsafe { CloseHandle(handle) };
    }
}
