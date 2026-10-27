#![expect(
    unsafe_code,
    reason = "Diagnostic mode attaches to an existing Windows console"
)]

use std::ffi::OsString;

use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

pub(super) fn prepare() {
    if diagnostic_arguments(std::env::args_os().skip(1)) {
        // GUI launches must not create a second console window.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn diagnostic_arguments(arguments: impl Iterator<Item = OsString>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument != "--background")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_and_background_launches_do_not_attach_a_console() {
        assert!(!diagnostic_arguments([].into_iter()));
        assert!(!diagnostic_arguments(
            [OsString::from("--background")].into_iter()
        ));
        assert!(diagnostic_arguments([OsString::from("--help")].into_iter()));
        assert!(diagnostic_arguments([OsString::from("run")].into_iter()));
    }
}
