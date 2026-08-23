// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Gui,
    Daemon,
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

pub fn run_daemon_headless() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let server = Arc::new(ferryx_lib::daemon::server::DaemonServer::new());
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move { server_clone.run_server().await });

        // Ensure server listener is bound and accepting before emitting readiness signal
        let socket_path = ferryx_lib::daemon::server::get_socket_path();
        loop {
            if socket_path.exists() {
                if let Ok(_probe) = tokio::net::UnixStream::connect(&socket_path).await {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }

        println!("FERRYX_DAEMON_READY");
        let _ = std::io::stdout().flush();

        match server_task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match parse_launch_mode(&args) {
        LaunchMode::Daemon => {
            if let Err(e) = run_daemon_headless() {
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
}
