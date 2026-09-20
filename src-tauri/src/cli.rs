use std::io::Write;
use std::sync::Arc;

use crate::browser::BrowserAutomationAction;
use crate::ipc::browser_cli::{send_browser_cli_request, BrowserCliRequest, BrowserCliResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Gui,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserCliCommand {
    List,
    Open {
        url: String,
        workspace_id: Option<String>,
        worktree_path: Option<String>,
    },
    Navigate {
        browser_id: String,
        url: String,
    },
    Close {
        browser_id: String,
    },
    Identify,
    Url {
        browser_id: String,
    },
    Title {
        browser_id: String,
    },
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
    Eval {
        browser_id: String,
        script: String,
    },
    Wait {
        browser_id: String,
        condition: BrowserWaitCondition,
        timeout_ms: Option<u64>,
    },
    Screenshot {
        browser_id: String,
        out_path: String,
    },
    Console {
        browser_id: String,
        errors_only: bool,
        clear: bool,
    },
    Errors {
        browser_id: String,
        clear: bool,
    },
    Focus {
        browser_id: String,
    },
    Cookies {
        browser_id: String,
        action: CookieCliAction,
    },
    Storage {
        browser_id: String,
        kind: StorageCliKind,
        action: StorageCliAction,
    },
}

pub use crate::browser::model::BrowserWaitCondition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieCliAction {
    Get,
    Set {
        name: String,
        value: String,
        domain: Option<String>,
        path: Option<String>,
    },
    Clear {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageCliKind {
    Local,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageCliAction {
    Get { key: Option<String> },
    Set { key: String, value: String },
    Clear { key: Option<String> },
}

const BROWSER_USAGE: &str =
    "expected `ferryx browser <list|open|navigate|close|identify|url|title|snapshot|click|fill|keypress|eval|wait|screenshot|console|errors|focus|cookies|storage>`";

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

fn optional_option(args: &[String], name: &str) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .map(Some)
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
        return Err(BROWSER_USAGE.into());
    }
    match args.get(2).map(String::as_str) {
        Some("list") => Ok(BrowserCliCommand::List),
        Some("open") => {
            let url = required_option(&args, "--url")?;
            let workspace_id = optional_option(&args, "--workspace")?.or_else(|| {
                std::env::var("FERRYX_WORKSPACE_ID")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });
            let worktree_path = optional_option(&args, "--worktree-path")?.or_else(|| {
                std::env::var("FERRYX_WORKTREE_PATH")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });
            Ok(BrowserCliCommand::Open {
                url,
                workspace_id,
                worktree_path,
            })
        }
        Some("navigate") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let url = required_option(&args, "--url")?;
            Ok(BrowserCliCommand::Navigate { browser_id, url })
        }
        Some("close") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Close { browser_id })
        }
        Some("identify") => Ok(BrowserCliCommand::Identify),
        Some("url") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Url { browser_id })
        }
        Some("title") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Title { browser_id })
        }
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
        Some("eval") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let script = required_option(&args, "--script")?;
            Ok(BrowserCliCommand::Eval { browser_id, script })
        }
        Some("wait") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let timeout_ms = optional_option(&args, "--timeout-ms")?
                .map(|v| {
                    v.parse::<u64>()
                        .map_err(|_| "--timeout-ms must be an unsigned integer")
                })
                .transpose()?;
            let condition = if let Some(selector) = optional_option(&args, "--selector")? {
                BrowserWaitCondition::Selector { selector }
            } else if let Some(text) = optional_option(&args, "--text")? {
                BrowserWaitCondition::Text { text }
            } else if let Some(fragment) = optional_option(&args, "--url-contains")? {
                BrowserWaitCondition::UrlContains { fragment }
            } else if let Some(state) = optional_option(&args, "--load-state")? {
                BrowserWaitCondition::LoadState { state }
            } else if let Some(script) = optional_option(&args, "--function")? {
                BrowserWaitCondition::Function { script }
            } else {
                return Err("missing wait condition: expected --selector, --text, --url-contains, --load-state, or --function".into());
            };
            Ok(BrowserCliCommand::Wait {
                browser_id,
                condition,
                timeout_ms,
            })
        }
        Some("screenshot") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let out_path = required_option(&args, "--out")?;
            Ok(BrowserCliCommand::Screenshot {
                browser_id,
                out_path,
            })
        }
        Some("console") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let errors_only = args.iter().any(|arg| arg == "--errors");
            let clear = args.iter().any(|arg| arg == "--clear");
            Ok(BrowserCliCommand::Console {
                browser_id,
                errors_only,
                clear,
            })
        }
        Some("errors") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let clear = args.iter().any(|arg| arg == "--clear");
            Ok(BrowserCliCommand::Errors { browser_id, clear })
        }
        Some("focus") => {
            let browser_id = required_option(&args, "--browser-id")?;
            Ok(BrowserCliCommand::Focus { browser_id })
        }
        Some("cookies") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" || arg == "--domain" || arg == "--path" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let action = match positional.first().map(String::as_str) {
                Some("get") => CookieCliAction::Get,
                Some("set") => {
                    let name = positional
                        .get(1)
                        .cloned()
                        .ok_or("missing cookie name for set")?;
                    let value = positional
                        .get(2)
                        .cloned()
                        .ok_or("missing cookie value for set")?;
                    let domain = optional_option(&args, "--domain")?;
                    let path = optional_option(&args, "--path")?;
                    CookieCliAction::Set {
                        name,
                        value,
                        domain,
                        path,
                    }
                }
                Some("clear") => {
                    let name = positional
                        .get(1)
                        .cloned()
                        .ok_or("missing cookie name for clear")?;
                    CookieCliAction::Clear { name }
                }
                _ => return Err("expected cookies <get|set|clear>".into()),
            };
            Ok(BrowserCliCommand::Cookies { browser_id, action })
        }
        Some("storage") => {
            let browser_id = required_option(&args, "--browser-id")?;
            let mut positional = Vec::new();
            let mut i = 3;
            while i < args.len() {
                let arg = &args[i];
                if arg == "--browser-id" {
                    i += 2;
                } else if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional.push(arg.clone());
                    i += 1;
                }
            }
            let kind = match positional.first().map(String::as_str) {
                Some("local") => StorageCliKind::Local,
                Some("session") => StorageCliKind::Session,
                _ => return Err("expected storage kind `local` or `session`".into()),
            };
            let action = match positional.get(1).map(String::as_str) {
                Some("get") => StorageCliAction::Get {
                    key: positional.get(2).cloned(),
                },
                Some("set") => {
                    let key = positional
                        .get(2)
                        .cloned()
                        .ok_or("missing key for storage set")?;
                    let value = positional
                        .get(3)
                        .cloned()
                        .ok_or("missing value for storage set")?;
                    StorageCliAction::Set { key, value }
                }
                Some("clear") => StorageCliAction::Clear {
                    key: positional.get(2).cloned(),
                },
                _ => return Err("expected storage action `get`, `set`, or `clear`".into()),
            };
            Ok(BrowserCliCommand::Storage {
                browser_id,
                kind,
                action,
            })
        }
        _ => Err(BROWSER_USAGE.into()),
    }
}

