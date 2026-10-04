use crate::watch_registration::Registration;
use crate::watch_win32::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

enum Command {
    Start {
        id: usize,
        path: Vec<u16>,
        recursive: bool,
        reply: mpsc::SyncSender<Result<usize, u32>>,
    },
    Stop {
        id: usize,
        reply: mpsc::SyncSender<()>,
    },
    Shutdown {
        reply: mpsc::SyncSender<()>,
    },
}

struct Engine {
    port: isize,
    commands: mpsc::Sender<Command>,
    thread: JoinHandle<()>,
}

impl Engine {
    fn new() -> Result<Self, u32> {
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE, 0, 0, 1) };
        if port == 0 {
            return Err(unsafe { GetLastError() });
        }
        let (commands, receiver) = mpsc::channel();
        let thread = match thread::Builder::new()
            .name("drip-watch".into())
            .spawn(move || run(port, receiver))
        {
            Ok(thread) => thread,
            Err(_) => {
                close(port);
                return Err(8);
            }
        };
        Ok(Self {
            port,
            commands,
            thread,
        })
    }

    fn send(&self, command: Command) -> bool {
        if self.commands.send(command).is_err() {
            return false;
        }
        unsafe { PostQueuedCompletionStatus(self.port, 0, 0, std::ptr::null_mut()) != 0 }
    }
}

static ENGINE: OnceLock<Mutex<Option<Engine>>> = OnceLock::new();
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn engine() -> &'static Mutex<Option<Engine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

pub fn start_wide(path: Vec<u16>, recursive: bool) -> Result<usize, u32> {
    let mut engine = engine().lock().unwrap();
    if engine.is_none() {
        *engine = Some(Engine::new()?);
    }
    let (reply, result) = mpsc::sync_channel(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let command = Command::Start {
        id,
        path,
        recursive,
        reply,
    };
    if !engine.as_ref().unwrap().send(command) {
        return Err(6);
    }
    result.recv().unwrap_or(Err(6))
}

pub fn stop(id: usize) {
    let engine = engine().lock().unwrap();
    let Some(engine) = engine.as_ref() else {
        return;
    };
    let (reply, result) = mpsc::sync_channel(1);
    if engine.send(Command::Stop { id, reply }) {
        let _ = result.recv();
    }
}

pub fn shutdown() {
    let mut engine = engine().lock().unwrap();
    let Some(current) = engine.take() else {
        return;
    };
    let (reply, result) = mpsc::sync_channel(1);
    if current.send(Command::Shutdown { reply }) {
        let _ = result.recv();
    }
    let _ = current.thread.join();
}

fn run(port: isize, commands: mpsc::Receiver<Command>) {
    let mut registrations = HashMap::<usize, Box<Registration>>::new();
    let mut shutdown = None;
    loop {
        for command in commands.try_iter() {
            match command {
                Command::Start {
                    id,
                    path,
                    recursive,
                    reply,
                } => {
                    let result =
                        Registration::new(id, &path, recursive, port).map(|registration| {
                            registrations.insert(id, registration);
                            id
                        });
                    let _ = reply.send(result);
                }
                Command::Stop { id, reply } => {
                    if let Some(registration) = registrations.get_mut(&id) {
                        registration.stopped.push(reply);
                        registration.cancel();
                    } else {
                        let _ = reply.send(());
                    }
                }
                Command::Shutdown { reply } => {
                    shutdown = Some(reply);
                    for registration in registrations.values_mut() {
                        registration.cancel();
                    }
                }
            }
        }
        if shutdown.is_some() && registrations.is_empty() {
            let _ = shutdown.take().unwrap().send(());
            break;
        }
        let mut bytes = 0;
        let mut key = 0;
        let mut operation = std::ptr::null_mut();
        let success = unsafe {
            GetQueuedCompletionStatus(port, &mut bytes, &mut key, &mut operation, u32::MAX)
        };
        if operation.is_null() {
            continue;
        }
        let error = if success == 0 {
            unsafe { GetLastError() }
        } else {
            0
        };
        complete(&mut registrations, key, bytes, error);
    }
    close(port);
}

fn complete(
    registrations: &mut HashMap<usize, Box<Registration>>,
    id: usize,
    bytes: u32,
    error: u32,
) {
    let Some(mut registration) = registrations.remove(&id) else {
        return;
    };
    if error == 0 {
        registration.decode(bytes);
    } else if error != ABORTED || !registration.stopping {
        registration.record(0, Vec::new(), error, true);
    }
    if registration.stopping || (error != 0 && error != ENUM_DIR) {
        return;
    }
    if let Err(error) = registration.arm() {
        registration.record(0, Vec::new(), error, true);
        return;
    }
    registrations.insert(id, registration);
}
