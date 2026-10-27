use std::{path::Path, sync::Arc};

use drip_client::{Engine, EventSink, RunControl};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use super::Event;

pub(super) struct Worker {
    pub sink: EventSink,
    task: JoinHandle<Result<(), drip_client::EngineError>>,
    activity: JoinHandle<()>,
    status: JoinHandle<()>,
    progress: JoinHandle<()>,
}

impl Worker {
    pub(super) async fn start(
        engine: Arc<Engine>,
        directory: &Path,
        generation: u64,
        events: mpsc::Sender<Event>,
        control: watch::Receiver<RunControl>,
    ) -> Self {
        let (lifecycle, received) = mpsc::channel(64);
        let (progress, mut latest) = watch::channel(None);
        let sink = EventSink::new(lifecycle, progress).with_log(directory);
        let mut state = sink.subscribe_status();
        let _ = events.send(Event::WorkerStarted(generation)).await;
        let activity = tokio::spawn(super::activity::forward(
            received,
            generation,
            events.clone(),
        ));
        let progress_events = events.clone();
        let progress = tokio::spawn(async move {
            while latest.changed().await.is_ok() {
                let progress = latest.borrow_and_update().clone().map(Box::new);
                if progress_events
                    .send(Event::Progress {
                        generation,
                        progress,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let status_events = events.clone();
        let status = tokio::spawn(async move {
            while state.changed().await.is_ok() {
                let status = state.borrow_and_update().clone();
                if status_events
                    .send(Event::Status { generation, status })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let engine_sink = sink.clone();
        let task = tokio::spawn(async move {
            let result = engine.run(control, engine_sink).await;
            let registration_required = result
                .as_ref()
                .err()
                .is_some_and(|error| error.registration_required());
            let error = result.as_ref().err().map(ToString::to_string);
            let _ = events
                .send(Event::WorkerEnded {
                    generation,
                    error,
                    registration_required,
                })
                .await;
            result
        });
        Self {
            sink,
            task,
            activity,
            status,
            progress,
        }
    }

    pub(super) async fn stop(self, control: &watch::Sender<RunControl>) -> Option<String> {
        control.send_modify(|desired| desired.stop = true);
        let Self {
            sink,
            task,
            activity,
            status,
            progress,
        } = self;
        let error = match task.await {
            Ok(result) => result.err().map(|error| error.to_string()),
            Err(error) => Some(format!("Sync worker stopped unexpectedly: {error}")),
        };
        drop(sink);
        status.abort();
        progress.abort();
        let _ = activity.await;
        error
    }
}
