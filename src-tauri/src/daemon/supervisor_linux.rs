//! Linux side of the daemon supervisor: the systemd user unit that owns the daemon's lifetime, and
//! the `sd_notify` messages the daemon sends to it.
//!
//! The unit is `Type=notify`: the daemon reports `READY=1` once it serves its socket. A handover
//! successor is forked by the predecessor, so it lives in the unit's cgroup; it reports
//! `MAINPID=<own pid>` + `READY=1` as soon as the predecessor confirms the commit, before the
//! predecessor exits 0, so systemd supervises the successor from then on (`NotifyAccess=all` lets a
//! non-main process send it). `KillMode=process` is what keeps the successor and every PTY child
//! alive when the predecessor exits: the default `control-group` mode would kill the whole cgroup.
//! `Restart=on-failure` restarts a crash but never a deliberate exit 0.
//!
//! `NOTIFY_SOCKET` is taken out of the daemon's environment at startup, so a program running in a
//! terminal pane cannot retarget the unit's main PID; only the daemon's own successors get it back.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Unit name used when no QA override is set.
pub const DEFAULT_UNIT_NAME: &str = "ferryx-daemon";
const NOTIFY_SOCKET_ENV: &str = "NOTIFY_SOCKET";

/// `<name>` from the QA override (a trailing `.service` is accepted), else [`DEFAULT_UNIT_NAME`].
pub(crate) fn unit_name(override_name: Option<&OsStr>) -> Result<String, String> {
    let name = match override_name.filter(|name| !name.is_empty()) {
        Some(name) => name
            .to_str()
            .ok_or("The supervisor unit name is not valid UTF-8")?,
        None => DEFAULT_UNIT_NAME,
    };
    let name = name.strip_suffix(".service").unwrap_or(name);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '@'));
    if !valid {
        return Err(format!("Invalid systemd unit name: {name:?}"));
    }
    Ok(name.to_string())
}

/// `$XDG_CONFIG_HOME/systemd/user/<name>.service`, falling back to `~/.config` when the variable is
/// unset or not absolute (the XDG base directory rule).
pub(crate) fn unit_path(
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
    name: &str,
) -> Option<PathBuf> {
    let config = xdg_config_home
        .map(Path::new)
        .filter(|dir| dir.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| {
            home.filter(|home| !home.is_empty())
                .map(|home| Path::new(home).join(".config"))
        })?;
    Some(config.join("systemd/user").join(format!("{name}.service")))
}

/// The executable the unit runs: an AppImage's own path when the app runs from one, because the
/// `/tmp/.mount_*` path `current_exe` reports disappears as soon as the GUI exits.
pub(crate) fn daemon_executable(appimage: Option<&OsStr>, current_exe: &Path) -> PathBuf {
    appimage
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| current_exe.to_path_buf())
}

