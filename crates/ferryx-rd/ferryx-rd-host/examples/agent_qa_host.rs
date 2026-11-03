//! Finite real-host fixture with explicit ports and a separate pairing store.
//! Run only on a physical desktop; this uses the production capture/encode path.
use std::{net::SocketAddr, path::PathBuf, sync::mpsc, thread};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ferryx_rd_host::{ConsentPrompt, HostConfig, HostServer, PairingStore};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    tcp: SocketAddr,
    #[arg(long)]
    udp: SocketAddr,
    #[arg(long)]
    pairing_store: PathBuf,
    #[arg(long)]
    pin: String,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    connections: u32,
    #[arg(long)]
    output: Option<String>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let store = PairingStore::new(cli.pairing_store);
    #[cfg(target_os = "linux")]
    let mut config = HostConfig::linux_default(Some(cli.pin), store, cli.output)?;
    #[cfg(target_os = "windows")]
    let mut config = HostConfig::windows_default(Some(cli.pin), store)?;
    #[cfg(target_os = "macos")]
    let mut config = HostConfig::macos_default(Some(cli.pin), store)?;
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    compile_error!("agent_qa_host requires a native desktop host backend");

    config.tcp_addr = cli.tcp;
    config.udp_addr = cli.udp;
    config.host_name = "MahoRD isolated QA".into();
    let (sender, receiver) = mpsc::channel::<ConsentPrompt>();
    config.consent_sender = Some(sender);
    let server = HostServer::bind(config)?;
    let consent = thread::Builder::new()
        .name("maho-qa-consent".into())
        .spawn(move || {
            while let Ok(prompt) = receiver.recv() {
                prompt.respond(true);
            }
        })
        .context("start isolated QA consent worker")?;
    println!(
        "QA_HOST_READY tcp={} udp={}",
        server.tcp_addr()?,
        server.udp_addr()?
    );
    let result = server.serve_n(usize::try_from(cli.connections)?);
    drop(server);
    let consent_result = consent
        .join()
        .map_err(|_| anyhow!("isolated QA consent worker panicked"));
    result.context("isolated real-host session")?;
    consent_result?;
    println!("QA_HOST_COMPLETE");
    Ok(())
}
