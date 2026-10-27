#![expect(
    unsafe_code,
    reason = "The GPUI Windows backend requires an HWND visibility bridge"
)]

use std::error::Error;

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SW_HIDE, SW_RESTORE, ShowWindow},
};

fn hwnd(window: &Window) -> Result<HWND, Box<dyn Error>> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
        _ => Err("Drip requires a native Windows window".into()),
    }
}

pub(super) fn hide(window: &Window) -> Result<(), Box<dyn Error>> {
    // GPUI's platform hide is a no-op at the pinned revision.
    let handle = hwnd(window)?;
    unsafe {
        let _ = ShowWindow(handle, SW_HIDE);
    }
    Ok(())
}

pub(super) fn show(window: &Window) -> Result<(), Box<dyn Error>> {
    let handle = hwnd(window)?;
    unsafe {
        let _ = ShowWindow(handle, SW_RESTORE);
    }
    window.activate_window();
    Ok(())
}
