use std::{path::PathBuf, time::Instant};

use gpui::{Context, PathPromptOptions, Window};

use super::{
    feedback,
    model::{FeedbackSeverity, Shell, Tab},
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
        self.request(request, true, true, cx);
    }

    pub(super) fn toggle_startup(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.request(
            Request::Startup(!self.state.settings.start_at_sign_in),
            true,
            false,
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
            let result = paths.await;
            match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| {
                            this.request(Request::Add(path), false, false, cx)
                        });
                    }
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.state.watch_feedback = Some(feedback::action(
                            "Could not open the file picker. Check Windows shell access and try again.",
                            FeedbackSeverity::Error,
                            Some(error.to_string()),
                        ));
                        cx.notify();
                    });
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.state.watch_feedback = Some(feedback::action(
                            "Could not open the file picker. Check Windows shell access and try again.",
                            FeedbackSeverity::Error,
                            Some(error.to_string()),
                        ));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn remove(&mut self, path: PathBuf, _window: &mut Window, cx: &mut Context<Self>) {
        self.request(Request::Remove(path), false, false, cx);
    }

    fn request(
        &mut self,
        request: Request,
        settings: bool,
        connecting: bool,
        cx: &mut Context<Self>,
    ) {
        if self.state.busy || self.state.loading || self.state.quitting {
            return;
        }
        match self.requests.try_send(request) {
            Ok(()) => {
                self.state.busy = true;
                self.state.connecting = connecting;
                let (summary, page_feedback) = if connecting {
                    ("Connecting...", &mut self.state.settings_feedback)
                } else if settings {
                    (
                        "Saving sign-in preference...",
                        &mut self.state.settings_feedback,
                    )
                } else {
                    ("Updating Watch...", &mut self.state.watch_feedback)
                };
                *page_feedback = Some(feedback::action(summary, FeedbackSeverity::Info, None));
            }
            Err(_) => {
                self.state.connecting = false;
                let summary = if settings {
                    "Could not start this settings action. Try again or open Activity for details."
                } else {
                    "Could not start this Watch action. Try again or open Activity for details."
                };
                let result = feedback::action(
                    summary,
                    FeedbackSeverity::Error,
                    Some("The application worker is unavailable or busy.".into()),
                );
                if settings {
                    self.state.settings_feedback = Some(result);
                } else {
                    self.state.watch_feedback = Some(result);
                }
            }
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
