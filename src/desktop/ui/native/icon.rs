use std::sync::OnceLock;
mod metadata;
mod names;
mod native_abi;
mod watch;
mod watch_api;
mod watch_queue;
mod watch_registration;
#[cfg(test)]
mod watch_tests;
mod watch_win32;

mod instance;
mod shell;
mod shell_win32;
mod tray;

// Win32 requires DWORD-aligned icon resource bytes.
#[repr(align(4))]
struct IconResource([u8; include_bytes!("drip.png").len()]);
static PNG: IconResource = IconResource(*include_bytes!("drip.png"));
const WM_SETICON: u32 = 0x0080;
const SM_CXICON: i32 = 11;
const SM_CXSMICON: i32 = 49;
static ICONS: OnceLock<(isize, isize)> = OnceLock::new();

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumThreadWindows(
        thread: u32,
        callback: unsafe extern "system" fn(isize, isize) -> i32,
        context: isize,
    ) -> i32;
    fn GetClassNameW(window: isize, text: *mut u16, capacity: i32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn CreateIconFromResourceEx(
        data: *const u8,
        length: u32,
        is_icon: i32,
        version: u32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> isize;
    fn SendMessageW(window: isize, message: u32, parameter: usize, value: isize) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

fn create_icon(size: i32) -> isize {
    // The embedded resource remains aligned and valid for the entire call.
    unsafe {
        CreateIconFromResourceEx(
            PNG.0.as_ptr(),
            PNG.0.len() as u32,
            1,
            0x0003_0000,
            size,
            size,
            0,
        )
    }
}

pub(crate) fn small_icon() -> isize {
    ICONS
        .get_or_init(|| {
            (
                create_icon(unsafe { GetSystemMetrics(SM_CXSMICON) }),
                create_icon(unsafe { GetSystemMetrics(SM_CXICON) }),
            )
        })
        .0
}

extern "system" fn find_main_window(window: isize, context: isize) -> i32 {
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    if length > 0
        && class[..length as usize]
            .iter()
            .copied()
            .eq("PerryMainWindow".encode_utf16())
    {
        unsafe { *(context as *mut isize) = window };
        return 0;
    }
    1
}

pub(crate) fn main_window() -> isize {
    let mut window = 0isize;
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            find_main_window,
            &mut window as *mut _ as isize,
        )
    };
    window
}

extern "system" fn set_main_window_icon(window: isize, _context: isize) -> i32 {
    let mut class = [0u16; 64];
    // Windows writes at most the supplied capacity into this owned buffer.
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    if length <= 0
        || !class[..length as usize]
            .iter()
            .copied()
            .eq("PerryMainWindow".encode_utf16())
    {
        return 1;
    }

    let small = small_icon();
    let large = ICONS.get().unwrap().1;
    if small != 0 {
        unsafe { SendMessageW(window, WM_SETICON, 0, small) };
    }
    if large != 0 {
        unsafe { SendMessageW(window, WM_SETICON, 1, large) };
    }
    0
}

/// Set the title-bar and taskbar icons on this UI thread's main window.
///
/// Call from DRIP's UI thread to update its window.
#[unsafe(no_mangle)]
pub extern "C" fn js_drip_set_window_icon() {
    unsafe { EnumThreadWindows(GetCurrentThreadId(), set_main_window_icon, 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn DestroyIcon(icon: isize) -> i32;
    }

    #[test]
    fn safe_icon_api_creates_both_windows_icon_sizes() {
        js_drip_set_window_icon();
        assert_eq!((PNG.0.as_ptr() as usize) % 4, 0);
        for size in [16, 32] {
            let icon = create_icon(size);
            assert_ne!(icon, 0);
            assert_ne!(unsafe { DestroyIcon(icon) }, 0);
        }
    }
}
