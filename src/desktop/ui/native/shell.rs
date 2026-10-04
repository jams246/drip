use crate::native_abi::{StringHeader, write_string};
use crate::shell_win32::*;
use std::cell::RefCell;

#[derive(Default)]
struct Shell {
    window: isize,
    taskbar_message: u32,
    activation_message: u32,
    paused: bool,
    tray_ready: bool,
    commands: Vec<&'static str>,
}

thread_local! {
    static SHELL: RefCell<Shell> = RefCell::new(Shell::default());
}

fn enqueue(command: &'static str) {
    SHELL.with(|shell| {
        let mut shell = shell.borrow_mut();
        if shell.commands.last() != Some(&command) {
            shell.commands.push(command);
        }
    });
}

fn open_window(window: isize) {
    if window != 0 {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
}

unsafe extern "system" fn subclass(
    window: isize,
    message: u32,
    parameter: usize,
    value: isize,
    _id: usize,
    _data: usize,
) -> isize {
    let (taskbar, activation, paused) = SHELL.with(|shell| {
        let shell = shell.borrow();
        (
            shell.taskbar_message,
            shell.activation_message,
            shell.paused,
        )
    });
    if message == activation && activation != 0 {
        enqueue("open");
        return 0;
    }
    if message == taskbar && taskbar != 0 {
        let ready = crate::tray::add(window);
        SHELL.with(|shell| shell.borrow_mut().tray_ready = ready);
        if !ready {
            open_window(window);
            enqueue("tray-error");
        }
        return 0;
    }
    match message {
        WM_CLOSE => {
            enqueue("close");
            0
        }
        WM_SYSCOMMAND if parameter & 0xfff0 == SC_MINIMIZE => {
            enqueue("minimize");
            0
        }
        WM_QUERYENDSESSION => {
            enqueue("shutdown");
            1
        }
        WM_ENDSESSION if parameter != 0 => {
            enqueue("shutdown");
            0
        }
        WM_TRAY => {
            match value as u32 & 0xffff {
                0x0202 | 0x0203 | 0x0400 | 0x0401 => enqueue("open"),
                0x007b | 0x0205 => {
                    if let Some(command) = crate::tray::menu(window, paused) {
                        enqueue(command);
                    }
                }
                _ => {}
            }
            0
        }
        WM_NCDESTROY => {
            crate::tray::remove(window);
            SHELL.with(|shell| *shell.borrow_mut() = Shell::default());
            unsafe { RemoveWindowSubclass(window, subclass, SUBCLASS_ID) };
            unsafe { DefSubclassProc(window, message, parameter, value) }
        }
        _ => {
            if message == WM_SIZE && parameter == 1 {
                enqueue("minimize");
            }
            unsafe { DefSubclassProc(window, message, parameter, value) }
        }
    }
}

fn attach(window: isize) -> bool {
    if window == 0 {
        return false;
    }
    let taskbar_message = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    let activation_message = crate::instance::activation_message();
    if taskbar_message == 0 || activation_message == 0 {
        return false;
    }
    if unsafe { SetWindowSubclass(window, subclass, SUBCLASS_ID, 0) } == 0 {
        return false;
    }
    let tray_ready = crate::tray::add(window);
    SHELL.with(|shell| {
        *shell.borrow_mut() = Shell {
            window,
            taskbar_message,
            activation_message,
            tray_ready,
            ..Shell::default()
        };
    });
    tray_ready
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_init() -> i32 {
    let window = SHELL.with(|shell| shell.borrow().window);
    if window != 0 {
        return i32::from(SHELL.with(|shell| shell.borrow().tray_ready));
    }
    i32::from(attach(crate::main_window()))
}

fn drain_commands() -> String {
    let commands = SHELL.with(|shell| std::mem::take(&mut shell.borrow_mut().commands));
    let quoted: Vec<String> = commands
        .iter()
        .map(|command| format!("\"{command}\""))
        .collect();
    format!("[{}]", quoted.join(","))
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_drain() -> *mut StringHeader {
    write_string(&drain_commands())
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_hide() {
    let (window, ready) = SHELL.with(|shell| {
        let shell = shell.borrow();
        (shell.window, shell.tray_ready)
    });
    if ready {
        unsafe { ShowWindow(window, SW_HIDE) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_open() {
    open_window(SHELL.with(|shell| shell.borrow().window));
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_paused(paused: i32) {
    SHELL.with(|shell| shell.borrow_mut().paused = paused != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn js_drip_shell_exit() {
    let window = SHELL.with(|shell| {
        let mut shell = shell.borrow_mut();
        let window = shell.window;
        *shell = Shell::default();
        window
    });
    if window != 0 {
        crate::tray::remove(window);
        unsafe {
            RemoveWindowSubclass(window, subclass, SUBCLASS_ID);
            PostMessageW(window, WM_CLOSE, 0, 0);
        }
    }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
