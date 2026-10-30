mod pair;

use std::{path::Path, sync::Arc};

use drip_client::{
    Engine, EventKind, RunControl, credentials::CredentialStore, settings::SettingsStore,
};
use tokio::sync::{mpsc, watch};

use super::super::{Event, Request, worker::Worker};
use crate::gui::{feedback, model::FeedbackSeverity};

pub(super) use pair::binding;

pub(super) struct RequestHandler<'a> {
    pub(super) engine: &'a Arc<Engine>,
    pub(super) directory: &'a Path,
    pub(super) credentials: &'a CredentialStore,
    pub(super) worker: &'a mut Option<Worker>,
    pub(super) generation: &'a mut u64,
    pub(super) events: &'a mpsc::Sender<Event>,
    pub(super) control: &'a watch::Sender<RunControl>,
    pub(super) quit: &'a watch::Sender<bool>,
}

impl RequestHandler<'_> {
    pub(super) async fn handle(&mut self, request: Request) -> bool {
        match request {
            Request::Pair {
                server,
                label,
                token,
            } => self.pair(server, label, token).await,
            Request::Add(path) => {
                self.add(path).await;
                false
            }
            Request::Remove(path) => {
                self.remove(path).await;
                false
            }
            Request::Startup(enabled) => {
                self.startup(enabled).await;
                false
            }
            Request::Quit => true,
        }
    }

    async fn add(&mut self, path: std::path::PathBuf) {
        let result = self.engine.add(&path);
        let message = match result {
            Ok(added) => {
                if let Some(running) = &self.worker {
                    let _ = running
                        .sink
                        .emit(
                            EventKind::SelectionChanged,
                            Some(&added.selection.id),
                            Some(&added.selection.source),
                            "selection-add",
                            "Selection saved; reconciliation pending",
                        )
                        .await;
                }
                if added.authority_over.is_empty() {
                    None
                } else {
                    let retained = added
                        .authority_over
                        .iter()
                        .map(|path| path.to_windows_path())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Some(feedback::action(
                        "This folder now controls retained items, including their changes and deletions.",
                        FeedbackSeverity::Warning,
                        Some(format!("Affected retained items: {retained}")),
                    ))
                }
            }
            Err(error) => Some(feedback::add_selection(&error)),
        };
        self.reload().await;
        self.refresh_selections();
        let _ = self
            .events
            .send(Event::Message {
                settings: false,
                feedback: message,
                busy: false,
            })
            .await;
    }

    async fn remove(&mut self, path: std::path::PathBuf) {
        let message = self
            .engine
            .remove(&path)
            .await
            .err()
            .map(|error| feedback::remove_selection(&error));
        self.reload().await;
        self.refresh_selections();
        let _ = self
            .events
            .send(Event::Message {
                settings: false,
                feedback: message,
                busy: false,
            })
            .await;
    }

    async fn startup(&mut self, enabled: bool) {
        let result = SettingsStore::open(self.directory).set_start_at_sign_in(enabled);
        self.reload().await;
        let message = match result {
            Ok(()) => feedback::action(
                if enabled {
                    "Start at sign-in enabled."
                } else {
                    "Start at sign-in disabled."
                },
                FeedbackSeverity::Success,
                Some("Windows sign-in preference saved.".into()),
            ),
            Err(error) => feedback::startup(&error),
        };
        let _ = self
            .events
            .send(Event::Message {
                settings: true,
                feedback: Some(message),
                busy: false,
            })
            .await;
    }

    async fn reload(&self) {
        match self.engine.status() {
            Ok(settings) => {
                let _ = self.events.send(Event::Settings(settings)).await;
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(Event::Diagnostic(feedback::diagnostic(error.to_string())))
                    .await;
            }
        }
    }

    fn refresh_selections(&self) {
        self.control.send_modify(|desired| {
            desired.refresh_selections = desired.refresh_selections.wrapping_add(1);
        });
    }
}
