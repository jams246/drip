mod requests;

use std::sync::Arc;

use drip_client::{ActivityLog, Engine, RunControl, credentials::CredentialStore};
use tokio::sync::{mpsc, watch};

use super::{Event, Initial, Request, worker::Worker};
use crate::gui::{
    feedback,
    model::{Feedback, FeedbackSeverity},
};

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
) -> Result<(), Feedback> {
    let executable =
        std::env::current_exe().map_err(|error| feedback::diagnostic(error.to_string()))?;
    let directory = executable
        .parent()
        .ok_or_else(|| {
            feedback::action(
                "Drip could not locate its data folder.",
                FeedbackSeverity::Error,
                None,
            )
        })?
        .to_owned();
    let engine = Arc::new(
        Engine::beside_executable().map_err(|error| feedback::diagnostic(error.to_string()))?,
    );
    let settings = engine
        .status()
        .map_err(|error| feedback::settings_load(&error));
    let credentials = CredentialStore::beside_executable()
        .map_err(|error| feedback::diagnostic(error.to_string()))?;
    let pairing = credentials.load();
    let initial_binding = pairing
        .as_ref()
        .map(requests::binding)
        .map_err(feedback::credential_load);
    let logs = ActivityLog::new(&directory)
        .load_recent()
        .map(|events| events.into_iter().map(super::activity::row).collect())
        .map_err(|error| {
            feedback::diagnostic(format!("Could not load retained activity logs: {error}"))
        });
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
    {
        let mut handler = requests::RequestHandler {
            engine: &engine,
            directory: &directory,
            credentials: &credentials,
            worker: &mut worker,
            generation: &mut generation,
            events,
            control,
            quit,
        };
        while let Some(request) = requests.recv().await {
            if handler.handle(request).await {
                break;
            }
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
