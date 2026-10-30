use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

use drip_server::{Store, serve};

#[derive(Parser)]
#[command(name = "server", about = "Drip private mirror server")]
struct Arguments {
    #[arg(long, global = true, default_value = "/var/lib/drip")]
    state_dir: PathBuf,
    #[arg(long, global = true, default_value = "/mirror")]
    mirror_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value = "0.0.0.0:8080")]
        listen: SocketAddr,
    },
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
    Devices {
        #[command(subcommand)]
        command: DevicesCommand,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    Create,
}

#[derive(Subcommand)]
enum DevicesCommand {
    List,
    Revoke { pc_id: String },
}

pub async fn run() -> Result<()> {
    let arguments = Arguments::parse();
    let store = if matches!(arguments.command, Command::Serve { .. }) {
        Store::open_for_daemon(&arguments.state_dir, &arguments.mirror_dir)
    } else {
        Store::open(&arguments.state_dir, &arguments.mirror_dir)
    }
    .context("open server state and mirror directories")?;
    match arguments.command {
        Command::Serve { listen } => {
            let listener = TcpListener::bind(listen)
                .await
                .context("bind private listener")?;
            serve(listener, store)
                .await
                .context("serve private listener")?;
        }
        Command::Token {
            command: TokenCommand::Create,
        } => {
            println!(
                "{}",
                store
                    .create_enrollment_token()
                    .context("create enrollment token")?
            );
        }
        Command::Devices {
            command: DevicesCommand::List,
        } => {
            for device in store.list_devices().context("list registered devices")? {
                let status = if device.active { "active" } else { "revoked" };
                println!("{}\t{}\t{}", device.pc_id, device.label, status);
            }
        }
        Command::Devices {
            command: DevicesCommand::Revoke { pc_id },
        } => {
            store.revoke_device(&pc_id).context("revoke device")?;
        }
    }
    Ok(())
}
