//! A failed connection alone does not surrender predecessor ownership.
use super::LegacyPeer;
use std::io::ErrorKind;

impl LegacyPeer {
    pub(crate) async fn confirmed_dead(&self) -> bool {
        let pid = self.socket_path().file_name().and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("legacy-"))
            .and_then(|name| name.split('-').next())
            .and_then(|pid| pid.parse::<u32>().ok());
        let Some(pid) = pid else {
            return false;
        };
        let path = self.socket_path().to_path_buf();
        let absent = crate::ipc::run_blocking(move || {
            crate::daemon::server::validate_runtime_socket_path(&path)
                .map_err(crate::ipc::IpcError::internal)?;
            Ok(process_absent(pid))
        })
        .await;
        if !matches!(absent, Ok(true)) {
            return false;
        }
        #[cfg(unix)]
        let connected = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            tokio::net::UnixStream::connect(self.socket_path()),
        )
        .await;
        #[cfg(not(unix))]
        let connected = {
            let path = self.socket_path().to_path_buf();
            let port = crate::ipc::run_blocking(move || {
                let value = match std::fs::read_to_string(path) {
                    Ok(value) => value,
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(crate::ipc::IpcError::internal(error.to_string())),
                };
                value.trim().parse::<u16>()
                    .map(Some)
                    .map_err(|error| crate::ipc::IpcError::internal(error.to_string()))
            }).await;
            let port = match port {
                Ok(Some(port)) => port,
                Ok(None) => return true,
                Err(_) => return false,
            };
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)),
            ).await
        };
        matches!(connected, Ok(Err(error)) if matches!(error.kind(), ErrorKind::ConnectionRefused | ErrorKind::NotFound))
    }
}

/// Only a complete, successful process listing proves absence. An inspection failure,
/// permission error or reused PID keeps the route; none licenses a competing controller.
fn process_absent(pid: u32) -> bool {
    #[cfg(unix)]
    let output = crate::util::no_window_command("ps").args(["-axo", "pid="]).output();
    #[cfg(windows)]
    let output = crate::util::no_window_command("tasklist")
        .args(["/FO", "CSV", "/NH"]).output();
    #[cfg(not(any(unix, windows)))]
    return false;
    #[cfg(any(unix, windows))]
    {
        let Ok(output) = output else { return false; };
        if !output.status.success() { return false; }
        let Ok(text) = std::str::from_utf8(&output.stdout) else { return false; };
        let mut count = 0;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            #[cfg(unix)]
            let parsed = line.trim().parse::<u32>().ok();
            #[cfg(windows)]
            let parsed = line.split("\",\"").nth(1).and_then(|value| value.parse::<u32>().ok());
            let Some(found) = parsed else { return false; };
            if found == pid { return false; }
            count += 1;
        }
        count > 0
    }
}
