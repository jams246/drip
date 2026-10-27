use gpui::{App, KeyBinding, actions};

actions!(
    drip_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("DripInput")),
        KeyBinding::new("delete", Delete, Some("DripInput")),
        KeyBinding::new("left", Left, Some("DripInput")),
        KeyBinding::new("right", Right, Some("DripInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("DripInput")),
        KeyBinding::new("shift-right", SelectRight, Some("DripInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("DripInput")),
        KeyBinding::new("ctrl-v", Paste, Some("DripInput")),
        KeyBinding::new("ctrl-c", Copy, Some("DripInput")),
        KeyBinding::new("ctrl-x", Cut, Some("DripInput")),
        KeyBinding::new("home", Home, Some("DripInput")),
        KeyBinding::new("end", End, Some("DripInput")),
    ]);
}
