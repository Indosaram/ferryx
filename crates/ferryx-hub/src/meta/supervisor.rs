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

    pub fn log_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("omo-appserver-{}.log", self.port()))
    }

    /// Owner-only append log for the daemon's stdout and stderr, rotated past 10 MiB.
    fn open_log(&self) -> Option<(std::fs::File, std::fs::File)> {
        let path = self.log_path();
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 10 * 1024 * 1024) {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            let file = options.open(&path).ok()?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .ok()?;
            let clone = file.try_clone().ok()?;
            Some((file, clone))
        }
        #[cfg(not(unix))]
        {
            let file = options.open(&path).ok()?;
            let clone = file.try_clone().ok()?;
            Some((file, clone))
        }
    }

    /// The token file the daemon is started with. An existing non-empty file is kept so a
    /// daemon that outlived the previous hub (a handover) still accepts the token.
    pub fn ensure_token_file(&self) -> Result<PathBuf> {
        let path = self.token_path();
        if std::fs::read_to_string(&path).is_ok_and(|token| !token.trim().is_empty()) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            return Ok(path);
        }
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        self.write_token_file(&token)
    }

    /// Waits until the daemon accepts connections, up to `limit`.
    pub async fn wait_responsive(&self, limit: Duration) -> bool {
        let until = tokio::time::Instant::now() + limit;
        loop {
            if self.responsive().await {
                return true;
            }
            if tokio::time::Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
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
        command
            .args(&self.command[1..])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(false);
        // Engine tool calls inherit the app-server's PATH, so a bare `launchctl` resolves to
        // the guard first (macOS only; launchd does not exist elsewhere).
        if cfg!(target_os = "macos") {
            match install_launchctl_guard(&self.state_dir) {
                Ok(dir) => {
                    let mut path = std::ffi::OsString::from(dir.as_os_str());
                    path.push(":");
                    path.push(crate::child_env::child_path());
                    command.env("PATH", path);
                }
                Err(error) => {
                    tracing::warn!(%error, "launchctl guard for the omo app-server was not installed")
                }
            }
        }
        match self.open_log() {
            Some((out, err)) => {
                command.stdout(out).stderr(err);
            }
            None => {
                command
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
            }
        }
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

/// omo binary for the app-server: `OMON_OMO_BIN`, else `omo` on the hub child PATH.
pub fn omo_bin() -> String {
    crate::hub_env::var("OMON_OMO_BIN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "omo".into())
}

/// `<bin> app-server --listen <url> --ws-auth <token file>`, the command omo-gateway spawned.
pub fn app_server_command(bin: &str, listen_url: &str, token_file: &Path) -> Vec<String> {
    vec![
        bin.to_owned(),
        "app-server".into(),
        "--listen".into(),
        listen_url.to_owned(),
        "--ws-auth".into(),
        token_file.display().to_string(),
    ]
}

pub fn state_dir_from_home() -> Option<PathBuf> {
    let home = crate::hub_env::var_os("HOME").or_else(|| crate::hub_env::var_os("USERPROFILE"))?;
    Some(Path::new(&home).join(".ferryx").join("hub"))
}

/// `launchctl` wrapper put first on the app-server PATH. It refuses to start, enable, or load
/// the retired omo-gateway (`ai.omon.gateway*`) and passes every other call to the real
/// launchctl. The approval guard in `omo_client` covers absolute paths and other spellings.
const LAUNCHCTL_GUARD: &str = r#"#!/bin/sh
# Installed by the ferryx hub. omo-gateway is retired; the ferryx hub replaced it.
real="${FERRYX_REAL_LAUNCHCTL:-/bin/launchctl}"
case "$1" in
  bootstrap|enable|kickstart|load|start|submit)
    for arg in "$@"; do
      case "$arg" in
        *ai.omon.gateway*)
          echo "launchctl: refused: omo-gateway is intentionally retired and replaced by the ferryx hub; do not start it again" >&2
          exit 1
          ;;
      esac
    done
    ;;
esac
exec "$real" "$@"
"#;

/// Writes the guard to `<state_dir>/guard-bin/launchctl` and returns that directory.
pub fn install_launchctl_guard(state_dir: &Path) -> Result<PathBuf> {
    let dir = state_dir.join("guard-bin");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("launchctl");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(LAUNCHCTL_GUARD) {
        std::fs::write(&path, LAUNCHCTL_GUARD)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_launchctl_guard_refuses_only_starting_the_retired_gateway() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = install_launchctl_guard(&dir).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new(bin.join("launchctl"))
                .args(args)
                .env("FERRYX_REAL_LAUNCHCTL", "/bin/echo")
                .output()
                .unwrap()
        };
        for blocked in [
            &[
                "bootstrap",
                "gui/501",
                "/Users/x/Library/LaunchAgents/ai.omon.gateway.plist",
            ][..],
            &["enable", "gui/501/ai.omon.gateway"],
            &["kickstart", "-k", "gui/501/ai.omon.gateway"],
            &[
                "load",
                "-w",
                "/Users/x/Library/LaunchAgents/ai.omon.gateway-watchdog.plist",
            ],
        ] {
            let output = run(blocked);
            assert!(!output.status.success(), "{blocked:?}");
            assert!(output.stdout.is_empty(), "{blocked:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("retired"));
        }
        for allowed in [
            &["print", "gui/501/ai.omon.gateway"][..],
            &["bootout", "gui/501/ai.omon.gateway"],
            &["disable", "gui/501/ai.omon.gateway"],
            &["kickstart", "-k", "gui/501/ai.omon.tracker-proxy"],
        ] {
            let output = run(allowed);
            assert!(output.status.success(), "{allowed:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                allowed.join(" ")
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_token_survives_and_the_command_matches_omo_gateway() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-sup-tok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let supervisor = DaemonSupervisor::new("ws://127.0.0.1:19742", vec![], dir.clone());
        let path = supervisor.ensure_token_file().unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(supervisor.ensure_token_file().unwrap(), path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        assert!(path.ends_with("omo-appserver-19742.token"));
        assert_eq!(
            app_server_command("/bin/omo", "ws://127.0.0.1:19742", &path),
            [
                "/bin/omo",
                "app-server",
                "--listen",
                "ws://127.0.0.1:19742",
                "--ws-auth",
                path.to_str().unwrap()
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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
