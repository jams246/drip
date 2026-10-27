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
#[derive(Parser)]
#[command(name = "drip", about = "Secure one-way file upload to a Drip server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
}

#[cfg(windows)]
#[tokio::main]
async fn main() {
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
    match Cli::parse().command {
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
                UploadPhase::Uploading => "Uploading",
            };
            let stage_changed = previous != Some(snapshot.phase);
            if stage_changed || snapshot.processed_bytes % (1024 * 1024) == 0 {
                eprintln!(
                    "{name}: {} of {} bytes.",
                    snapshot.processed_bytes, snapshot.total_bytes
                );
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
