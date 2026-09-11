
use std::io::Write;
use std::sync::Arc;

use crate::browser::BrowserAutomationAction;
use crate::ipc::browser_cli::{
    send_browser_cli_request, BrowserCliRequest, BrowserCliResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Gui,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserCliCommand {
    List,
    Snapshot {
        browser_id: String,
    },
    Click {
        browser_id: String,
        generation: u64,
        reference: String,
    },
    Fill {
        browser_id: String,
        generation: u64,
        reference: String,
        value: String,
    },
    Keypress {
        browser_id: String,
        generation: u64,
        key: String,
    },
}

fn required_option(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|arg| arg == name)
        .ok_or_else(|| format!("missing required option {name}"))?;
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

pub fn parse_browser_cli<I, T>(args: I) -> Result<BrowserCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "browser") {
        return Err("expected `ferryx browser <list|snapshot|click|fill|keypress>`".into());
    }
    match args.get(2).map(String::as_str) {
        Some("list") => Ok(BrowserCliCommand::List),
        Some("snapshot") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Snapshot { browser_id })
        }
        Some("click") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Click {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                reference: required_option(&args, "--ref")?,
            })
        }
        Some("fill") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Fill {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                reference: required_option(&args, "--ref")?,
                value: required_option(&args, "--value")?,
            })
        }
        Some("keypress") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Keypress {
                browser_id,
                generation: required_option(&args, "--generation")?
                    .parse()
                    .map_err(|_| "--generation must be an unsigned integer")?,
                key: required_option(&args, "--key")?,
            })
        }
        _ => Err("expected `ferryx browser <list|snapshot|click|fill|keypress>`".into()),
    }
}

fn browser_cli_request(command: BrowserCliCommand) -> BrowserCliRequest {
    match command {
        BrowserCliCommand::List => BrowserCliRequest::List,
        BrowserCliCommand::Snapshot { browser_id } => BrowserCliRequest::Snapshot { browser_id },
        BrowserCliCommand::Click {
            browser_id,
            generation,
            reference,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Click { reference },
            },
        },
        BrowserCliCommand::Fill {
            browser_id,
            generation,
            reference,
            value,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Fill { reference, value },
            },
        },
        BrowserCliCommand::Keypress {
            browser_id,
            generation,
            key,
        } => BrowserCliRequest::Act {
            request: crate::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Keypress { key },
            },
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairCliCommand {
    List,
    GeneratePin,
    Approve { pin: String },
}

const PAIR_USAGE: &str = "expected `ferryx pair <list|generate|--generate-pin|approve <pin>>`";

/// Parses a pair subcommand (`list`, `generate`, `--generate-pin`, `approve <pin>`)
/// starting at `args[subcommand_index]`. Shared by both `ferryx pair <...>` and
/// `ferryx remote pair <...>`.
fn parse_pair_subcommand(
    args: &[String],
    subcommand_index: usize,
) -> Result<PairCliCommand, String> {
    match args.get(subcommand_index).map(String::as_str) {
        Some("list") => Ok(PairCliCommand::List),
        Some("--generate-pin") | Some("generate") => Ok(PairCliCommand::GeneratePin),
        Some("approve") => {
            let pin = args
                .get(subcommand_index + 1)
                .cloned()
                .ok_or_else(|| "missing <pin> for `ferryx pair approve`".to_string())?;
            Ok(PairCliCommand::Approve { pin })
        }
        _ => Err(PAIR_USAGE.into()),
    }
}

pub fn parse_pair_cli<I, T>(args: I) -> Result<PairCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "pair") {
        return Err(PAIR_USAGE.into());
    }
    parse_pair_subcommand(&args, 2)
}

fn remote_auth_manager() -> Result<crate::remote::AuthManager, String> {
    let data_dir = std::env::var_os("FERRYX_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map(|dir| dir.join("remote"))
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("LOCALAPPDATA")
                    .map(|dir| std::path::PathBuf::from(dir).join("Ferryx").join("remote"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE")
                            .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
                    })
            }
            #[cfg(not(windows))]
            {
                std::env::var_os("HOME")
                    .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
            }
        });
    let auth_path = data_dir
        .ok_or_else(|| "Cannot persist pairing state: set FERRYX_DATA_DIR".to_string())?
        .join("remote-auth.json");
    Ok(crate::remote::AuthManager::with_persistence(Some(
        auth_path,
    )))
}

