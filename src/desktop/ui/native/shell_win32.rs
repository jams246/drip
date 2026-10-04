pub(crate) type SubclassProc =
    unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize;

pub(crate) const WM_CLOSE: u32 = 0x0010;
pub(crate) const WM_QUERYENDSESSION: u32 = 0x0011;
pub(crate) const WM_ENDSESSION: u32 = 0x0016;
pub(crate) const WM_SYSCOMMAND: u32 = 0x0112;
pub(crate) const WM_SIZE: u32 = 0x0005;
pub(crate) const WM_NCDESTROY: u32 = 0x0082;
pub(crate) const WM_TRAY: u32 = 0x8000 + 92;
pub(crate) const SC_MINIMIZE: usize = 0xf020;
pub(crate) const SW_HIDE: i32 = 0;
pub(crate) const SW_RESTORE: i32 = 9;
pub(crate) const SUBCLASS_ID: usize = 0x44524950;

#[repr(C)]
pub(crate) struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub(crate) struct NotifyIconData {
    pub size: u32,
    pub window: isize,
    pub id: u32,
    pub flags: u32,
    pub callback_message: u32,
    pub icon: isize,
    pub tip: [u16; 128],
    pub state: u32,
    pub state_mask: u32,
    pub info: [u16; 256],
    pub version: u32,
    pub info_title: [u16; 64],
    pub info_flags: u32,
    pub guid: [u8; 16],
    pub balloon_icon: isize,
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(crate) fn ShowWindow(window: isize, command: i32) -> i32;
    pub(crate) fn SetForegroundWindow(window: isize) -> i32;
    pub(crate) fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub(crate) fn FindWindowW(class: *const u16, title: *const u16) -> isize;
    pub(crate) fn PostMessageW(window: isize, message: u32, parameter: usize, value: isize) -> i32;
    pub(crate) fn CreatePopupMenu() -> isize;
    pub(crate) fn AppendMenuW(menu: isize, flags: u32, id: usize, label: *const u16) -> i32;
    pub(crate) fn DestroyMenu(menu: isize) -> i32;
    pub(crate) fn GetCursorPos(point: *mut Point) -> i32;
    pub(crate) fn TrackPopupMenu(
        menu: isize,
        flags: u32,
        x: i32,
        y: i32,
        reserved: i32,
        window: isize,
        rectangle: *const std::ffi::c_void,
    ) -> u32;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    pub(crate) fn SetWindowSubclass(
        window: isize,
        callback: SubclassProc,
        id: usize,
        data: usize,
    ) -> i32;
    pub(crate) fn RemoveWindowSubclass(window: isize, callback: SubclassProc, id: usize) -> i32;
    pub(crate) fn DefSubclassProc(
        window: isize,
        message: u32,
        parameter: usize,
        value: isize,
    ) -> isize;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub(crate) fn Shell_NotifyIconW(operation: u32, data: *const NotifyIconData) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn CreateMutexW(
        attributes: *const std::ffi::c_void,
        owner: i32,
        name: *const u16,
    ) -> isize;
    pub(crate) fn GetLastError() -> u32;
    pub(crate) fn CloseHandle(handle: isize) -> i32;
}

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
