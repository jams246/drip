mod activity;
mod actor;
mod worker;

use std::path::PathBuf;

use drip_client::{RunControl, SelectionProgress, SyncSnapshot, settings::ClientSettings};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use super::model::{Binding, Feedback, LogRow};

pub(super) enum Request {
    Pair {
        server: String,
        label: String,
        token: drip_core::SensitiveString,
    },
    Add(PathBuf),
    Remove(PathBuf),
    Startup(bool),
    Quit,
}

pub(super) struct Initial {
    pub settings: Result<ClientSettings, Feedback>,
    pub binding: Result<Binding, Feedback>,
    pub logs: Result<Vec<LogRow>, Feedback>,
}

pub(super) enum Event {
    Initial(Box<Initial>),
    WorkerStarted(u64),
    Status {
        generation: u64,
        status: SyncSnapshot,
    },
    Progress {
        generation: u64,
        progress: Option<Box<SelectionProgress>>,
    },
    Activity(Box<LogRow>),
    Settings(ClientSettings),
    Paired(Binding),
    Message {
        settings: bool,
        feedback: Feedback,
        busy: bool,
    },
    Diagnostic(Feedback),
    Fatal(Feedback),
    WorkerEnded {
        generation: u64,
        error: Option<Feedback>,
        registration_required: bool,
    },
    ShutdownReady,
}

pub(super) fn start(
    requests: mpsc::Receiver<Request>,
    events: mpsc::Sender<Event>,
    control: watch::Sender<RunControl>,
    quit: watch::Sender<bool>,
) -> JoinHandle<()> {
    tokio::spawn(actor::run(requests, events, control, quit))
}
