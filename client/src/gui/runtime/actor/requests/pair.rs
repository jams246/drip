use drip_client::{credentials::Pairing, settings::SettingsStore};

use super::super::super::Event;
use super::RequestHandler;
use crate::gui::{
    feedback,
    model::{Binding, FeedbackSeverity},
};

impl RequestHandler<'_> {
    pub(super) async fn pair(
        &mut self,
        server: String,
        label: String,
        token: drip_core::SensitiveString,
    ) -> bool {
        if let Some(running) = self.worker.take() {
            let _ = self
                .events
                .send(Event::Message {
                    settings: true,
                    feedback: feedback::action(
                        "Connecting... Waiting for the existing connection to stop safely.",
                        FeedbackSeverity::Info,
                        None,
                    ),
                    busy: true,
                })
                .await;
            if let Some(error) = running.stop(self.control).await {
                let _ = self.events.send(Event::Diagnostic(error)).await;
            }
        }
        if *self.quit.borrow() {
            return true;
        }
        match self
            .engine
            .pair(&server, &label, token.expose_secret())
            .await
        {
            Ok(pairing) => {
                let preference = SettingsStore::open(self.directory).save_pc_display_name(&label);
                let connection = binding(&pairing);
                let _ = self.events.send(Event::Paired(connection.clone())).await;
                if let Err(error) = preference {
                    let detail = format!(
                        "Computer ID: {}. Server folder: /mirror/{}. {error}",
                        connection.pc_id, connection.mirror_directory
                    );
                    let _ = self
                        .events
                        .send(Event::Message {
                            settings: true,
                            busy: false,
                            feedback: feedback::action(
                                "Connected. Computer name could not be saved.",
                                FeedbackSeverity::Warning,
                                Some(detail),
                            ),
                        })
                        .await;
                }
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(Event::Message {
                        settings: true,
                        feedback: feedback::pairing(&error),
                        busy: false,
                    })
                    .await;
            }
        }
        if *self.quit.borrow() {
            return true;
        }
        if self.credentials.load().is_ok() {
            self.control.send_modify(|desired| {
                desired.stop = false;
                desired.reconcile = desired.reconcile.wrapping_add(1);
            });
            *self.generation = self.generation.wrapping_add(1);
            *self.worker = Some(
                super::Worker::start(
                    self.engine.clone(),
                    self.directory,
                    *self.generation,
                    self.events.clone(),
                    self.control.subscribe(),
                )
                .await,
            );
        }
        false
    }
}

pub(in crate::gui::runtime::actor) fn binding(pairing: &Pairing) -> Binding {
    Binding {
        server: pairing.server_url().to_owned(),
        pc_id: pairing.pc_id().to_owned(),
        mirror_directory: pairing.mirror_directory().to_owned(),
    }
}