fn forwarded_key(key: &str) -> bool {
    let ferryx = key.starts_with("FERRYX_") && key != "FERRYX_DAEMON_SUCCESSOR";
    let locale = key == "LANG" || key == "LANGUAGE" || key.starts_with("LC_");
    (ferryx || locale) && key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Only `FERRYX_*` and locale variables reach the unit; `PATH` is rebuilt for every PTY child, and
/// anything else would freeze one GUI session's environment into the user's login service.
pub(crate) fn forwarded_env(vars: &[(OsString, OsString)]) -> Vec<(String, String)> {
    let mut forwarded: Vec<(String, String)> = vars
        .iter()
        .filter_map(|(key, value)| {
            let key = key.to_str().filter(|key| forwarded_key(key))?;
            let value = value
                .to_str()
                .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    // Sorted, so an unchanged environment renders a byte-identical unit and is not rewritten.
    forwarded.sort();
    forwarded.dedup_by(|a, b| a.0 == b.0);
    forwarded
}

/// A double-quoted systemd unit value. `%` starts a specifier everywhere; `$` expands a variable
/// only in `ExecStart=`.
fn quoted(value: &str, escape_dollar: bool) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' if escape_dollar => out.push_str("$$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Renders the user unit for `executable`, forwarding the allowed subset of `vars`.
pub(crate) fn render_unit(executable: &Path, vars: &[(OsString, OsString)]) -> Result<String, String> {
    let exe = executable
        .to_str()
        .filter(|exe| !exe.contains(['\n', '\r']))
        .ok_or_else(|| format!("Unsupported daemon executable path: {}", executable.display()))?;
    let mut unit = format!(
        "# Managed by Ferryx: rewritten when the app or its environment changes.\n\
         [Unit]\n\
         Description=Ferryx terminal daemon\n\
         \n\
         [Service]\n\
         Type=notify\n\
         NotifyAccess=all\n\
         ExecStart={} --daemon\n\
         Restart=on-failure\n\
         RestartSec=1\n\
         KillMode=process\n\
         LimitNOFILE=4096\n",
        quoted(exe, true)
    );
    for (key, value) in forwarded_env(vars) {
        unit.push_str(&format!("Environment={}\n", quoted(&format!("{key}={value}"), false)));
    }
    unit.push_str("\n[Install]\nWantedBy=default.target\n");
    Ok(unit)
}

/// Writes the unit only when its content differs, and reports whether it was rewritten (same rule
/// as `launchd::write_launchd_plist`).
pub(crate) fn write_unit(content: &str, path: &Path) -> Result<bool, String> {
    if fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create the systemd user unit directory: {e}"))?;
    }
    fs::write(path, content).map_err(|e| format!("Failed to write the systemd user unit: {e}"))?;
    Ok(true)
}

/// Why no systemd user manager can supervise the daemon here, or `None` when one can.
pub(crate) fn unavailable_reason(
    env: &dyn Fn(&str) -> Option<OsString>,
    exists: &dyn Fn(&Path) -> bool,
    uid: u32,
) -> Option<String> {
    let set = |key: &str| env(key).is_some_and(|value| !value.is_empty());
    if set("WSL_DISTRO_NAME") || set("WSL_INTEROP") {
        return Some("WSL has no dependable systemd user manager".to_string());
    }
    if set("container") || exists(Path::new("/.dockerenv")) || exists(Path::new("/run/.containerenv"))
    {
        return Some("running inside a container".to_string());
    }
    if !exists(Path::new("/run/systemd/system")) {
        return Some("systemd is not the init system".to_string());
    }
    let on_path = env("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| exists(&dir.join("systemctl")))
    });
    if !on_path {
        return Some("systemctl is not on PATH".to_string());
    }
    let runtime_dir = env("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    if !exists(&runtime_dir.join("systemd/private")) {
        return Some(format!(
            "no systemd user manager is running ({} is missing)",
            runtime_dir.join("systemd/private").display()
        ));
    }
    None
}

fn current_uid() -> u32 {
    // SAFETY: `libc::getuid` takes no arguments, dereferences no pointers, and has no side effects.
    unsafe { libc::getuid() }
}

/// [`unavailable_reason`] for this process, probed once (a few `stat`s of tmpfs paths).
pub fn availability() -> &'static Option<String> {
    static AVAILABILITY: OnceLock<Option<String>> = OnceLock::new();
    AVAILABILITY.get_or_init(|| {
        unavailable_reason(&|key| std::env::var_os(key), &|path| path.exists(), current_uid())
    })
}

