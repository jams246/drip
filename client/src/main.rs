#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
use std::{error::Error, path::PathBuf};

#[cfg(windows)]
use clap::{Parser, Subcommand};
#[cfg(windows)]
use drip_client::transport::ServerUrl;
#[cfg(windows)]
use drip_client::{ActivityEvent, Engine, UploadPhase};
#[cfg(windows)]
use tokio::sync::mpsc;

#[cfg(windows)]
mod cli_run;
#[cfg(windows)]
mod console;
#[cfg(windows)]
mod gui;

#[cfg(windows)]
#[derive(Parser)]
#[command(
    name = "drip",
    about = "Continuously mirror selected local paths to a Drip server"
)]
struct Cli {
    #[arg(long)]
    background: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[cfg(windows)]
#[derive(Subcommand)]
enum Command {
    Pair {
        #[arg(long)]
        server: String,
        #[arg(long)]
        label: Option<String>,
    },
    Upload {
        path: PathBuf,
    },
    Add {
        path: PathBuf,
    },
    Remove {
        path: PathBuf,
    },
    Status,
    Run,
}

#[cfg(windows)]
#[tokio::main]
async fn main() {
    console::prepare();
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("drip client supports Windows only");
    std::process::exit(1);
}

#[cfg(windows)]
async fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or("Executable has no data directory")?;
    let mut instance = if !matches!(cli.command, Some(Command::Status)) {
        use drip_client::windows_integration::{InstanceAcquire, InstanceMode, acquire};
        let mode = if cli.command.is_none() {
            InstanceMode::Gui
        } else {
            InstanceMode::Diagnostic
        };
        match acquire(directory, mode).await? {
            InstanceAcquire::Acquired(guard) => Some(guard),
            InstanceAcquire::AlreadyRunning if cli.command.is_none() => return Ok(()),
            InstanceAcquire::AlreadyRunning => return Err("Another Drip engine already owns this app directory. Use the running app or quit it before starting a diagnostic engine.".into()),
        }
    } else {
        None
    };
    let Some(command) = cli.command else {
        return gui::run(
            cli.background,
            instance
                .as_mut()
                .and_then(|instance| instance.take_open_listener()),
        );
    };
    match command {
        Command::Pair { server, label } => {
            let server = ServerUrl::parse(&server)?;
            let token = rpassword::prompt_password("Enrollment token: ")?;
            let label = label.unwrap_or_else(|| {
                std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_owned())
            });
            let pairing = Engine::beside_executable()?
                .pair(server.as_str(), &label, &token)
                .await?;
            println!("Paired PC {}.", pairing.pc_id());
        }
        Command::Upload { path } => {
            if !path.is_absolute() {
                return Err("upload path must be absolute".into());
            }
            upload(path).await?;
        }
        Command::Add { path } => {
            let result = Engine::beside_executable()?.add(&path)?;
            println!(
                "Selection saved: {}",
                result.selection.source.to_windows_path()
            );
            for retained in result.authority_over {
                println!(
                    "Ancestor selection now controls retained {} including confirmed deletions.",
                    retained.to_windows_path()
                );
            }
            println!("Run `drip run` to reconcile saved selections.");
        }
        Command::Remove { path } => {
            Engine::beside_executable()?.remove(&path).await?;
            println!("Selection removed. Existing mirror retained.");
        }
        Command::Status => {
            let engine = Engine::beside_executable()?;
            let settings = engine.status()?;
            let snapshot = engine.snapshot()?;
            println!("Last connection state: {:?}", snapshot.connection);
            println!(
                "Acknowledged generation: {}; update pending: {}",
                settings.acknowledged_generation,
                settings.pending_update.is_some()
            );
            for selection in settings.selections {
                let state = snapshot
                    .roots
                    .iter()
                    .find(|root| root.selection_id == selection.id);
                println!(
                    "{}: {}: {}",
                    selection.source.to_windows_path(),
                    if selection.active {
                        "Active"
                    } else {
                        "Retained"
                    },
                    state.map_or_else(
                        || "Pending reconciliation".to_owned(),
                        |root| format!("{:?}: {}", root.state, root.message)
                    )
                );
            }
        }
        Command::Run => cli_run::run().await?,
    }
    Ok(())
}

#[cfg(windows)]
async fn upload(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let engine = Engine::beside_executable()?;
    let (sender, receiver) = mpsc::channel(16);
    let progress = tokio::spawn(report_progress(receiver));
    let result = engine.upload(&path, &sender).await;
    drop(sender);
    progress.await?;
    let result = result?;
    println!(
        "Uploaded {} bytes, BLAKE3 {}.",
        result.size,
        result.digest.to_hex()
    );
    Ok(())
}

#[cfg(windows)]
async fn report_progress(mut events: mpsc::Receiver<ActivityEvent>) {
    let mut previous = None;
    while let Some(event) = events.recv().await {
        if let ActivityEvent::Progress(snapshot) = event {
            let name = match snapshot.phase {
                UploadPhase::Hashing => "Hashing",
                UploadPhase::Chunking => "Chunking / hashing",
                UploadPhase::Uploading => "Uploading",
                UploadPhase::Verifying => "Verifying",
            };
            let stage_changed = previous != Some(snapshot.phase);
            if stage_changed || snapshot.processed_bytes % (1024 * 1024) == 0 {
                let total = snapshot
                    .total_bytes
                    .map_or_else(|| "unknown".to_owned(), |total| total.to_string());
                eprintln!("{name}: {} of {total} bytes.", snapshot.processed_bytes);
            }
            previous = Some(snapshot.phase);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use clap::Parser;

    use super::Cli;

    #[test]
    fn pairing_token_cannot_be_provided_as_a_command_argument() {
        assert!(
            Cli::try_parse_from([
                "drip",
                "pair",
                "--server",
                "https://server.example",
                "--token",
                "sensitive-value",
            ])
            .is_err()
        );
    }
}
