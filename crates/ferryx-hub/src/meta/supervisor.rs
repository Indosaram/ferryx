//! Owns the omo app-server process the headless engine talks to.
//! A pid file distinguishes a daemon this hub spawned from one somebody else started.
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{HubError, Result};

#[derive(Debug, Clone)]
pub struct DaemonSupervisor {
    pub listen_url: String,
    pub command: Vec<String>,
    pub state_dir: PathBuf,
}

impl DaemonSupervisor {
    pub fn new(listen_url: impl Into<String>, command: Vec<String>, state_dir: PathBuf) -> Self {
        Self {
            listen_url: listen_url.into(),
            command,
            state_dir,
        }
    }

    pub fn port(&self) -> u16 {
        self.listen_url
            .rsplit_once(':')
            .and_then(|(_, port)| port.trim_end_matches('/').parse().ok())
            .unwrap_or(0)
    }

    pub fn pid_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("omo-appserver-{}.pid", self.port()))
    }

    pub fn token_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("omo-appserver-{}.token", self.port()))
    }

    /// Writes the app-server token owner-only. The mode is tightened again after create
    /// because an existing file keeps its old mode.
    pub fn write_token_file(&self, token: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.state_dir)?;
        let path = self.token_path();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mut options = std::fs::OpenOptions::new();
            options.create(true).write(true).truncate(true).mode(0o600);
            let file = options.open(&path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)?;
        }
        std::fs::write(&path, token)?;
        Ok(path)
    }

    pub fn record_pid(&self, pid: u32) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir)?;
        std::fs::write(self.pid_path(), format!("{pid} {}", std::process::id()))?;
        Ok(())
    }

    pub fn owned_pid(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.pid_path()).ok()?;
        text.split_whitespace().next()?.parse().ok()
    }

    pub async fn responsive(&self) -> bool {
        let host = self
            .listen_url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .trim_start_matches("http://");
        let host = host.split('/').next().unwrap_or(host);
        tokio::time::timeout(
            Duration::from_millis(200),
            tokio::net::TcpStream::connect(host),
        )
        .await
        .is_ok_and(|result| result.is_ok())
    }

    /// Spawns the configured command when nothing is listening and this hub does not
    /// already have a live owned pid. An externally managed daemon is left alone.
    pub async fn ensure(&self) -> Result<Option<u32>> {
        if self.responsive().await {
            return Ok(self.owned_pid());
        }
        if self.command.is_empty() {
            return Err(HubError::config("omo app-server command is not configured"));
        }
        if let Some(pid) = self.owned_pid() {
            if process_alive(pid) {
                return Ok(Some(pid));
            }
        }
        let mut command = crate::child_env::command(&self.command[0]);
        command.args(&self.command[1..]).kill_on_drop(false);
        let child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| HubError::message("spawned omo app-server has no pid"))?;
        self.record_pid(pid)?;
        Ok(Some(pid))
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        libc_kill(pid)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(unix)]
fn libc_kill(pid: u32) -> bool {
    // signal 0 tests existence without delivering a signal.
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0
}

#[cfg(unix)]
mod libc {
    unsafe extern "C" {
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
}

pub fn state_dir_from_home() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(Path::new(&home).join(".ferryx").join("hub"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_file_is_owner_only_on_unix() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-sup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let supervisor = DaemonSupervisor::new("ws://127.0.0.1:9", vec![], dir.clone());
        let path = supervisor.write_token_file("secret").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
