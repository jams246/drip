mod events;

use std::collections::{HashMap, VecDeque};

use drip_client::{
    LifecycleEvent, RunControl, SelectionProgress, SyncSnapshot, settings::ClientSettings,
};
use gpui::{AppContext, Context, Entity, FocusHandle};
use tokio::sync::{mpsc, watch};

use super::{input::Input, runtime::Request};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Tab {
    Home,
    Watch,
    Logs,
    Settings,
}

impl Tab {
    pub(super) const ALL: [Self; 4] = [Self::Home, Self::Watch, Self::Logs, Self::Settings];
    pub(super) fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Watch => "Watch",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
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
    pub quitting: bool,
    pub diagnostic: String,
    pub watch_message: String,
    pub settings_message: String,
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
            quitting: false,
            diagnostic: String::new(),
            watch_message: String::new(),
            settings_message: String::new(),
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
        let pc_name = cx.new(|cx| Input::new("PC display name", false, cx));
        pc_name.update(cx, |input, cx| {
            input.set_value(
                std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into()),
                cx,
            )
        });
        Self {
            tab: Tab::Settings,
            state: UiState::default(),
            server: cx.new(|cx| Input::new("Server URL", false, cx)),
            token: cx.new(|cx| Input::new("Registration token", true, cx)),
            pc_name,
            focus: cx.focus_handle().tab_stop(true),
            tab_focus: std::array::from_fn(|_| cx.focus_handle()),
            requests,
            control,
            quit,
            opened: false,
        }
    }
}
