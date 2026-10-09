use std::process::{Command, ExitCode};

const INBOX_SSH: &str = r"C:\Windows\System32\OpenSSH\ssh.exe";

fn main() -> ExitCode {
    let config = match std::env::var_os("FERRYX_CAUSAL_SSH_CONFIG") {
        Some(path) => path,
        None => {
            eprintln!("FERRYX_CAUSAL_SSH_CONFIG is not set");
            return ExitCode::from(2);
        }
    };
    let known_hosts = match std::env::var_os("FERRYX_CAUSAL_KNOWN_HOSTS") {
        Some(path) => path,
        None => {
            eprintln!("FERRYX_CAUSAL_KNOWN_HOSTS is not set");
            return ExitCode::from(2);
        }
    };
    let mut command = Command::new(INBOX_SSH);
    command
        .arg("-F")
        .arg(config)
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts.to_string_lossy()))
        .args(std::env::args_os().skip(1));
    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("failed to launch {INBOX_SSH}: {error}");
            ExitCode::from(1)
        }
    }
}
