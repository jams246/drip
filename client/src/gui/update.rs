use drip_client::SyncSnapshot;
use gpui::{Context, Focusable, Window};

use super::{
    model::{Shell, Tab},
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
                        self.state.diagnostic = format!(
                            "Could not load drip.json: {error}. Preserve the file and repair its permissions or contents before syncing."
                        )
                    }
                }
                match initial.logs {
                    Ok(logs) => {
                        for row in logs {
                            self.state.activity(row);
                        }
                    }
                    Err(error) => self.state.diagnostic = error,
                }
                match initial.binding {
                    Ok(binding) => {
                        if self.server.read(cx).value().is_empty() {
                            self.server.update(cx, |input, cx| {
                                input.set_value(binding.server.clone(), cx)
                            });
                        }
                        self.state.binding = Some(binding);
                        self.tab = Tab::Home;
                        if self.opened {
                            self.show(window, cx);
                        }
                    }
                    Err(message) => {
                        self.state.settings_message = message;
                        self.tab = Tab::Settings;
                        self.opened = true;
                        self.show(window, cx);
                        window.focus(&self.server.focus_handle(cx), cx);
                    }
                }
            }
            Event::WorkerStarted(generation) => {
                self.state.worker_generation = generation;
                self.state.status = SyncSnapshot::default();
                self.state.progress = None;
                self.state.last_file.clear();
                self.state.finished_operations.clear();
            }
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
                self.state.settings_message = format!(
                    "Registered PC {}. Credential saved with user-scoped Windows DPAPI.",
                    binding.pc_id
                );
                self.state.binding = Some(binding);
                self.state.busy = false;
                self.select_tab(Tab::Home, window, cx);
            }
            Event::Message {
                settings,
                message,
                busy,
            } => {
                self.state.busy = busy;
                if settings {
                    self.state.settings_message = message;
                } else {
                    self.state.watch_message = message;
                }
            }
            Event::Diagnostic(message) => self.state.diagnostic = message,
            Event::Fatal(message) => {
                self.state.diagnostic = message;
                self.state.loading = false;
                self.state.busy = false;
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
                        self.state.settings_message = format!(
                            "Registration was rejected: {error}. Verify the server URL, then pair again with a valid registration token."
                        );
                        self.tab = Tab::Settings;
                        self.opened = true;
                        self.show(window, cx);
                        window.focus(&self.server.focus_handle(cx), cx);
                    } else {
                        self.state.diagnostic = format!(
                            "Sync stopped: {error}. Resolve the reported error, then retry."
                        );
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
            self.state.diagnostic = error.to_string();
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