pub fn run_pair_cli(command: PairCliCommand) -> Result<(), String> {
    let manager = remote_auth_manager()?;
    match command {
        PairCliCommand::List => {
            let devices = manager.list_devices();
            if devices.is_empty() {
                println!("No paired devices");
            } else {
                for device in devices {
                    println!("{}\t{}\t{:?}", device.id, device.name, device.permission);
                }
            }
            Ok(())
        }
        PairCliCommand::GeneratePin => {
            // The daemon owns the machine's single relay control connection and pairing
            // coordinator. Ask it first: standing up a second RelayClient here would
            // contend for the same machine identity on the relay, and a PIN minted
            // outside the relay-registered coordinator is not redeemable remotely.
            let daemon_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            // Only defer to a daemon that is ALREADY running. DaemonClient will happily
            // start one on demand, but silently spawning a daemon is not this command's
            // job, and it would also make the message below untrue.
            let daemon_socket = crate::daemon::server::get_socket_path();
            if daemon_socket.exists() {
                // A daemon is present, so it owns this machine's relay identity. Its
                // answer is authoritative: an explicit refusal must surface as an error
                // rather than silently starting a competing relay owner, which would
                // replace the daemon's control generation and invalidate its live PIN.
                let answer = daemon_runtime.block_on(async {
                    let client = crate::daemon::client::DaemonClient::new();
                    client
                        .remote_create_pairing_code(Some(
                            crate::remote::auth::DevicePermission::Control,
                        ))
                        .await
                });
                match answer {
                    Ok(code) => {
                        println!("{code}");
                        std::io::stdout().flush().map_err(|error| error.to_string())?;
                        eprintln!(
                            "Pairing registered by the running daemon; it holds the relay control connection."
                        );
                        return Ok(());
                    }
                    Err(error) => {
                        return Err(format!(
                            "The running daemon refused this pairing request: {}. \
                             It owns this machine's relay identity, so pairing standalone \
                             would replace its control connection and invalidate any PIN it \
                             already issued.",
                            error.message
                        ));
                    }
                }
            }
            eprintln!(
                "No running daemon answered; pairing standalone from this process instead."
            );
            let directory =
                remote_state_dir().ok_or("Cannot persist machine identity: set FERRYX_DATA_DIR")?;
            let config_path = directory.join("remote-config.json");
            let config: crate::remote::RemoteGatewayConfig = match std::fs::read(&config_path)
            {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
                Err(error) => return Err(error.to_string()),
            };
            let relay_url = match std::env::var("FERRYX_RELAY_URL") {
                Ok(val) if val.trim().is_empty() => {
                    return Err("Pairing requires a configured relay URL (FERRYX_RELAY_URL)".to_string());
                }
                Ok(val) => val,
                Err(_) => config
                    .relay_url
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| crate::remote::state::DEFAULT_RELAY_URL.to_string()),
            };
            let identity = crate::remote::auth::load_or_generate_machine_identity(&directory)?;
            let client = crate::remote::relay_client::RelayClient::with_identity(
                &relay_url,
                identity,
                format!("127.0.0.1:{}", config.port),
            );
            let coordinator = client.pairing_coordinator();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(async move {
                let relay_task = tokio::spawn(async move { client.run().await });
                let result = coordinator.generate_pairing(std::time::Duration::from_secs(60)).await;
                match result {
                    Ok(session) => {
                        println!("{}", session.pin);
                        println!("{}#pair={}", relay_url.trim_end_matches('/'), session.pairing_token);
                        std::io::stdout().flush().map_err(|error| error.to_string())?;
                        eprintln!("Pairing registered; keep this command running until pairing completes or expires.");
                        // This process owns the authenticated control connection; do not drop
                        // it immediately after printing the relay-acknowledged credentials.
                        let deadline = std::time::UNIX_EPOCH + std::time::Duration::from_secs(session.expires_at);
                        tokio::time::sleep(deadline.duration_since(std::time::SystemTime::now()).unwrap_or_default()).await;
                        relay_task.abort();
                        Ok(())
                    }
                    Err(error) => { relay_task.abort(); Err(error) }
                }
            })
        }
        PairCliCommand::Approve { pin } => match manager.approve_pairing_code_cli(&pin) {
            Ok(_device) => {
                println!("Pairing approved for {pin}; ready for remote client exchange");
                Ok(())
            }
            Err(error) => Err(format!("Failed to approve pairing: {error}")),
        },
    }
}

