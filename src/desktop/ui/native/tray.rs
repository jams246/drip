use crate::shell_win32::*;

fn notification(window: isize) -> NotifyIconData {
    let mut data: NotifyIconData = unsafe { std::mem::zeroed() };
    data.size = std::mem::size_of::<NotifyIconData>() as u32;
    data.window = window;
    data.id = 1;
    data.flags = 1 | 2 | 4 | 0x80;
    data.callback_message = WM_TRAY;
    data.icon = crate::small_icon();
    data.tip[..4].copy_from_slice(&[68, 82, 73, 80]);
    data
}

pub(crate) fn add(window: isize) -> bool {
    let mut data = notification(window);
    if data.icon == 0 || unsafe { Shell_NotifyIconW(0, &data) } == 0 {
        return false;
    }
    data.version = 4;
    unsafe { Shell_NotifyIconW(4, &data) };
    true
}

pub(crate) fn remove(window: isize) {
    unsafe { Shell_NotifyIconW(2, &notification(window)) };
}

pub(crate) fn menu(window: isize, paused: bool) -> Option<&'static str> {
    let menu = unsafe { CreatePopupMenu() };
    if menu == 0 {
        return None;
    }
    let pause_label = if paused { "Resume" } else { "Pause" };
    let items = [(1, "Open"), (2, pause_label), (3, "Exit")];
    for (id, text) in items {
        if unsafe { AppendMenuW(menu, 0, id, wide(text).as_ptr()) } == 0 {
            unsafe { DestroyMenu(menu) };
            return None;
        }
    }
    let mut position = Point { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut position);
        SetForegroundWindow(window);
    }
    let selected = unsafe {
        TrackPopupMenu(
            menu,
            0x0100 | 0x0002,
            position.x,
            position.y,
            0,
            window,
            std::ptr::null(),
        )
    };
    unsafe {
        DestroyMenu(menu);
        PostMessageW(window, 0, 0, 0);
    }
    match selected {
        1 => Some("open"),
        2 => Some("pause"),
        3 => Some("exit"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_layout_matches_windows_x64() {
        assert_eq!(std::mem::size_of::<NotifyIconData>(), 976);
        assert_eq!(std::mem::offset_of!(NotifyIconData, window), 8);
        assert_eq!(std::mem::offset_of!(NotifyIconData, icon), 32);
        assert_eq!(std::mem::offset_of!(NotifyIconData, version), 816);
        assert_eq!(std::mem::offset_of!(NotifyIconData, balloon_icon), 968);
    }
}
