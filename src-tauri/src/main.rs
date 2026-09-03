// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::sync::Arc;

use ferryx_lib::browser::BrowserAutomationAction;
use ferryx_lib::ipc::browser_cli::{
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
            request: ferryx_lib::browser::BrowserAutomationRequest {
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
            request: ferryx_lib::browser::BrowserAutomationRequest {
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
            request: ferryx_lib::browser::BrowserAutomationRequest {
                browser_id,
                generation,
                action: BrowserAutomationAction::Keypress { key },
            },
        },
    }
}

fn print_browser_cli_error(code: &str, message: impl AsRef<str>) {
    eprintln!(
        "{}",
        serde_json::json!({ "type": "error", "code": code, "message": message.as_ref() })
    );
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
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = Arc::new(ferryx_lib::daemon::server::DaemonServer::new());
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone
                .run_server_with_handover_and_readiness(handover_from, Some(ready_tx))
                .await
        });

        // Wait for server to bind listener and initialize before emitting readiness signal
        match ready_rx.await {
            Ok(()) => {
                println!("FERRYX_DAEMON_READY");
                let _ = std::io::stdout().flush();
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "browser") {
        match parse_browser_cli(&args).and_then(run_browser_cli) {
            Ok(()) => return,
            Err(error) => {
                print_browser_cli_error("BROWSER_CLI_INVALID_OR_UNAVAILABLE", error);
                std::process::exit(2);
            }
        }
    }
    match parse_launch_mode(&args) {
        LaunchMode::Daemon => {
            let handover_from = parse_handover_from(&args);
            if let Err(e) = run_daemon_headless(handover_from) {
                eprintln!("Ferryx daemon error: {e}");
                std::process::exit(1);
            }
        }
        LaunchMode::Gui => {
            ferryx_lib::run();
        }
    }
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
