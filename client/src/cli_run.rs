use std::{error::Error, io::BufRead};

use drip_client::{Engine, EventSink, RunControl};
use tokio::sync::{mpsc, watch};

pub async fn run() -> Result<(), Box<dyn Error>> {
    let engine = Engine::beside_executable()?;
    let (commands, control) = watch::channel(RunControl::default());
    let (sender, mut receiver) = mpsc::channel(64);
    let (progress, mut snapshots) = watch::channel::<Option<drip_client::SelectionProgress>>(None);
    let reporting = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match serde_json::to_string(&event) {
                Ok(line) => println!("{line}"),
                Err(error) => eprintln!("activity output failed: {error}"),
            }
        }
    });
    let progress_reporting = tokio::spawn(async move {
        while snapshots.changed().await.is_ok() {
            if let Some(progress) = &*snapshots.borrow_and_update() {
                eprintln!(
                    "{:?}: {}: {} of {} bytes",
                    progress.file.phase,
                    progress.file.source_path.to_windows_path(),
                    progress.file.processed_bytes,
                    progress.file.total_bytes
                );
            }
        }
    });
    let interrupt = commands.clone();
    let interrupt_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.send_modify(|control| control.stop = true);
        }
    });
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            match line.trim() {
                "pause" => commands.send_modify(|control| control.paused = true),
                "resume" => commands.send_modify(|control| control.paused = false),
                "rescan" => commands
                    .send_modify(|control| control.reconcile = control.reconcile.wrapping_add(1)),
                "quit" => {
                    commands.send_modify(|control| control.stop = true);
                    break;
                }
                "" => {}
                _ => eprintln!("Commands: pause, resume, rescan, quit"),
            }
        }
    });
    eprintln!("Commands: pause, resume, rescan, quit. Ctrl+C waits for the selection barrier.");
    let result = engine.run(control, EventSink::new(sender, progress)).await;
    interrupt_task.abort();
    reporting.await?;
    progress_reporting.await?;
    result?;
    Ok(())
}
