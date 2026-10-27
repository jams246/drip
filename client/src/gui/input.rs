mod actions;
mod edit;
mod element;
mod handler;
mod model;
mod render;
mod text;

#[cfg(test)]
mod tests;

pub(super) use model::Input;

pub(super) fn init(cx: &mut gpui::App) {
    actions::init(cx);
}
