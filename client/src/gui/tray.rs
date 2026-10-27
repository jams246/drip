use std::error::Error;

use drip_client::RunControl;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

use super::Command;

pub(super) struct Tray {
    icon: TrayIcon,
    pause: MenuItem,
}

impl Tray {
    pub(super) fn new(
        sender: mpsc::Sender<Command>,
        control: watch::Sender<RunControl>,
        quit_signal: watch::Sender<bool>,
    ) -> Result<Self, Box<dyn Error>> {
        let menu = Menu::new();
        let open = MenuItem::new("Open", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let pause = MenuItem::new("Pause", false, None);
        menu.append_items(&[&open, &pause, &quit])?;
        let open_id = open.id().clone();
        let quit_id = quit.id().clone();
        let pause_id = pause.id().clone();
        let click_sender = sender.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = click_sender.try_send(Command::Open);
            }
        }));
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == open_id {
                let _ = sender.try_send(Command::Open);
            } else if event.id == pause_id {
                control.send_modify(|desired| desired.paused = !desired.paused);
            } else if event.id == quit_id {
                control.send_modify(|desired| desired.stop = true);
                let _ = quit_signal.send(true);
            }
        }));
        let icon = Icon::from_rgba([0x47, 0xa9, 0xff, 0xff].repeat(16 * 16), 16, 16)?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("DRIP - Disconnected")
            .with_menu_on_left_click(false)
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()?;
        Ok(Self { icon, pause })
    }

    pub(super) fn update(
        &self,
        status: &str,
        paused: bool,
        enabled: bool,
    ) -> Result<(), Box<dyn Error>> {
        self.pause.set_text(if paused { "Resume" } else { "Pause" });
        self.pause.set_enabled(enabled);
        self.icon.set_tooltip(Some(format!("DRIP - {status}")))?;
        Ok(())
    }
}
