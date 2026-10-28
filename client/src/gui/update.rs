use drip_client::SyncSnapshot;
use gpui::{Context, Focusable, Window};

use super::{
    feedback,
    model::{Feedback, FeedbackSeverity, Shell, Tab},
    native,
    runtime::Event,
};

impl Shell {
    pub(super) fn update_event(
        &mut self,
        event: Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            Event::Initial(initial) => {
                self.state.loading = false;
                match initial.settings {
                    Ok(settings) => {
                        if let Some(name) = &settings.pc_display_name {
                            self.pc_name
                                .update(cx, |input, cx| input.set_value(name.clone(), cx));
                        }
                        self.state.settings = settings;
                    }
                    Err(error) => {
                        self.state.diagnostic = Some(error);
                        self.state.settings_feedback = Some(Feedback::new(
                            "Settings could not be loaded. Preserve drip.json and open Activity for details.",
                            FeedbackSeverity::Error,
                            None,
                        ));
                    }
                }
                match initial.logs {
                    Ok(logs) => {
                        for row in logs {
                            self.state.activity(row);
                        }
                    }
                    Err(error) => self.state.diagnostic = Some(error),
                }
                match initial.binding {
                    Ok(binding) => {
                        if self.server.read(cx).value().is_empty() {
                            self.server.update(cx, |input, cx| {
                                input.set_value(binding.server.clone(), cx)
                            });
                        }
                        self.state.settings_feedback = Some(Feedback::new(
                            "Saved registration loaded. Current reachability appears in the footer.",
                            FeedbackSeverity::Info,
                            Some(format!(
                                "Computer ID: {}. Server folder: /mirror/{}. Credential stored with user-scoped Windows DPAPI.",
                                binding.pc_id, binding.mirror_directory
                            )),
                        ));
                        self.state.binding = Some(binding);
                        self.tab = Tab::Home;
                        window.focus(&self.tab_focus[self.tab.index()], cx);
                        if self.opened {
                            self.show(window, cx);
                        }
                    }
                    Err(feedback) => {
                        self.state.settings_feedback = Some(feedback);
                        self.tab = Tab::Settings;
                        self.opened = true;
                        self.show(window, cx);
                        window.focus(&self.server.focus_handle(cx), cx);
                    }
                }
            }
            Event::WorkerStarted(generation) => self.state.worker_started(generation),
            Event::Status { generation, status } => self.state.status(generation, status),
            Event::Progress {
                generation,
                progress,
            } => self
                .state
                .progress(generation, progress.map(|progress| *progress)),
            Event::Activity(row) => self.state.activity(*row),
            Event::Settings(settings) => self.state.settings = settings,
            Event::Paired(binding) => {
                self.server
                    .update(cx, |input, cx| input.set_value(binding.server.clone(), cx));
                self.token.update(cx, |input, cx| input.clear(cx));
                self.state.settings_feedback = Some(Feedback::new(
                    "Computer registration saved. Current reachability appears in the footer.",
                    FeedbackSeverity::Success,
                    Some(format!(
                        "Computer ID: {}. Server folder: /mirror/{}. Credential stored with user-scoped Windows DPAPI.",
                        binding.pc_id, binding.mirror_directory
                    )),
                ));
                self.state.binding = Some(binding);
                self.state.busy = false;
                self.state.connecting = false;
                self.select_tab(Tab::Home, window, cx);
            }
            Event::Message {
                settings,
                feedback,
                busy,
            } => {
                self.state.busy = busy;
                if !busy {
                    self.state.connecting = false;
                }
                if settings {
                    self.state.settings_feedback = Some(feedback);
                } else {
                    self.state.watch_feedback = Some(feedback);
                }
            }
            Event::Diagnostic(feedback) => self.state.diagnostic = Some(feedback),
            Event::Fatal(feedback) => {
                self.state.diagnostic = Some(feedback.clone());
                self.state.settings_feedback = Some(Feedback::new(
                    "Drip could not start. Open Activity for technical details.",
                    FeedbackSeverity::Error,
                    None,
                ));
                self.state.loading = false;
                self.state.busy = false;
                self.state.connecting = false;
                self.tab = Tab::Settings;
                self.opened = true;
                self.show(window, cx);
            }
            Event::WorkerEnded {
                generation,
                error,
                registration_required,
            } if generation == self.state.worker_generation => {
                self.state.progress = None;
                if let Some(error) = error {
                    self.state.status = SyncSnapshot::default();
                    if registration_required {
                        self.state.settings_feedback = Some(Feedback::new(
                            "Registration needs attention. Connect again with a valid one-time token.",
                            FeedbackSeverity::Error,
                            error.detail,
                        ));
                        self.tab = Tab::Settings;
                        self.opened = true;
                        self.show(window, cx);
                        window.focus(&self.server.focus_handle(cx), cx);
                    } else {
                        self.state.diagnostic = Some(error);
                    }
                }
            }
            Event::WorkerEnded { .. } => {}
            Event::ShutdownReady => return true,
        }
        cx.notify();
        false
    }

    pub(super) fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = native::show(window) {
            self.state.diagnostic = Some(feedback::diagnostic(error.to_string()));
        }
        cx.notify();
    }

    pub(super) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.opened && self.state.binding.is_some() {
            self.tab = Tab::Home;
            window.focus(&self.tab_focus[self.tab.index()], cx);
        }
        self.opened = true;
        self.show(window, cx);
        cx.notify();
    }
}