#[cfg(target_os = "linux")]
fn systemctl(args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("systemctl");
    command
        .arg("--user")
        .args(args)
        .stdin(std::process::Stdio::null());
    // `systemctl --user` finds the manager through XDG_RUNTIME_DIR, which some sessions (SSH, some
    // desktop launchers) leave unset.
    if std::env::var_os("XDG_RUNTIME_DIR").is_none_or(|dir| dir.is_empty()) {
        command.env("XDG_RUNTIME_DIR", format!("/run/user/{}", current_uid()));
    }
    let output = command
        .output()
        .map_err(|e| format!("systemctl --user {}: {e}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn unit_is_running(name: &str) -> bool {
    let unit = format!("{name}.service");
    systemctl(&["show", "-p", "ActiveState", "--value", &unit]).is_ok_and(|state| {
        matches!(state.as_str(), "active" | "activating" | "reloading")
    })
}

/// Writes the unit for `executable`, reloads systemd when it changed, and enables and starts it
/// when it was rewritten or is not running. A running daemon is never restarted.
#[cfg(target_os = "linux")]
pub(crate) fn ensure(name: &str, path: &Path, executable: &Path) -> Result<bool, String> {
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let content = render_unit(executable, &vars)?;
    let rewritten = write_unit(&content, path)?;
    if rewritten {
        systemctl(&["daemon-reload"])?;
    }
    if rewritten || !unit_is_running(name) {
        systemctl(&["enable", "--now", "--no-block", &format!("{name}.service")])?;
        return Ok(true);
    }
    Ok(false)
}

/// Starts the unit again; a unit stopped by its start limit is reset first.
#[cfg(target_os = "linux")]
pub(crate) fn start(name: &str) -> Result<(), String> {
    let unit = format!("{name}.service");
    let _ = systemctl(&["reset-failed", &unit]);
    systemctl(&["start", "--no-block", &unit]).map(|_| ())
}

/// `disable --now` and delete the unit file; a missing unit is success.
#[cfg(target_os = "linux")]
pub(crate) fn uninstall(name: &str, path: &Path) -> Result<(), String> {
    let unit = format!("{name}.service");
    if let Err(error) = systemctl(&["disable", "--now", &unit]) {
        if path.exists() {
            return Err(error);
        }
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to remove the systemd user unit: {error}")),
    }
    systemctl(&["daemon-reload"]).map(|_| ())
}

static NOTIFY_SOCKET: OnceLock<Option<OsString>> = OnceLock::new();

fn notify_socket() -> Option<&'static OsString> {
    NOTIFY_SOCKET
        .get_or_init(|| std::env::var_os(NOTIFY_SOCKET_ENV).filter(|socket| !socket.is_empty()))
        .as_ref()
}

/// Takes `NOTIFY_SOCKET` out of the process environment so PTY children never inherit it. Must run
/// before the daemon starts any thread.
pub fn capture_notify_socket() {
    let _ = notify_socket();
    std::env::remove_var(NOTIFY_SOCKET_ENV);
}

/// Hands `NOTIFY_SOCKET` back to a successor daemon, so it can report itself to systemd.
pub fn forward_notify_socket(command: &mut std::process::Command) {
    if let Some(socket) = notify_socket() {
        command.env(NOTIFY_SOCKET_ENV, socket);
    }
}

pub(crate) const READY: &str = "READY=1";

pub(crate) fn main_pid_message(pid: u32) -> String {
    format!("MAINPID={pid}\n{READY}")
}

/// One `sd_notify` datagram to `socket` (a path, or a Linux abstract name starting with `@`).
pub(crate) fn send_notify(socket: &OsStr, message: &str) -> std::io::Result<()> {
    let sender = UnixDatagram::unbound()?;
    let bytes = socket.as_bytes();
    if let Some(name) = bytes.strip_prefix(b"@") {
        #[cfg(target_os = "linux")]
        {
            use std::os::linux::net::SocketAddrExt as _;
            let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
            sender.send_to_addr(message.as_bytes(), &address)?;
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "abstract notify sockets exist only on Linux",
            ));
        }
    }
    sender.send_to(message.as_bytes(), Path::new(socket))?;
    Ok(())
}

fn notify(message: &str) {
    if let Some(socket) = notify_socket() {
        if let Err(error) = send_notify(socket, message) {
            tracing::warn!(%error, message, "Could not notify the systemd supervisor");
        }
    }
}

/// Tells systemd the daemon serves its socket.
pub fn notify_ready() {
    notify(READY);
}