pub fn print_browser_cli_error(code: &str, message: impl AsRef<str>) {
    eprintln!(
        "{}",
        serde_json::json!({ "type": "error", "code": code, "message": message.as_ref() })
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCliCommand {
    Status { json: bool },
    Pair(PairCliCommand),
}

pub fn parse_remote_cli<I, T>(args: I) -> Result<RemoteCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).is_none_or(|arg| arg != "remote") {
        return Err("expected `ferryx remote <status|pair>`".into());
    }
    match args.get(2).map(String::as_str) {
        Some("status") => {
            let json = args.iter().skip(3).any(|arg| arg == "--json");
            Ok(RemoteCliCommand::Status { json })
        }
        Some("pair") => parse_pair_subcommand(&args, 3).map(RemoteCliCommand::Pair),
        _ => Err("expected `ferryx remote <status|pair>`".into()),
    }
}

/// Resolves the base directory Ferryx stores remote gateway state under,
/// mirroring the resolution used by [`remote_auth_manager`].
fn remote_state_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("FERRYX_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map(|dir| dir.join("remote"))
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("LOCALAPPDATA")
                    .map(|dir| std::path::PathBuf::from(dir).join("Ferryx").join("remote"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE")
                            .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
                    })
            }
            #[cfg(not(windows))]
            {
                std::env::var_os("HOME")
                    .map(|dir| std::path::PathBuf::from(dir).join(".ferryx").join("remote"))
            }
        })
}

#[derive(Debug, serde::Serialize)]
struct RemoteStatusOutput {
    status: &'static str,
    port: u16,
    mode: String,
}

/// Reads the persisted remote gateway config (`remote-config.json`) if one
/// exists, otherwise falls back to the default config, and reports the
/// configured port and mode. This does not require the daemon to be running.
fn remote_status_output() -> RemoteStatusOutput {
    let persisted = remote_state_dir()
        .map(|dir| dir.join("remote-config.json"))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());

    let default_config = crate::remote::RemoteGatewayConfig::default();
    let mode = persisted
        .as_ref()
        .and_then(|value| value.get("mode"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            serde_json::to_value(default_config.mode)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "off".to_string())
        });
    let port = persisted
        .as_ref()
        .and_then(|value| value.get("port"))
        .and_then(|value| value.as_u64())
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(default_config.port);

    RemoteStatusOutput {
        status: "ok",
        port,
        mode,
    }
}

pub fn run_remote_cli(command: RemoteCliCommand) -> Result<(), String> {
    match command {
        RemoteCliCommand::Status { json } => {
            let output = remote_status_output();
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&output).map_err(|error| error.to_string())?
                );
            } else {
                println!(
                    "status={} port={} mode={}",
                    output.status, output.port, output.mode
                );
            }
            Ok(())
        }
        RemoteCliCommand::Pair(pair_command) => run_pair_cli(pair_command),
    }
}

