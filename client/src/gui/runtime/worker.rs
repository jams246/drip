use std::{path::Path, sync::Arc};

use drip_client::{Engine, EngineError, EventSink, RunControl};
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
            let cause = result.as_ref().err();
            let registration_required = cause.is_some_and(EngineError::registration_required);
            let error = cause
                .filter(|error| !matches!(error, EngineError::Cancelled))
                .map(|error| crate::gui::feedback::sync(error, registration_required));
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

    pub(super) async fn stop(
        self,
        control: &watch::Sender<RunControl>,
    ) -> Option<crate::gui::model::Feedback> {
        control.send_modify(|desired| desired.stop = true);
        let Self {
            sink,
            task,
            activity,
            status,
            progress,
        } = self;
        let error = match task.await {
            Ok(Err(EngineError::Cancelled)) | Ok(Ok(())) => None,
            Ok(Err(error)) => {
                let registration_required = error.registration_required();
                Some(crate::gui::feedback::sync(&error, registration_required))
            }
            Err(error) => Some(crate::gui::feedback::action(
                "Sync worker stopped unexpectedly. Open Activity for details.",
                crate::gui::model::FeedbackSeverity::Error,
                Some(error.to_string()),
            )),
        };
        drop(sink);
        status.abort();
        progress.abort();
        let _ = activity.await;
        error
    }
}