fn browser_cli_request(command: BrowserCliCommand) -> BrowserCliRequest {
    match command {
        BrowserCliCommand::List => BrowserCliRequest::List,
        BrowserCliCommand::Open {
            url,
            workspace_id,
            worktree_path,
        } => BrowserCliRequest::Open {
            url,
            workspace_id,
            worktree_path,
        },
        BrowserCliCommand::Navigate { browser_id, url } => {
            BrowserCliRequest::Navigate { browser_id, url }
        }
        BrowserCliCommand::Close { browser_id } => BrowserCliRequest::Close { browser_id },
        BrowserCliCommand::Identify => BrowserCliRequest::Identify,
        BrowserCliCommand::Url { browser_id } => BrowserCliRequest::Snapshot { browser_id },
        BrowserCliCommand::Title { browser_id } => BrowserCliRequest::Snapshot { browser_id },
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
        #[cfg(any())]
        BrowserCliCommand::Eval { browser_id, script } => {
            BrowserCliRequest::Eval { browser_id, script }
        }
        #[cfg(any())]
        BrowserCliCommand::Wait {
            browser_id,
            condition,
            timeout_ms: _,
        } => BrowserCliRequest::Wait {
            browser_id,
            condition,
        },
        #[cfg(any())]
        BrowserCliCommand::Screenshot {
            browser_id,
            out_path,
        } => BrowserCliRequest::Screenshot {
            browser_id,
            out_path,
        },
        #[cfg(any())]
        BrowserCliCommand::Console {
            browser_id,
            errors_only,
            clear,
        } => BrowserCliRequest::Console {
            browser_id,
            errors_only: Some(errors_only),
            clear: Some(clear),
        },
        #[cfg(any())]
        BrowserCliCommand::Errors { browser_id, clear } => BrowserCliRequest::Console {
            browser_id,
            errors_only: Some(true),
            clear: Some(clear),
        },
        #[cfg(any())]
        BrowserCliCommand::Focus { browser_id } => BrowserCliRequest::Focus { browser_id },
        #[cfg(any())]
        BrowserCliCommand::Cookies { browser_id, action } => match action {
            CookieCliAction::Get => BrowserCliRequest::Cookies {
                browser_id,
                action: "get".into(),
                name: None,
                value: None,
                domain: None,
                path: None,
            },
            CookieCliAction::Set {
                name,
                value,
                domain,
                path,
            } => BrowserCliRequest::Cookies {
                browser_id,
                action: "set".into(),
                name: Some(name),
                value: Some(value),
                domain,
                path,
            },
            CookieCliAction::Clear { name } => BrowserCliRequest::Cookies {
                browser_id,
                action: "clear".into(),
                name: Some(name),
                value: None,
                domain: None,
                path: None,
            },
        },
        #[cfg(any())]
        BrowserCliCommand::Storage {
            browser_id,
            kind,
            action,
        } => {
            let kind = match kind {
                StorageCliKind::Local => "local",
                StorageCliKind::Session => "session",
            };
            match action {
                StorageCliAction::Get { key } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "get".into(),
                    key,
                    value: None,
                },
                StorageCliAction::Set { key, value } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "set".into(),
                    key: Some(key),
                    value: Some(value),
                },
                StorageCliAction::Clear { key } => BrowserCliRequest::Storage {
                    browser_id,
                    kind: kind.into(),
                    action: "clear".into(),
                    key,
                    value: None,
                },
            }
        }
        _ => unimplemented!("Lane E wire variants converging"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairCliCommand {
    List,
    GeneratePin,
    GenerateMachinePin,
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
        Some("--generate-pin") | Some("generate") => match &args[subcommand_index + 1..] {
            [] => Ok(PairCliCommand::GeneratePin),
            [flag, scope] if flag == "--access" && scope == "mirror" => {
                Ok(PairCliCommand::GeneratePin)
            }
            [flag, scope] if flag == "--access" && scope == "machine" => {
                Ok(PairCliCommand::GenerateMachinePin)
            }
            _ => Err("expected pair generate [--access mirror|machine]".into()),
        },
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
        PairCliCommand::GeneratePin | PairCliCommand::GenerateMachinePin => {
            let machine = command == PairCliCommand::GenerateMachinePin;
            if let Ok(val) = std::env::var("FERRYX_RELAY_URL") {
                if val.trim().is_empty() {
                    return Err(
                        "Pairing requires a configured relay URL (FERRYX_RELAY_URL)".to_string()
                    );
                }
            }

            let daemon_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;

            let answer = daemon_runtime.block_on(async {
                let client = crate::daemon::client::DaemonClient::new();
                if machine {
                    use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
                    match client
                        .send_request(DaemonRequest::RemoteCreateMachinePairingCode)
                        .await?
                    {
                        DaemonResponse::RemotePairingCodeOk {
                            code,
                            pairing_token,
                            machine_id,
                            relay_url,
                        } => Ok((code, pairing_token, machine_id, relay_url)),
                        DaemonResponse::Error { message, .. } => Err(crate::ipc::IpcError::new(
                            crate::ipc::IpcErrorCode::InternalError,
                            message,
                        )),
                        _ => Err(crate::ipc::IpcError::new(
                            crate::ipc::IpcErrorCode::InternalError,
                            "Daemon does not support machine pairing",
                        )),
                    }
                } else {
                    client
                        .remote_create_pairing_code_detailed(Some(
                            crate::remote::auth::DevicePermission::Control,
                        ))
                        .await
                }
            });

            match answer {
                Ok((code, pairing_token, _machine_id, daemon_relay_url)) => {
                    if machine {
                        eprintln!("Access: machine; permits browsing and executing programs as the daemon OS user.");
                    } else {
                        eprintln!("Access: mirror; controls only exposed desktop sessions.");
                    }
                    println!("{code}");
                    if let Some(token) = pairing_token {
                        let relay_url = daemon_relay_url
                            .filter(|s| !s.trim().is_empty())
                            .or_else(|| {
                                std::env::var("FERRYX_RELAY_URL")
                                    .ok()
                                    .filter(|s| !s.trim().is_empty())
                            })
                            .unwrap_or_else(|| crate::remote::state::DEFAULT_RELAY_URL.to_string());
                        println!("{}#pair={token}", relay_url.trim_end_matches('/'));
                    }
                    eprintln!(
                        "Pairing registered by the running daemon; it holds the relay control connection."
                    );
                    eprintln!("Enter this PIN in the desktop's Paired machines settings. Mirror PINs stay valid for one minute; machine PINs for ten minutes.");
                    std::io::stdout()
                        .flush()
                        .map_err(|error| error.to_string())?;
                    Ok(())
                }
                Err(error) => Err(format!(
                    "The running daemon refused this pairing request: {}. \
                     It owns this machine's relay identity, so pairing standalone \
                     would replace its control connection and invalidate any PIN it \
                     already issued.",
                    error.message
                )),
            }
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

fn format_browser_cli_response(
    command: &BrowserCliCommand,
    response: BrowserCliResponse,
) -> Result<String, String> {
    match response {
        BrowserCliResponse::List { sessions } => {
            serde_json::to_string(&sessions).map_err(|error| error.to_string())
        }
        BrowserCliResponse::Snapshot { snapshot } => match command {
            BrowserCliCommand::Url { .. } => Ok(snapshot.url),
            BrowserCliCommand::Title { .. } => Ok(snapshot.title),
            _ => serde_json::to_string(&snapshot).map_err(|error| error.to_string()),
        },
        BrowserCliResponse::Acted => Ok(serde_json::json!({ "type": "acted" }).to_string()),
        BrowserCliResponse::Opened { browser } => {
            serde_json::to_string(&browser).map_err(|error| error.to_string())
        }
        BrowserCliResponse::Navigated => Ok(serde_json::json!({ "type": "navigated" }).to_string()),
        BrowserCliResponse::Closed => Ok(serde_json::json!({ "type": "closed" }).to_string()),
        BrowserCliResponse::Identified { browser } => {
            serde_json::to_string(&browser).map_err(|error| error.to_string())
        }
        #[cfg(any())]
        BrowserCliResponse::Evaluated { result, truncated } => {
            if truncated {
                eprintln!("(truncated)");
            }
            Ok(result.unwrap_or_default())
        }
        #[cfg(any())]
        BrowserCliResponse::Waited => Ok("waited".into()),
        #[cfg(any())]
        BrowserCliResponse::Focused => Ok("focused".into()),
        #[cfg(any())]
        BrowserCliResponse::ScreenshotSaved { path } => Ok(path),
        #[cfg(any())]
        BrowserCliResponse::ConsoleEntries { entries } => {
            serde_json::to_string(&entries).map_err(|error| error.to_string())
        }
        #[cfg(any())]
        BrowserCliResponse::CookieEntries { cookies } => {
            serde_json::to_string(&cookies).map_err(|error| error.to_string())
        }
        #[cfg(any())]
        BrowserCliResponse::StorageValue { value } => {
            serde_json::to_string(&value).map_err(|error| error.to_string())
        }
        BrowserCliResponse::Error { code, message } => {
            print_browser_cli_error(&code, &message);
            Err(message)
        }
        #[allow(unreachable_patterns)]
        _ => Ok(String::new()),
    }
}

pub fn run_browser_cli(command: BrowserCliCommand) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .map_err(|error| error.to_string())?;
    let response = runtime
        .block_on(send_browser_cli_request(browser_cli_request(
            command.clone(),
        )))
        .map_err(|error| error.to_string())?;
    let output = format_browser_cli_response(&command, response)?;
    println!("{output}");
    Ok(())
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

/// Parses `--handover-from <path>`.
///
/// Returns `Ok(None)` when the flag is absent (a legitimate cold start) and an
/// error when the flag is present but its value is missing or is itself an
/// option. Treating a malformed flag as "no handover" would silently cold-start
/// the daemon and abandon every live PTY session the handover existed to carry
/// across, while still looking like a successful start to the caller.
pub fn parse_handover_from<I, T>(args: I) -> Result<Option<std::path::PathBuf>, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let Some(pos) = args.iter().position(|a| a == "--handover-from") else {
        return Ok(None);
    };
    match args.get(pos + 1) {
        Some(value) if !value.starts_with("--") => Ok(Some(std::path::PathBuf::from(value))),
        _ => Err("missing value for --handover-from".to_string()),
    }
}

pub fn run_daemon_headless(
    handover_from: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let announce_readiness = handover_from.is_none();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let logging = match crate::daemon::logging::DaemonLogging::start().await {
            Ok(logging) => Some(logging),
            Err(error) => {
                crate::daemon::logging::report_failure(&error);
                None
            }
        };
        #[cfg(test)]
        if std::env::var("FERRYX_LOGGING_FIXTURE").as_deref() == Ok("1") {
            let hub = crate::daemon::agent_state::AgentStateHub::default();
            hub.release_manual("logging-fixture-session");
            hub.release_foreground("logging-fixture-session");
            if let Some(logging) = logging {
                logging.finish().await?;
            }
            return Ok(());
        }
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        #[cfg(test)]
        let lifecycle_fixture = std::env::var("FERRYX_LOGGING_FIXTURE").is_ok_and(|v| v != "1");
        #[cfg(not(test))]
        let lifecycle_fixture = false;
        let server_task = if lifecycle_fixture {
            tokio::spawn(async move {
                let _ = ready_tx.send(());
                crate::daemon::agent_state::AgentStateHub::default()
                    .release_manual("failure-fixture");
                use tokio::io::AsyncReadExt;
                let mut byte = [0];
                tokio::io::stdin()
                    .read_exact(&mut byte)
                    .await
                    .map_err(|e| e.to_string())?;
                println!("FERRYX_PRIMARY_SERVICE_ALIVE");
                Ok(())
            })
        } else {
            let server = Arc::new(crate::daemon::server::DaemonServer::new());
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone
                    .run_server_with_handover_and_readiness(handover_from, Some(ready_tx))
                    .await
            });

            let registry = server.workspace_registry().clone();
            tokio::task::spawn_blocking(move || {
                if let Ok(initial) = crate::ipc::project::initial_project(&registry) {
                    tracing::info!(
                        workspace_id = %initial.workspace_id,
                        repo_root = %initial.repo_root.display(),
                        "Registered startup workspace for headless daemon"
                    );
                }
            });

            server_task
        };

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

        let completed = server_task.await;
        let result = match completed {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        };
        if let Some(logging) = logging {
            if let Err(error) = logging.finish().await {
                crate::daemon::logging::report_failure(&error);
            }
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_logging_child() {
        if std::env::var_os("FERRYX_LOGGING_FIXTURE").is_some() {
            run_daemon_headless(None).expect("headless initialization and shutdown");
        }
    }

    #[test]
    fn headless_release_reasons_reach_private_bounded_sink() {
        // Given: a separate process with all writable locations inside this worktree.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        let fixture = tempfile::tempdir_in(root).unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "cli::tests::headless_logging_child",
                "--nocapture",
            ])
            .env("FERRYX_LOGGING_FIXTURE", "1")
            .current_dir(fixture.path())
            .kill_on_drop(true);
        for key in [
            "FERRYX_DATA_DIR",
            "FERRYX_RUNTIME_DIR",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "TMPDIR",
            "TMP",
            "TEMP",
        ] {
            child.env(key, fixture.path());
        }
        // When: production headless initialization emits both real release operations and exits.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let output = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(20), child.output())
                .await
                .expect("bounded child exit")
                .unwrap()
        });
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Then: both structured reasons and their session reach the daemon-owned file.
        let path = fixture.path().join("logs/daemon.log");
        let text = std::fs::read_to_string(&path).expect("headless release log must exist");
        for reason in ["manual_reset", "foreground_agent_to_shell"] {
            assert!(
                text.lines().any(
                    |line| line.contains("session_id=\"logging-fixture-session\"")
                        && line.contains(&format!("reason=\"{reason}\""))
                ),
                "missing {reason}: {text}"
            );
        }
        assert!(text.len() <= 1024 * 1024);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("reason="));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("reason="));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn headless_primary_service_survives_logging_failures() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for mode in ["init_failure", "write_failure"] {
            // Given: isolated production CLI lifecycle with a gated primary task.
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
            let fixture = tempfile::tempdir_in(root).unwrap();
            if mode == "init_failure" {
                std::fs::write(fixture.path().join("logs"), b"not a directory").unwrap();
            }
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    let mut command =
                        tokio::process::Command::new(std::env::current_exe().unwrap());
                    command
                        .args([
                            "--exact",
                            "cli::tests::headless_logging_child",
                            "--nocapture",
                        ])
                        .env("FERRYX_LOGGING_FIXTURE", mode)
                        .current_dir(fixture.path())
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true);
                    for key in [
                        "FERRYX_DATA_DIR",
                        "FERRYX_RUNTIME_DIR",
                        "HOME",
                        "USERPROFILE",
                        "LOCALAPPDATA",
                        "APPDATA",
                        "TMPDIR",
                        "TMP",
                        "TEMP",
                    ] {
                        command.env(key, fixture.path());
                    }
                    let mut child = command.spawn().unwrap();
                    let mut errors = BufReader::new(child.stderr.take().unwrap()).lines();
                    // When: the actual CLI reports sink failure, release the primary task.
                    let report = errors.next_line().await.unwrap().unwrap_or_default();
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(b"x").await;
                    }
                    let output = child.wait_with_output().await.unwrap();
                    // Then: readiness and a post-failure service action both survive.
                    assert!(output.status.success(), "{mode}: {report}");
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    assert!(stdout.contains("FERRYX_DAEMON_READY"), "{mode}: {stdout}");
                    assert!(
                        stdout.contains("FERRYX_PRIMARY_SERVICE_ALIVE"),
                        "{mode}: {stdout}"
                    );
                    assert!(
                        report.contains("FERRYX_DAEMON_LOGGING_DISABLED"),
                        "{report}"
                    );
                    assert!(
                        errors.next_line().await.unwrap().is_none(),
                        "only one failure report"
                    );
                })
                .await
                .expect("bounded lifecycle fixture exit");
            });
        }
    }

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

    #[test]
    fn handover_from_rejects_a_missing_or_option_like_value() {
        // `--handover-from` with no value must be an ERROR, not a silent cold start.
        // A handover exists to transplant live PTY sessions from a retiring daemon;
        // starting fresh instead abandons every session the caller meant to preserve,
        // and the caller sees a daemon that looks healthy.
        assert!(
            parse_handover_from(&["--daemon".to_string(), "--handover-from".to_string()]).is_err()
        );
        // A following flag is not a path.
        assert!(
            parse_handover_from(&["--handover-from".to_string(), "--daemon".to_string()]).is_err()
        );
        // Absent entirely is a legitimate cold start.
        assert_eq!(
            parse_handover_from(&["--daemon".to_string()]).expect("absent is ok"),
            None
        );
        // A real value still parses.
        assert_eq!(
            parse_handover_from(&[
                "--handover-from".to_string(),
                "/tmp/ferryx-handover.json".to_string()
            ])
            .expect("value parses"),
            Some(std::path::PathBuf::from("/tmp/ferryx-handover.json"))
        );
    }

    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn browser_open_cli_happy_path() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let prev_ws = std::env::var("FERRYX_WORKSPACE_ID").ok();
        let prev_wt = std::env::var("FERRYX_WORKTREE_PATH").ok();
        std::env::remove_var("FERRYX_WORKSPACE_ID");
        std::env::remove_var("FERRYX_WORKTREE_PATH");

        let args = vec!["ferryx", "browser", "open", "--url", "https://example.com"];
        let parsed = parse_browser_cli(&args);

        let full_args = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--workspace",
            "ws-123",
            "--worktree-path",
            "/path/to/worktree",
        ];
        let parsed_full = parse_browser_cli(&full_args);

        if let Some(ws) = prev_ws {
            std::env::set_var("FERRYX_WORKSPACE_ID", ws);
        }
        if let Some(wt) = prev_wt {
            std::env::set_var("FERRYX_WORKTREE_PATH", wt);
        }

        assert_eq!(
            parsed.expect("parse browser open"),
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: None,
                worktree_path: None,
            }
        );

        assert_eq!(
            parsed_full.expect("parse browser open with optional flags"),
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-123".into()),
                worktree_path: Some("/path/to/worktree".into()),
            }
        );
    }

    #[test]
    fn browser_open_cli_missing_options() {
        let args_no_url = vec!["ferryx", "browser", "open"];
        assert!(parse_browser_cli(&args_no_url).is_err());

        let args_empty_url = vec!["ferryx", "browser", "open", "--url"];
        assert!(parse_browser_cli(&args_empty_url).is_err());

        let args_empty_ws = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--workspace",
        ];
        assert!(parse_browser_cli(&args_empty_ws).is_err());

        let args_empty_wt = vec![
            "ferryx",
            "browser",
            "open",
            "--url",
            "https://example.com",
            "--worktree-path",
        ];
        assert!(parse_browser_cli(&args_empty_wt).is_err());
    }

    #[test]
    fn browser_navigate_cli_happy_and_missing() {
        let happy = vec![
            "ferryx",
            "browser",
            "navigate",
            "--browser-id",
            "b-1",
            "--url",
            "https://example.com",
        ];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser navigate"),
            BrowserCliCommand::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }
        );

        let missing_url = vec!["ferryx", "browser", "navigate", "--browser-id", "b-1"];
        assert!(parse_browser_cli(&missing_url).is_err());

        let missing_id = vec![
            "ferryx",
            "browser",
            "navigate",
            "--url",
            "https://example.com",
        ];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_close_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "close", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser close"),
            BrowserCliCommand::Close {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "close"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_identify_cli_happy() {
        let happy = vec!["ferryx", "browser", "identify"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser identify"),
            BrowserCliCommand::Identify
        );
    }

    #[test]
    fn browser_url_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "url", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser url"),
            BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "url"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_title_cli_happy_and_missing() {
        let happy = vec!["ferryx", "browser", "title", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&happy).expect("parse browser title"),
            BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            }
        );

        let missing_id = vec!["ferryx", "browser", "title"];
        assert!(parse_browser_cli(&missing_id).is_err());
    }

    #[test]
    fn browser_cli_unknown_subcommand_and_usage() {
        let err = parse_browser_cli(&["ferryx", "browser", "unknown"]).unwrap_err();
        assert_eq!(
            err,
            "expected `ferryx browser <list|open|navigate|close|identify|url|title|snapshot|click|fill|keypress|eval|wait|screenshot|console|errors|focus|cookies|storage>`"
        );

        let err_no_browser = parse_browser_cli(&["ferryx", "other"]).unwrap_err();
        assert_eq!(
            err_no_browser,
            "expected `ferryx browser <list|open|navigate|close|identify|url|title|snapshot|click|fill|keypress|eval|wait|screenshot|console|errors|focus|cookies|storage>`"
        );
    }

    #[cfg(any())]
    #[test]
    fn browser_cli_request_mappings_phase3() {
        assert_eq!(
            browser_cli_request(BrowserCliCommand::Eval {
                browser_id: "b-1".into(),
                script: "1+1".into(),
            }),
            BrowserCliRequest::Eval {
                browser_id: "b-1".into(),
                script: "1+1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
                timeout_ms: Some(5000),
            }),
            BrowserCliRequest::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }),
            BrowserCliRequest::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Console {
                browser_id: "b-1".into(),
                errors_only: true,
                clear: true,
            }),
            BrowserCliRequest::Console {
                browser_id: "b-1".into(),
                errors_only: Some(true),
                clear: Some(true),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Errors {
                browser_id: "b-1".into(),
                clear: true,
            }),
            BrowserCliRequest::Console {
                browser_id: "b-1".into(),
                errors_only: Some(true),
                clear: Some(true),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Focus {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Get,
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "get".into(),
                name: None,
                value: None,
                domain: None,
                path: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Set {
                    name: "session".into(),
                    value: "xyz".into(),
                    domain: Some("example.com".into()),
                    path: Some("/".into()),
                },
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "set".into(),
                name: Some("session".into()),
                value: Some("xyz".into()),
                domain: Some("example.com".into()),
                path: Some("/".into()),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Clear {
                    name: "session".into(),
                },
            }),
            BrowserCliRequest::Cookies {
                browser_id: "b-1".into(),
                action: "clear".into(),
                name: Some("session".into()),
                value: None,
                domain: None,
                path: None,
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Get {
                    key: Some("token".into()),
                },
            }),
            BrowserCliRequest::Storage {
                browser_id: "b-1".into(),
                kind: "local".into(),
                action: "get".into(),
                key: Some("token".into()),
                value: None,
            }
        );
    }

    #[test]
    fn browser_cli_request_mappings() {
        assert_eq!(
            browser_cli_request(BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-1".into()),
                worktree_path: Some("/tree".into()),
            }),
            BrowserCliRequest::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-1".into()),
                worktree_path: Some("/tree".into()),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }),
            BrowserCliRequest::Navigate {
                browser_id: "b-1".into(),
                url: "https://example.com".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Close {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Close {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Identify),
            BrowserCliRequest::Identify
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Snapshot {
                browser_id: "b-1".into(),
            }
        );

        assert_eq!(
            browser_cli_request(BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            }),
            BrowserCliRequest::Snapshot {
                browser_id: "b-1".into(),
            }
        );
    }

    #[test]
    fn browser_formatter_prints_only_url_or_title_from_snapshot() {
        let snapshot = crate::browser::BrowserAutomationSnapshot {
            browser_id: "b-1".into(),
            generation: 1,
            url: "https://example.com/page".into(),
            title: "Example Title".into(),
            elements: vec![],
        };
        let response = BrowserCliResponse::Snapshot {
            snapshot: snapshot.clone(),
        };

        let url_output = format_browser_cli_response(
            &BrowserCliCommand::Url {
                browser_id: "b-1".into(),
            },
            response.clone(),
        )
        .expect("format url");
        assert_eq!(url_output, "https://example.com/page");

        let title_output = format_browser_cli_response(
            &BrowserCliCommand::Title {
                browser_id: "b-1".into(),
            },
            response.clone(),
        )
        .expect("format title");
        assert_eq!(title_output, "Example Title");

        let snapshot_output = format_browser_cli_response(
            &BrowserCliCommand::Snapshot {
                browser_id: "b-1".into(),
            },
            response,
        )
        .expect("format snapshot");
        assert_eq!(snapshot_output, serde_json::to_string(&snapshot).unwrap());
    }

    #[test]
    fn browser_eval_cli_parsing() {
        let args = vec![
            "ferryx",
            "browser",
            "eval",
            "--browser-id",
            "b-1",
            "--script",
            "document.title",
        ];
        assert_eq!(
            parse_browser_cli(&args).expect("parse eval"),
            BrowserCliCommand::Eval {
                browser_id: "b-1".into(),
                script: "document.title".into(),
            }
        );
    }

    #[test]
    fn browser_wait_cli_parsing() {
        let args_sel = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--selector",
            "#submit",
            "--timeout-ms",
            "5000",
        ];
        assert_eq!(
            parse_browser_cli(&args_sel).expect("parse wait selector"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Selector {
                    selector: "#submit".into(),
                },
                timeout_ms: Some(5000),
            }
        );

        let args_text = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--text",
            "Welcome",
        ];
        assert_eq!(
            parse_browser_cli(&args_text).expect("parse wait text"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Text {
                    text: "Welcome".into(),
                },
                timeout_ms: None,
            }
        );

        let args_url = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--url-contains",
            "/dashboard",
        ];
        assert_eq!(
            parse_browser_cli(&args_url).expect("parse wait url-contains"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::UrlContains {
                    fragment: "/dashboard".into(),
                },
                timeout_ms: None,
            }
        );

        let args_load = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--load-state",
            "complete",
        ];
        assert_eq!(
            parse_browser_cli(&args_load).expect("parse wait load-state"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::LoadState {
                    state: "complete".into(),
                },
                timeout_ms: None,
            }
        );

        let args_func = vec![
            "ferryx",
            "browser",
            "wait",
            "--browser-id",
            "b-1",
            "--function",
            "window.ready === true",
        ];
        assert_eq!(
            parse_browser_cli(&args_func).expect("parse wait function"),
            BrowserCliCommand::Wait {
                browser_id: "b-1".into(),
                condition: BrowserWaitCondition::Function {
                    script: "window.ready === true".into(),
                },
                timeout_ms: None,
            }
        );
    }

    #[test]
    fn browser_screenshot_cli_parsing() {
        let args = vec![
            "ferryx",
            "browser",
            "screenshot",
            "--browser-id",
            "b-1",
            "--out",
            "/tmp/shot.png",
        ];
        assert_eq!(
            parse_browser_cli(&args).expect("parse screenshot"),
            BrowserCliCommand::Screenshot {
                browser_id: "b-1".into(),
                out_path: "/tmp/shot.png".into(),
            }
        );
    }

    #[test]
    fn browser_console_and_errors_cli_parsing() {
        let args_console = vec![
            "ferryx",
            "browser",
            "console",
            "--browser-id",
            "b-1",
            "--errors",
            "--clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_console).expect("parse console"),
            BrowserCliCommand::Console {
                browser_id: "b-1".into(),
                errors_only: true,
                clear: true,
            }
        );

        let args_errors = vec![
            "ferryx",
            "browser",
            "errors",
            "--browser-id",
            "b-1",
            "--clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_errors).expect("parse errors"),
            BrowserCliCommand::Errors {
                browser_id: "b-1".into(),
                clear: true,
            }
        );
    }

    #[test]
    fn browser_focus_cli_parsing() {
        let args = vec!["ferryx", "browser", "focus", "--browser-id", "b-1"];
        assert_eq!(
            parse_browser_cli(&args).expect("parse focus"),
            BrowserCliCommand::Focus {
                browser_id: "b-1".into(),
            }
        );
    }

    #[test]
    fn browser_cookies_cli_parsing() {
        let args_get = vec!["ferryx", "browser", "cookies", "--browser-id", "b-1", "get"];
        assert_eq!(
            parse_browser_cli(&args_get).expect("parse cookies get"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Get,
            }
        );

        let args_set = vec![
            "ferryx",
            "browser",
            "cookies",
            "--browser-id",
            "b-1",
            "set",
            "session",
            "xyz",
            "--domain",
            "example.com",
            "--path",
            "/",
        ];
        assert_eq!(
            parse_browser_cli(&args_set).expect("parse cookies set"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Set {
                    name: "session".into(),
                    value: "xyz".into(),
                    domain: Some("example.com".into()),
                    path: Some("/".into()),
                },
            }
        );

        let args_clear = vec![
            "ferryx",
            "browser",
            "cookies",
            "--browser-id",
            "b-1",
            "clear",
            "session",
        ];
        assert_eq!(
            parse_browser_cli(&args_clear).expect("parse cookies clear"),
            BrowserCliCommand::Cookies {
                browser_id: "b-1".into(),
                action: CookieCliAction::Clear {
                    name: "session".into(),
                },
            }
        );
    }

    #[test]
    fn browser_storage_cli_parsing() {
        let args_get = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "local",
            "get",
            "token",
        ];
        assert_eq!(
            parse_browser_cli(&args_get).expect("parse storage local get"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Get {
                    key: Some("token".into()),
                },
            }
        );

        let args_set = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "session",
            "set",
            "user",
            "alice",
        ];
        assert_eq!(
            parse_browser_cli(&args_set).expect("parse storage session set"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Session,
                action: StorageCliAction::Set {
                    key: "user".into(),
                    value: "alice".into(),
                },
            }
        );

        let args_clear = vec![
            "ferryx",
            "browser",
            "storage",
            "--browser-id",
            "b-1",
            "local",
            "clear",
        ];
        assert_eq!(
            parse_browser_cli(&args_clear).expect("parse storage local clear"),
            BrowserCliCommand::Storage {
                browser_id: "b-1".into(),
                kind: StorageCliKind::Local,
                action: StorageCliAction::Clear { key: None },
            }
        );
    }

    #[test]
    fn browser_open_cli_env_var_defaults() {
        let _lock = TEST_ENV_LOCK.lock().unwrap();
        let prev_ws = std::env::var("FERRYX_WORKSPACE_ID").ok();
        let prev_wt = std::env::var("FERRYX_WORKTREE_PATH").ok();

        std::env::set_var("FERRYX_WORKSPACE_ID", "ws-from-env");
        std::env::set_var("FERRYX_WORKTREE_PATH", "/path/from/env");

        let args = vec!["ferryx", "browser", "open", "--url", "https://example.com"];
        let parsed = parse_browser_cli(&args).expect("parse browser open with env defaults");

        std::env::remove_var("FERRYX_WORKSPACE_ID");
        std::env::remove_var("FERRYX_WORKTREE_PATH");

        if let Some(ws) = prev_ws {
            std::env::set_var("FERRYX_WORKSPACE_ID", ws);
        }
        if let Some(wt) = prev_wt {
            std::env::set_var("FERRYX_WORKTREE_PATH", wt);
        }

        assert_eq!(
            parsed,
            BrowserCliCommand::Open {
                url: "https://example.com".into(),
                workspace_id: Some("ws-from-env".into()),
                worktree_path: Some("/path/from/env".into()),
            }
        );
    }
}
