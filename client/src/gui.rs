mod actions;
mod feedback;
mod input;
mod model;
mod native;
mod runtime;
mod theme;
mod tray;
mod update;
mod views;

use std::{error::Error, time::Instant};

use drip_client::{RunControl, windows_integration::OpenListener};
use gpui::{
    App, Bounds, Global, KeyBinding, Task, WindowBounds, WindowHandle, WindowOptions, actions,
    prelude::*, px, size,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use model::Shell;

actions!(drip_shell, [NextFocus, PreviousFocus, Quit, Pause]);

pub(super) enum Command {
    Open,
}

struct Lifetime {
    tray: tray::Tray,
    _commands: Task<()>,
    _backend: JoinHandle<()>,
    _ipc: Option<JoinHandle<()>>,
    ipc_shutdown: watch::Sender<bool>,
}

impl Global for Lifetime {}

pub(super) fn run(background: bool, listener: Option<OpenListener>) -> Result<(), Box<dyn Error>> {
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(error) = start(cx, listener, background) {
            eprintln!("Could not open Drip: {error}");
            cx.quit();
        }
    });
    Ok(())
}

fn start(
    cx: &mut App,
    listener: Option<OpenListener>,
    background: bool,
) -> Result<(), Box<dyn Error>> {
    input::init(cx);
    cx.bind_keys([
        KeyBinding::new("tab", NextFocus, None),
        KeyBinding::new("shift-tab", PreviousFocus, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-p", Pause, None),
    ]);
    let (commands, mut command_rx) = mpsc::channel(16);
    let (requests, request_rx) = mpsc::channel(16);
    let (events, mut event_rx) = mpsc::channel(64);
    let (control, mut controls) = watch::channel(RunControl::default());
    let (quit, mut quitting) = watch::channel(false);
    let (ipc_shutdown, ipc_done) = watch::channel(false);
    let shell_requests = requests.clone();
    let shell_control = control.clone();
    let shell_quit = quit.clone();
    let window: WindowHandle<Shell> = cx.open_window(
        WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Drip".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                initial_window_size(
                    cx.primary_display()
                        .map(|display| display.visible_bounds().size),
                ),
                cx,
            ))),
            window_min_size: Some(size(px(720.), px(480.))),
            show: !background,
            ..Default::default()
        },
        |window, cx| {
            window.on_window_should_close(cx, |window, _cx| {
                if let Err(error) = native::hide(window) {
                    eprintln!("Could not hide Drip: {error}");
                }
                false
            });
            cx.new(|cx| Shell::new(shell_requests, shell_control, shell_quit, cx))
        },
    )?;
    if background {
        window.update(cx, |_, window, _cx| native::hide(window))??;
    }
    let tray = tray::Tray::new(commands.clone(), control.clone(), quit.clone())?;
    let backend = runtime::start(request_rx, events.clone(), control.clone(), quit);
    let ipc_events = events.clone();
    let ipc = listener.map(|listener| {
        tokio::spawn(async move {
            if let Err(error) = listener
                .run(move || commands.try_send(Command::Open).is_ok(), ipc_done)
                .await
            {
                let _ = ipc_events
                    .send(runtime::Event::Diagnostic(feedback::diagnostic(format!(
                        "Second-instance Open channel failed: {error}"
                    ))))
                    .await;
            }
        })
    });
    let foreground = cx.spawn(async move |cx| {
        let mut events_open = true;
        loop {
            tokio::select! {
                command = command_rx.recv() => {
                    if command.is_none() { break; }
                    let _ = window.update(cx, |shell, window, cx| shell.open(window, cx));
                }
                event = event_rx.recv(), if events_open => {
                    let Some(event) = event else { events_open = false; continue; };
                    let quit = window.update(cx, |shell, window, cx| shell.update_event(event, window, cx)).unwrap_or(false);
                    if quit {
                        cx.update(|cx| { let _ = cx.global::<Lifetime>().ipc_shutdown.send(true); cx.quit(); });
                        break;
                    }
                }
                changed = controls.changed() => {
                    if changed.is_err() { break; }
                    let _ = window.update(cx, |_, _, cx| cx.notify());
                }
                changed = quitting.changed() => {
                    if changed.is_err() { break; }
                    if *quitting.borrow_and_update() {
                        control.send_modify(|desired| desired.stop = true);
                        let _ = window.update(cx, |shell, _, cx| {
                            shell.state.quitting = true;
                            shell.state.diagnostic = Some(feedback::stopping());
                            cx.notify();
                        });
                        if requests.send(runtime::Request::Quit).await.is_err() { cx.update(|cx| cx.quit()); break; }
                    }
                }
            }
            let state = window.update(cx, |shell, _, _| {
                (shell.footer_label(Instant::now()), shell.control.borrow().paused,
                    shell.state.binding.is_some() && !shell.state.quitting)
            });
            if let Ok((label, paused, enabled)) = state {
                cx.update(|cx| { if let Err(error) = cx.global::<Lifetime>().tray.update(label, paused, enabled) { eprintln!("Could not update Drip tray: {error}"); } });
            }
        }
    });
    cx.set_global(Lifetime {
        tray,
        _commands: foreground,
        _backend: backend,
        _ipc: ipc,
        ipc_shutdown,
    });
    Ok(())
}

fn initial_window_size(available: Option<gpui::Size<gpui::Pixels>>) -> gpui::Size<gpui::Pixels> {
    let preferred = size(px(1000.), px(760.));
    let Some(available) = available else {
        return preferred;
    };
    let fit = size(
        (available.width - px(40.)).max(px(720.)),
        (available.height - px(40.)).max(px(480.)),
    );
    preferred.min(&fit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_size_fits_work_area_below_preferred_height() {
        assert_eq!(
            initial_window_size(Some(size(px(1280.), px(672.)))),
            size(px(1000.), px(632.))
        );
    }

    #[test]
    fn initial_size_keeps_preferred_size_when_work_area_fits() {
        assert_eq!(
            initial_window_size(Some(size(px(1280.), px(900.)))),
            size(px(1000.), px(760.))
        );
    }
}
