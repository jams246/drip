use std::sync::Arc;

use drip_client::{
    ActivityLog, Engine, EventKind, RunControl,
    credentials::{CredentialError, CredentialStore, Pairing},
    settings::SettingsStore,
};
use tokio::sync::{mpsc, watch};

use super::{Event, Initial, Request, worker::Worker};
use crate::gui::model::Binding;

pub(super) async fn run(
    mut requests: mpsc::Receiver<Request>,
    events: mpsc::Sender<Event>,
    control: watch::Sender<RunControl>,
    quit: watch::Sender<bool>,
) {
    if let Err(error) = serve(&mut requests, &events, &control, &quit).await {
        let _ = events.send(Event::Fatal(error)).await;
    }
}

async fn serve(
    requests: &mut mpsc::Receiver<Request>,
    events: &mpsc::Sender<Event>,
    control: &watch::Sender<RunControl>,
    quit: &watch::Sender<bool>,
) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("Executable has no data directory")?
        .to_owned();
    let engine = Arc::new(Engine::beside_executable().map_err(|error| error.to_string())?);
    let settings = engine.status().map_err(|error| error.to_string());
    let credentials = CredentialStore::beside_executable().map_err(|error| error.to_string())?;
    let pairing = credentials.load();
    let initial_binding = pairing.as_ref().map(binding).map_err(|error| match error {
        CredentialError::NotPaired => "Register this PC with a validated HTTPS server and a one-time token.".into(),
        _ => format!("Saved registration could not be loaded: {error}. Pairing again creates a new PC identity; existing mirrors remain intact."),
    });
    let logs = ActivityLog::new(&directory)
        .load_recent()
        .map(|events| events.into_iter().map(super::activity::row).collect())
        .map_err(|error| format!("Could not load retained activity logs: {error}"));
    let ready = settings.is_ok() && pairing.is_ok();
    let _ = events
        .send(Event::Initial(Box::new(Initial {
            settings,
            binding: initial_binding,
            logs,
        })))
        .await;
    let mut generation = 1;
    let mut worker = if ready {
        Some(
            Worker::start(
                engine.clone(),
                &directory,
                generation,
                events.clone(),
                control.subscribe(),
            )
            .await,
        )
    } else {
        None
    };
    while let Some(request) = requests.recv().await {
        match request {
            Request::Pair {
                server,
                label,
                token,
            } => {
                if let Some(running) = worker.take() {
                    let _ = events.send(Event::Message { settings: true, message: "Stopping the existing connection before pairing; waiting for its safe shutdown.".into(), busy: true }).await;
                    if let Some(error) = running.stop(control).await {
                        let _ = events.send(Event::Diagnostic(error)).await;
                    }
                }
                if *quit.borrow() {
                    break;
                }
                let result = engine.pair(&server, &label, token.expose_secret()).await;
                match result {
                    Ok(pairing) => {
                        let preference =
                            SettingsStore::open(&directory).save_pc_display_name(&label);
                        let _ = events.send(Event::Paired(binding(&pairing))).await;
                        if let Err(error) = preference {
                            let _ = events.send(Event::Message { settings: true, busy: false, message: format!("Registration succeeded. PC display name preference could not be saved: {error}") }).await;
                        }
                    }
                    Err(error) => {
                        let _ = events.send(Event::Message { settings: true, message: format!("Registration failed: {error}. Prior saved registration remains unchanged."), busy: false }).await;
                    }
                }
                if *quit.borrow() {
                    break;
                }
                if credentials.load().is_ok() {
                    control.send_modify(|desired| {
                        desired.stop = false;
                        desired.reconcile = desired.reconcile.wrapping_add(1);
                    });
                    generation = generation.wrapping_add(1);
                    worker = Some(
                        Worker::start(
                            engine.clone(),
                            &directory,
                            generation,
                            events.clone(),
                            control.subscribe(),
                        )
                        .await,
                    );
                }
            }
            Request::Add(path) => {
                let message = match engine.add(&path) {
                    Ok(added) => {
                        if let Some(running) = &worker {
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
                        let mut message = "Selection saved. Existing mirror data is retained until reconciliation confirms changes.".to_owned();
                        for retained in added.authority_over {
                            message.push_str(&format!(
                                " Ancestor now controls retained {} including confirmed deletions.",
                                retained.to_windows_path()
                            ));
                        }
                        message
                    }
                    Err(error) => format!("Could not add selection: {error}"),
                };
                reload(&engine, events, control).await;
                let _ = events
                    .send(Event::Message {
                        settings: false,
                        message,
                        busy: false,
                    })
                    .await;
            }
            Request::Remove(path) => {
                let message = match engine.remove(&path).await {
                    Ok(()) => "Selection removed; existing server copy retained.".into(),
                    Err(error) => format!(
                        "Removal is pending locally; existing mirror retained. Server barrier could not be confirmed: {error}"
                    ),
                };
                reload(&engine, events, control).await;
                let _ = events
                    .send(Event::Message {
                        settings: false,
                        message,
                        busy: false,
                    })
                    .await;
            }
            Request::Startup(enabled) => {
                let result = SettingsStore::open(&directory).set_start_at_sign_in(enabled);
                reload(&engine, events, control).await;
                let message = match result {
                    Ok(()) => if enabled {
                        "Start at sign-in enabled."
                    } else {
                        "Start at sign-in disabled."
                    }
                    .into(),
                    Err(error) => format!("Could not update start at sign-in: {error}"),
                };
                let _ = events
                    .send(Event::Message {
                        settings: true,
                        message,
                        busy: false,
                    })
                    .await;
            }
            Request::Quit => break,
        }
    }
    if let Some(running) = worker.take()
        && let Some(error) = running.stop(control).await
    {
        let _ = events.send(Event::Diagnostic(error)).await;
    }
    let _ = events.send(Event::ShutdownReady).await;
    Ok(())
}

async fn reload(
    engine: &Engine,
    events: &mpsc::Sender<Event>,
    control: &watch::Sender<RunControl>,
) {
    match engine.status() {
        Ok(settings) => {
            let _ = events.send(Event::Settings(settings)).await;
        }
        Err(error) => {
            let _ = events.send(Event::Diagnostic(error.to_string())).await;
        }
    }
    control.send_modify(|desired| desired.reconcile = desired.reconcile.wrapping_add(1));
}

fn binding(pairing: &Pairing) -> Binding {
    Binding {
        server: pairing.server_url().to_owned(),
        pc_id: pairing.pc_id().to_owned(),
        mirror_directory: pairing.mirror_directory().to_owned(),
    }
}
