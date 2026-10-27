use std::{path::PathBuf, time::Instant};

use gpui::{Context, PathPromptOptions, Window};

use super::{
    model::{Shell, Tab},
    runtime::Request,
};

impl Shell {
    pub(super) fn select_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        window.focus(&self.tab_focus[tab.index()], cx);
        cx.notify();
    }

    pub(super) fn footer_label(&self, now: Instant) -> &'static str {
        self.state.status.footer_label(now)
    }

    pub(super) fn pair(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let request = Request::Pair {
            server: self.server.read(cx).value().to_owned(),
            label: self.pc_name.read(cx).value().to_owned(),
            token: drip_core::SensitiveString::new(self.token.read(cx).value()),
        };
        self.request(request, true, cx);
    }

    pub(super) fn toggle_startup(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.request(
            Request::Startup(!self.state.settings.start_at_sign_in),
            true,
            cx,
        );
    }

    pub(super) fn add_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add(false, window, cx);
    }
    pub(super) fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add(true, window, cx);
    }

    fn add(&mut self, directory: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.state.busy || self.state.loading || self.state.quitting {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(if directory { "Add folder" } else { "Add file" }.into()),
        });
        cx.spawn(async move |this, cx| {
            match paths.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| this.request(Request::Add(path), false, cx));
                    }
                }
                Ok(Ok(None)) => {}
                _ => { let _ = this.update(cx, |this, cx| { this.state.watch_message = "Native file selector could not be opened. Check Windows shell access and retry.".into(); cx.notify(); }); }
            }
        }).detach();
    }

    pub(super) fn remove(&mut self, path: PathBuf, _window: &mut Window, cx: &mut Context<Self>) {
        self.request(Request::Remove(path), false, cx);
    }

    fn request(&mut self, request: Request, settings: bool, cx: &mut Context<Self>) {
        if self.state.busy || self.state.loading || self.state.quitting {
            return;
        }
        match self.requests.try_send(request) {
            Ok(()) => {
                self.state.busy = true;
                if settings { self.state.settings_message = "Working...".into(); } else { self.state.watch_message = "Working...".into(); }
            }
            Err(_) => self.state.diagnostic = "Application worker unavailable or busy. Wait for the current action or restart Drip after safe shutdown.".into(),
        }
        cx.notify();
    }

    pub(super) fn pause(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.state.binding.is_none() || self.state.quitting {
            return;
        }
        self.control
            .send_modify(|desired| desired.paused = !desired.paused);
        cx.notify();
    }

    pub(super) fn quit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.state.quitting = true;
        self.control.send_modify(|desired| desired.stop = true);
        let _ = self.quit.send(true);
        if self.requests.is_closed() {
            cx.quit();
        }
        cx.notify();
    }
}
