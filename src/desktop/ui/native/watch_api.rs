use crate::native_abi::{StringHeader, read_path, write_string};
use crate::watch;
use crate::watch_queue::queue;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn js_drip_watch_start(path: *const StringHeader, recursive: i32) -> f64 {
    let result =
        unsafe { read_path(path) }.and_then(|path| watch::start_wide(path, recursive != 0));
    match result {
        Ok(id) => id as f64,
        Err(error) => -(error as f64),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_watch_stop(id: f64) {
    watch::stop(id as usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_watch_drain() -> *mut StringHeader {
    let records = queue().lock().unwrap().drain();
    write_string(&records)
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_watch_ack(id: f64, sequence: f64) {
    queue()
        .lock()
        .unwrap()
        .acknowledge(id as usize, sequence as u64);
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_watch_shutdown() {
    watch::shutdown();
}