pub fn run_browser_cli(command: BrowserCliCommand) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .map_err(|error| error.to_string())?;
    let response = runtime
        .block_on(send_browser_cli_request(browser_cli_request(command)))
        .map_err(|error| error.to_string())?;
    match response {
        BrowserCliResponse::List { sessions } => {
            println!(
                "{}",
                serde_json::to_string(&sessions).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        BrowserCliResponse::Snapshot { snapshot } => {
            println!(
                "{}",
                serde_json::to_string(&snapshot).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        BrowserCliResponse::Acted => {
            println!("{}", serde_json::json!({ "type": "acted" }));
            Ok(())
        }
        BrowserCliResponse::Error { code, message } => {
            print_browser_cli_error(&code, &message);
            Err(message)
        }
    }
}

pub fn parse_launch_mode<I, T>(args: I) -> LaunchMode
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    for arg in args {
        if arg.as_ref() == "--daemon" {
            return LaunchMode::Daemon;
        }
    }
    LaunchMode::Gui
}

pub fn parse_handover_from<I, T>(args: I) -> Option<std::path::PathBuf>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let pos = args.iter().position(|a| a == "--handover-from")?;
    args.get(pos + 1).map(std::path::PathBuf::from)
}

pub fn run_daemon_headless(
    handover_from: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let announce_readiness = handover_from.is_none();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = Arc::new(crate::daemon::server::DaemonServer::new());
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone
                .run_server_with_handover_and_readiness(handover_from, Some(ready_tx))
                .await
        });

        // Wait for server to bind listener and initialize before emitting readiness signal
        match ready_rx.await {
            Ok(()) => {
                if announce_readiness {
                    println!("FERRYX_DAEMON_READY");
                    let _ = std::io::stdout().flush();
                }
            }
            Err(_) => {
                // If ready_tx dropped, server_task must have returned an error
            }
        }

        match server_task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_list_cli_parses_without_browser_id() {
        let args = vec!["ferryx", "browser", "list"];
        assert_eq!(
            parse_browser_cli(&args).expect("parse browser list"),
            BrowserCliCommand::List
        );
    }

    #[test]
    fn test_parse_launch_mode_default_is_gui() {
        let args = vec!["ferryx".to_string()];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Gui);
    }

    #[test]
    fn test_parse_launch_mode_detects_daemon_flag() {
        let args = vec!["ferryx".to_string(), "--daemon".to_string()];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Daemon);
    }

    #[test]
    fn test_parse_launch_mode_ignores_other_flags() {
        let args = vec![
            "ferryx".to_string(),
            "--verbose".to_string(),
            "--other".to_string(),
        ];
        assert_eq!(parse_launch_mode(&args), LaunchMode::Gui);
    }

    #[test]
    fn browser_snapshot_cli_requires_browser_id() {
        let args = vec!["ferryx", "browser", "snapshot"];
        assert!(parse_browser_cli(&args).is_err());
    }

    #[test]
    fn browser_click_cli_preserves_generation_and_reference() {
        let args = vec![
            "ferryx",
            "browser",
            "click",
            "--browser-id",
            "browser-1",
            "--generation",
            "7",
            "--ref",
            "e3",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser click"),
            BrowserCliCommand::Click {
                browser_id: "browser-1".into(),
                generation: 7,
                reference: "e3".into(),
            }
        );
    }

    #[test]
    fn browser_fill_cli_preserves_generation_reference_and_value() {
        let args = vec![
            "ferryx",
            "browser",
            "fill",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--ref",
            "e2",
            "--value",
            "hello world",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser fill"),
            BrowserCliCommand::Fill {
                browser_id: "browser-1".into(),
                generation: 3,
                reference: "e2".into(),
                value: "hello world".into(),
            }
        );
    }

    #[test]
    fn browser_keypress_cli_preserves_generation_and_key() {
        let args = vec![
            "ferryx",
            "browser",
            "keypress",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--key",
            "Enter",
        ];

        assert_eq!(
            parse_browser_cli(&args).expect("parse browser keypress"),
            BrowserCliCommand::Keypress {
                browser_id: "browser-1".into(),
                generation: 3,
                key: "Enter".into(),
            }
        );

        let modifier_args = vec![
            "ferryx",
            "browser",
            "keypress",
            "--browser-id",
            "browser-1",
            "--generation",
            "3",
            "--key",
            "Meta+ArrowLeft",
        ];

        assert_eq!(
            parse_browser_cli(&modifier_args).expect("parse browser keypress with modifiers"),
            BrowserCliCommand::Keypress {
                browser_id: "browser-1".into(),
                generation: 3,
                key: "Meta+ArrowLeft".into(),
            }
        );
    }
}