/// Makes this process the unit's main process. A handover successor sends it once the predecessor
/// confirmed the commit and before that predecessor exits 0, so systemd keeps supervising (and
/// restarts) the daemon that now owns the sessions.
pub fn notify_main_pid_ready() {
    notify(&main_pid_message(std::process::id()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    }

    fn render(exe: &str) -> String {
        render_unit(Path::new(exe), &[]).unwrap()
    }

    fn service_lines(unit: &str) -> Vec<&str> {
        unit.lines().map(str::trim).collect()
    }

    #[test]
    fn unit_restarts_only_on_failure() {
        let unit = render("/opt/ferryx/ferryx");
        let lines = service_lines(&unit);
        assert!(lines.contains(&"Restart=on-failure"), "{unit}");
        assert!(lines.contains(&"RestartSec=1"), "{unit}");
        assert!(lines.contains(&"Type=notify"), "{unit}");
        assert!(lines.contains(&"NotifyAccess=all"), "{unit}");
        assert_eq!(
            lines.iter().filter(|line| line.starts_with("Restart")).count(),
            2,
            "no other restart policy (always, RestartForceExitStatus) may restart an exit 0: {unit}"
        );
        assert!(!unit.contains("SuccessExitStatus"), "{unit}");
        assert!(lines.contains(&"LimitNOFILE=4096"), "{unit}");
        assert!(lines.contains(&"WantedBy=default.target"), "{unit}");
    }

    #[test]
    fn unit_uses_killmode_process() {
        let unit = render("/opt/ferryx/ferryx");
        let kill_modes: Vec<&str> = service_lines(&unit)
            .into_iter()
            .filter(|line| line.starts_with("KillMode="))
            .collect();
        assert_eq!(kill_modes, ["KillMode=process"], "{unit}");
    }

    #[test]
    fn unit_prefers_appimage_path() {
        let mounted = Path::new("/tmp/.mount_FerryxAbC/usr/bin/ferryx");
        let appimage = OsStr::new("/home/u/Apps/Ferryx.AppImage");
        let exe = daemon_executable(Some(appimage), mounted);
        assert_eq!(exe, Path::new("/home/u/Apps/Ferryx.AppImage"));
        let unit = render_unit(&exe, &[]).unwrap();
        assert!(
            unit.contains("ExecStart=\"/home/u/Apps/Ferryx.AppImage\" --daemon\n"),
            "{unit}"
        );
        assert!(!unit.contains(".mount_"), "{unit}");
        assert_eq!(daemon_executable(Some(OsStr::new("")), mounted), mounted);
        assert_eq!(daemon_executable(None, mounted), mounted);
        let odd = render("/home/u/100%/$HOME/fer\"ryx");
        assert!(
            odd.contains("ExecStart=\"/home/u/100%%/$$HOME/fer\\\"ryx\" --daemon\n"),
            "specifiers, variables and quotes must be escaped: {odd}"
        );
    }

    #[test]
    fn unit_forwards_only_ferryx_and_locale_env() {
        let env = vars(&[
            ("PATH", "/usr/bin"),
            ("HOME", "/home/u"),
            ("NOTIFY_SOCKET", "/run/user/1000/systemd/notify"),
            ("DISPLAY", ":0"),
            ("APPIMAGE", "/home/u/Ferryx.AppImage"),
            ("LC_ALL", "ko_KR.UTF-8"),
            ("LANG", "en_US.UTF-8"),
            ("LANGUAGE", "ko"),
            ("FERRYX_DATA_DIR", "/tmp/qa 100%"),
            ("FERRYX_DAEMON_SUCCESSOR", "1"),
            ("FERRYX_EMPTY", ""),
            ("FERRYX_MULTILINE", "a\nb"),
        ]);
        let forwarded: HashMap<String, String> = forwarded_env(&env).into_iter().collect();
        let mut keys: Vec<&str> = forwarded.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["FERRYX_DATA_DIR", "LANG", "LANGUAGE", "LC_ALL"]);
        let unit = render_unit(Path::new("/opt/ferryx/ferryx"), &env).unwrap();
        let environment: Vec<&str> = unit
            .lines()
            .filter(|line| line.starts_with("Environment="))
            .collect();
        assert_eq!(
            environment,
            [
                "Environment=\"FERRYX_DATA_DIR=/tmp/qa 100%%\"",
                "Environment=\"LANG=en_US.UTF-8\"",
                "Environment=\"LANGUAGE=ko\"",
                "Environment=\"LC_ALL=ko_KR.UTF-8\"",
            ]
        );
        let mut reversed = env.clone();
        reversed.reverse();
        assert_eq!(
            render_unit(Path::new("/opt/ferryx/ferryx"), &reversed).unwrap(),
            unit,
            "environment order must not change the rendered unit"
        );
    }

    #[test]
    fn unit_path_respects_xdg_config_home() {
        let home = Some(OsStr::new("/home/u"));
        assert_eq!(
            unit_path(Some(OsStr::new("/srv/cfg")), home, "ferryx-daemon"),
            Some(PathBuf::from("/srv/cfg/systemd/user/ferryx-daemon.service"))
        );
        assert_eq!(
            unit_path(None, home, "ferryx-daemon"),
            Some(PathBuf::from("/home/u/.config/systemd/user/ferryx-daemon.service"))
        );
        assert_eq!(
            unit_path(Some(OsStr::new("relative/cfg")), home, "ferryx-daemon"),
            Some(PathBuf::from("/home/u/.config/systemd/user/ferryx-daemon.service")),
            "a relative XDG_CONFIG_HOME is ignored"
        );
        assert_eq!(unit_path(None, None, "ferryx-daemon"), None);
        assert_eq!(unit_name(None).unwrap(), "ferryx-daemon");
        assert_eq!(
            unit_name(Some(OsStr::new("ferryx-daemon-qa.service"))).unwrap(),
            "ferryx-daemon-qa"
        );
        assert!(unit_name(Some(OsStr::new("../escape"))).is_err());
    }

    #[test]
    fn write_reports_only_real_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg/systemd/user/ferryx-daemon.service");
        let unit = render("/opt/ferryx/ferryx");
        assert!(write_unit(&unit, &path).unwrap());
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(
            !write_unit(&unit, &path).unwrap(),
            "an unchanged unit must not be rewritten on every GUI start"
        );
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        let next = render("/opt/ferryx/ferryx-next");
        assert!(write_unit(&next, &path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), next);
    }

    #[test]
    fn unavailable_systemctl_falls_back() {
        let probe = |paths: &'static [&'static str]| {
            move |path: &Path| paths.iter().any(|known| Path::new(known) == path)
        };
        let env_of = |pairs: Vec<(&'static str, &'static str)>| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        const HEALTHY: [&str; 3] = [
            "/run/systemd/system",
            "/usr/bin/systemctl",
            "/run/user/1000/systemd/private",
        ];
        let healthy_env = env_of(vec![("PATH", "/usr/local/bin:/usr/bin")]);
        assert_eq!(unavailable_reason(&healthy_env, &probe(&HEALTHY), 1000), None);

        let no_systemctl = env_of(vec![("PATH", "/opt/empty")]);
        assert_eq!(
            unavailable_reason(&no_systemctl, &probe(&HEALTHY), 1000).as_deref(),
            Some("systemctl is not on PATH")
        );
        let wsl = env_of(vec![("PATH", "/usr/bin"), ("WSL_DISTRO_NAME", "Ubuntu")]);
        assert!(unavailable_reason(&wsl, &probe(&HEALTHY), 1000).is_some());
        let container = env_of(vec![("PATH", "/usr/bin"), ("container", "podman")]);
        assert!(unavailable_reason(&container, &probe(&HEALTHY), 1000).is_some());
        const NO_INIT: [&str; 2] = ["/usr/bin/systemctl", "/run/user/1000/systemd/private"];
        assert_eq!(
            unavailable_reason(&healthy_env, &probe(&NO_INIT), 1000).as_deref(),
            Some("systemd is not the init system")
        );
        const NO_USER_BUS: [&str; 2] = ["/run/systemd/system", "/usr/bin/systemctl"];
        assert!(unavailable_reason(&healthy_env, &probe(&NO_USER_BUS), 1000)
            .is_some_and(|reason| reason.contains("no systemd user manager")));
        let custom_runtime = env_of(vec![("PATH", "/usr/bin"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
        assert_eq!(unavailable_reason(&custom_runtime, &probe(&HEALTHY), 1000), None);
    }

    #[test]
    fn sd_notify_formats_mainpid_and_ready() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("notify");
        let receiver = UnixDatagram::bind(&socket_path).unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0u8; 256];

        send_notify(socket_path.as_os_str(), &main_pid_message(4242)).unwrap();
        let read = receiver.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"MAINPID=4242\nREADY=1");

        send_notify(socket_path.as_os_str(), READY).unwrap();
        let read = receiver.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"READY=1");
    }
}
