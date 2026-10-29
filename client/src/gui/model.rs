mod events;

use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};

use drip_client::{
    LifecycleEvent, RunControl, SelectionProgress, SyncSnapshot, settings::ClientSettings,
};
use gpui::{AppContext, Context, Entity, FocusHandle, ScrollHandle};
use tokio::sync::{mpsc, watch};

use super::{input::Input, runtime::Request};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FeedbackSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone)]
pub(super) struct Feedback {
    pub summary: String,
    pub severity: FeedbackSeverity,
    pub detail: Option<String>,
}

impl Feedback {
    pub(super) fn new(
        summary: impl Into<String>,
        severity: FeedbackSeverity,
        detail: Option<String>,
    ) -> Self {
        Self {
            summary: summary.into(),
            severity,
            detail,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Tab {
    Home,
    Watch,
    Activity,
    Settings,
}

impl Tab {
    pub(super) const ALL: [Self; 4] = [Self::Home, Self::Watch, Self::Activity, Self::Settings];
    pub(super) fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Watch => "Watch",
            Self::Activity => "Activity",
            Self::Settings => "Settings",
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Watch => "watch",
            Self::Activity => "logs",
            Self::Settings => "settings",
        }
    }
}

#[derive(Clone)]
pub(super) struct Binding {
    pub server: String,
    pub pc_id: String,
    pub mirror_directory: String,
}

pub(super) struct LogRow {
    pub event: LifecycleEvent,
    pub local_time: String,
}

pub(super) struct UiState {
    pub settings: ClientSettings,
    pub binding: Option<Binding>,
    pub status: SyncSnapshot,
    pub progress: Option<SelectionProgress>,
    pub logs: VecDeque<LogRow>,
    pub last_file: HashMap<String, LifecycleEvent>,
    pub finished_operations: VecDeque<String>,
    pub worker_generation: u64,
    pub loading: bool,
    pub busy: bool,
    pub connecting: bool,
    pub quitting: bool,
    pub diagnostic: Option<Feedback>,
    pub watch_feedback: Option<Feedback>,
    pub settings_feedback: Option<Feedback>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            settings: ClientSettings::default(),
            binding: None,
            status: SyncSnapshot::default(),
            progress: None,
            logs: VecDeque::new(),
            last_file: HashMap::new(),
            finished_operations: VecDeque::new(),
            worker_generation: 0,
            loading: true,
            busy: false,
            connecting: false,
            quitting: false,
            diagnostic: None,
            watch_feedback: None,
            settings_feedback: None,
        }
    }
}

impl UiState {
    pub(super) fn connection_at(&self, now: Instant) -> drip_client::ConnectionState {
        if self.status.footer_label(now) == "Disconnected" {
            drip_client::ConnectionState::Disconnected
        } else {
            self.status.connection
        }
    }
}

pub(super) struct Shell {
    pub tab: Tab,
    pub state: UiState,
    pub server: Entity<Input>,
    pub token: Entity<Input>,
    pub pc_name: Entity<Input>,
    pub focus: FocusHandle,
    pub tab_focus: [FocusHandle; 4],
    pub tab_scroll: [ScrollHandle; 4],
    pub requests: mpsc::Sender<Request>,
    pub control: watch::Sender<RunControl>,
    pub quit: watch::Sender<bool>,
    pub opened: bool,
}

impl Shell {
    pub(super) fn new(
        requests: mpsc::Sender<Request>,
        control: watch::Sender<RunControl>,
        quit: watch::Sender<bool>,
        cx: &mut Context<Self>,
    ) -> Self {
        let pc_name = cx.new(|cx| Input::new("pc-display-name", false, cx));
        pc_name.update(cx, |input, cx| {
            input.set_value(
                std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into()),
                cx,
            )
        });
        Self {
            tab: Tab::Settings,
            state: UiState::default(),
            server: cx.new(|cx| Input::new("server-url", false, cx)),
            token: cx.new(|cx| Input::new("registration-token", true, cx)),
            pc_name,
            focus: cx.focus_handle().tab_stop(true),
            tab_focus: std::array::from_fn(|_| cx.focus_handle()),
            tab_scroll: std::array::from_fn(|_| ScrollHandle::new()),
            requests,
            control,
            quit,
            opened: false,
        }
    }
}
