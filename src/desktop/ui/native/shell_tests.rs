use super::*;

#[repr(C)]
struct WindowClass {
    style: u32,
    procedure: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
    class_extra: i32,
    window_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
struct Message {
    window: isize,
    message: u32,
    parameter: usize,
    value: isize,
    time: u32,
    point: Point,
    private: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WindowClass) -> u16;
    fn UnregisterClassW(class: *const u16, instance: isize) -> i32;
    fn CreateWindowExW(
        extended_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        data: *const std::ffi::c_void,
    ) -> isize;
    fn DefWindowProcW(window: isize, message: u32, parameter: usize, value: isize) -> isize;
    fn SendMessageW(window: isize, message: u32, parameter: usize, value: isize) -> isize;
    fn IsWindow(window: isize) -> i32;
    fn IsWindowVisible(window: isize) -> i32;
    fn PeekMessageW(
        message: *mut Message,
        window: isize,
        first: u32,
        last: u32,
        remove: u32,
    ) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}

unsafe extern "system" fn default_procedure(
    window: isize,
    message: u32,
    parameter: usize,
    value: isize,
) -> isize {
    unsafe { DefWindowProcW(window, message, parameter, value) }
}

fn dispatch_pending(window: isize) {
    let mut message: Message = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut message, window, 0, 0, 1) } != 0 {
        unsafe { DispatchMessageW(&message) };
    }
}

#[test]
fn hidden_window_routes_lifecycle_and_recovers_tray_without_destroying_window() {
    let name = wide("PerryMainWindow");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WindowClass {
        style: 0,
        procedure: default_procedure,
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: 0,
        cursor: 0,
        background: 0,
        menu_name: std::ptr::null(),
        class_name: name.as_ptr(),
    };
    assert_ne!(unsafe { RegisterClassW(&class) }, 0);
    let window = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            wide("DRIP").as_ptr(),
            0,
            0,
            0,
            320,
            240,
            0,
            0,
            instance,
            std::ptr::null(),
        )
    };
    assert_ne!(window, 0);
    assert_eq!(unsafe { IsWindowVisible(window) }, 0);
    assert_eq!(crate::main_window(), window);
    assert_eq!(js_drip_shell_init(), 1);
    assert_eq!(js_drip_shell_init(), 1);
    js_drip_shell_paused(1);
    assert!(SHELL.with(|shell| shell.borrow().paused));

    unsafe {
        SendMessageW(window, WM_CLOSE, 0, 0);
        SendMessageW(window, WM_CLOSE, 0, 0);
        SendMessageW(window, WM_SYSCOMMAND, SC_MINIMIZE, 0);
        SendMessageW(window, WM_TRAY, 0, 0x0401);
    }
    assert_eq!(drain_commands(), "[\"close\",\"minimize\",\"open\"]");
    assert_eq!(drain_commands(), "[]");
    assert_ne!(unsafe { IsWindow(window) }, 0);
    assert_eq!(unsafe { IsWindowVisible(window) }, 0);

    crate::tray::remove(window);
    js_drip_shell_open();
    assert_ne!(unsafe { IsWindowVisible(window) }, 0);
    SHELL.with(|shell| shell.borrow_mut().tray_ready = false);
    assert_eq!(js_drip_shell_init(), 0);
    js_drip_shell_hide();
    assert_ne!(unsafe { IsWindowVisible(window) }, 0);
    let taskbar = SHELL.with(|shell| shell.borrow().taskbar_message);
    unsafe { SendMessageW(window, taskbar, 0, 0) };
    assert!(SHELL.with(|shell| shell.borrow().tray_ready));
    assert_eq!(js_drip_shell_init(), 1);
    assert_eq!(drain_commands(), "[]");

    assert_eq!(crate::instance::js_drip_instance_claim(), 1);
    assert_eq!(crate::instance::js_drip_instance_claim(), 1);
    assert_eq!(
        std::thread::spawn(|| crate::instance::js_drip_instance_claim())
            .join()
            .unwrap(),
        0
    );
    dispatch_pending(window);
    assert_eq!(drain_commands(), "[\"open\"]");
    crate::instance::js_drip_instance_release();

    assert_eq!(unsafe { SendMessageW(window, WM_QUERYENDSESSION, 0, 0) }, 1);
    assert_eq!(drain_commands(), "[\"shutdown\"]");
    js_drip_shell_hide();
    assert_eq!(unsafe { IsWindowVisible(window) }, 0);
    js_drip_shell_exit();
    dispatch_pending(window);
    assert_eq!(unsafe { IsWindow(window) }, 0);
    assert_ne!(unsafe { UnregisterClassW(name.as_ptr(), instance) }, 0);
}
