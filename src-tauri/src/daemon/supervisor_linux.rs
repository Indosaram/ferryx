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
//!
//! Secrets the daemon needs at startup (`FERRYX_MACHINE_TOKEN`) never go into the unit file. They
//! go into a 0600 file in `$XDG_RUNTIME_DIR` (a per-user tmpfs, cleared on logout), which the unit
//! reads through an optional `EnvironmentFile=-`, so a systemd restart still sees them.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
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

/// The `FERRYX_*` configuration the daemon reads at startup and while it runs. Anything else with
/// that prefix stays out: a GUI launched from inside a Ferryx pane carries per-session variables
/// (`FERRYX_SESSION_ID`, `FERRYX_PTY_INCARNATION`) and secrets (`FERRYX_AGENT_STATE_TOKEN`,
/// `FERRYX_MACHINE_TOKEN`) that must never be written into a unit file on disk.
const FORWARDED_FERRYX_KEYS: [&str; 7] = [
    "FERRYX_ALLOW_INSECURE_RELAY",
    "FERRYX_DATA_DIR",
    "FERRYX_RUNTIME_DIR",
    "FERRYX_SESSION_DIR",
    "FERRYX_HANDOVER_V5",
    "FERRYX_MAX_MACHINE_SESSIONS",
    "FERRYX_RELAY_URL",
];

/// Secrets the daemon reads at startup. They reach the unit only through the runtime env file
/// ([`secret_env_path`]), never through `Environment=`.
const SECRET_FERRYX_KEYS: [&str; 1] = ["FERRYX_MACHINE_TOKEN"];

/// Subdirectory of `$XDG_RUNTIME_DIR` (systemd's `%t` in a user unit) holding the secret env file.
const SECRET_ENV_SUBDIR: &str = "ferryx";

fn forwarded_key(key: &str) -> bool {
    let locale = key == "LANG" || key == "LANGUAGE" || key.starts_with("LC_");
    let locale = locale && key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    locale || FORWARDED_FERRYX_KEYS.contains(&key)
}

/// The non-empty single-line UTF-8 entries of `vars` whose key `keep` accepts, sorted so an
/// unchanged environment renders byte-identical output and nothing is rewritten.
fn filtered_env(vars: &[(OsString, OsString)], keep: fn(&str) -> bool) -> Vec<(String, String)> {
    let mut forwarded: Vec<(String, String)> = vars
        .iter()
        .filter_map(|(key, value)| {
            let key = key.to_str().filter(|key| keep(key))?;
            let value = value
                .to_str()
                .filter(|value| !value.is_empty() && !value.contains(['\n', '\r']))?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    forwarded.sort();
    forwarded
}

/// Only the allowlisted `FERRYX_*` and locale variables reach the unit; `PATH` is rebuilt for every
/// PTY child, and anything else would freeze one GUI session's environment into the user's login
/// service.
pub(crate) fn forwarded_env(vars: &[(OsString, OsString)]) -> Vec<(String, String)> {
    filtered_env(vars, forwarded_key)
}

/// The secrets of `vars` the daemon needs after a supervisor restart.
pub(crate) fn secret_env(vars: &[(OsString, OsString)]) -> Vec<(String, String)> {
    filtered_env(vars, |key| SECRET_FERRYX_KEYS.contains(&key))
}

/// `$XDG_RUNTIME_DIR/ferryx/<name>.env`, the file the unit's `EnvironmentFile=-%t/ferryx/%N.env`
/// reads. `None` when the runtime dir is unset or not absolute: secrets then stay out of the unit's
/// environment rather than falling back to persistent storage.
pub(crate) fn secret_env_path(runtime_dir: Option<&OsStr>, name: &str) -> Option<PathBuf> {
    let runtime_dir = runtime_dir.map(Path::new).filter(|dir| dir.is_absolute())?;
    Some(runtime_dir.join(SECRET_ENV_SUBDIR).join(format!("{name}.env")))
}

/// A double-quoted `EnvironmentFile=` value: inside double quotes systemd unescapes a backslash,
/// a double quote, `$` and a backtick. Specifiers (`%`) are not expanded in an env file.
fn env_file_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if matches!(ch, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// The secret env file's content, or `None` when `vars` carries no secret (the file is removed).
pub(crate) fn render_secret_env(vars: &[(OsString, OsString)]) -> Option<String> {
    let secrets = secret_env(vars);
    if secrets.is_empty() {
        return None;
    }
    let mut content = String::from("# Managed by Ferryx: daemon secrets for supervisor restarts.\n");
    for (key, value) in secrets {
        content.push_str(&format!("{key}={}\n", env_file_quoted(&value)));
    }
    Some(content)
}

fn remove_if_present(path: &Path) -> Result<bool, String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Failed to remove the daemon secret env file {}: {error}",
            path.display()
        )),
    }
}

/// Makes `path` hold `content` with mode 0600, or removes it when `content` is `None`. Returns
/// whether the file changed: an identical 0600 regular file is left alone. A new file is written to
/// a 0600 temp file in the same directory and renamed over `path`, so a reader never sees a partial
/// or wider-permission file. Errors name the path only, never the content.
pub(crate) fn write_secret_env(content: Option<&str>, path: &Path) -> Result<bool, String> {
    let Some(content) = content else {
        return remove_if_present(path);
    };
    let unchanged = fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o777 == 0o600)
        && fs::read_to_string(path).is_ok_and(|existing| existing == content);
    if unchanged {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid daemon secret env path: {}", path.display()))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("Invalid daemon secret env path: {}", path.display()))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    let written = (|| -> std::io::Result<()> {
        if let Err(error) = fs::remove_file(&temp) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error);
            }
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        // `mode` is filtered by the umask; set it explicitly before any secret byte lands.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Failed to write the daemon secret env file {}: {error}",
            path.display()
        ));
    }
    Ok(true)
}

/// Writes or removes this process's secret env file for unit `name`. Without `XDG_RUNTIME_DIR` the
/// secrets are skipped (logged once) rather than stored on persistent disk.
#[cfg(target_os = "linux")]
fn sync_secret_env(name: &str, vars: &[(OsString, OsString)]) -> Result<bool, String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    let content = render_secret_env(vars);
    match secret_env_path(runtime_dir.as_deref(), name) {
        Some(path) => write_secret_env(content.as_deref(), &path),
        None => {
            if content.is_some() {
                static SKIPPED: std::sync::Once = std::sync::Once::new();
                SKIPPED.call_once(|| {
                    tracing::warn!(
                        "XDG_RUNTIME_DIR is unset; daemon secrets will not survive a supervisor restart"
                    );
                });
            }
            Ok(false)
        }
    }
}

/// Removes the secret env file for unit `name`, if any. Used when the unit is disabled or removed.
#[cfg(target_os = "linux")]
pub(crate) fn remove_secret_env(name: &str) -> Result<(), String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    match secret_env_path(runtime_dir.as_deref(), name) {
        Some(path) => remove_if_present(&path).map(|_| ()),
        None => Ok(()),
    }
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

/// Renders the user unit for `executable`, forwarding the allowed subset of `vars`. Secrets are
/// never rendered here: the unit always names the optional runtime env file (`%t` is
/// `$XDG_RUNTIME_DIR`, `%N` the unit name without `.service`), so a secret appearing, changing or
/// going away never rewrites the unit.
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
         LimitNOFILE=4096\n\
         EnvironmentFile=-%t/{}/%N.env\n",
        quoted(exe, true),
        SECRET_ENV_SUBDIR
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
    // Before any start below, so a freshly started daemon already sees the secrets. A failure only
    // costs the token after a restart, so it does not stop the unit from supervising the daemon.
    if let Err(error) = sync_secret_env(name, &vars) {
        tracing::warn!(%error, unit = %name, "Could not update the daemon secret env file");
    }
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
    remove_secret_env(name)?;
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
            tracing::warn!(%error, notify_state = message, "Could not notify the systemd supervisor");
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
    fn unit_never_forwards_agent_state_token() {
        let env = vars(&[
            ("FERRYX_DATA_DIR", "/home/u/.ferryx-qa"),
            ("FERRYX_AGENT_STATE_TOKEN", "secret-token-value"),
            ("FERRYX_AGENT_STATE_PORT", "43210"),
            ("FERRYX_SESSION_ID", "pty-session-1234"),
            ("FERRYX_PTY_INCARNATION", "7"),
            ("FERRYX_MACHINE_TOKEN", "machine-secret"),
        ]);
        let keys: Vec<String> = forwarded_env(&env).into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["FERRYX_DATA_DIR"]);
        let unit = render_unit(Path::new("/opt/ferryx/ferryx"), &env).unwrap();
        assert!(
            unit.contains("Environment=\"FERRYX_DATA_DIR=/home/u/.ferryx-qa\"\n"),
            "{unit}"
        );
        for leaked in [
            "FERRYX_AGENT_STATE_TOKEN",
            "secret-token-value",
            "FERRYX_SESSION_ID",
            "pty-session-1234",
            "FERRYX_MACHINE_TOKEN",
        ] {
            assert!(!unit.contains(leaked), "{leaked} leaked into the unit: {unit}");
        }
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
    fn allow_insecure_relay_is_forwarded() {
        let env = vars(&[
            ("FERRYX_RELAY_URL", "ws://relay.test:8080"),
            ("FERRYX_ALLOW_INSECURE_RELAY", "1"),
        ]);
        let keys: Vec<String> = forwarded_env(&env).into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["FERRYX_ALLOW_INSECURE_RELAY", "FERRYX_RELAY_URL"]);
        let unit = render_unit(Path::new("/opt/ferryx/ferryx"), &env).unwrap();
        let environment: Vec<&str> = unit
            .lines()
            .filter(|line| line.starts_with("Environment="))
            .collect();
        assert_eq!(
            environment,
            [
                "Environment=\"FERRYX_ALLOW_INSECURE_RELAY=1\"",
                "Environment=\"FERRYX_RELAY_URL=ws://relay.test:8080\"",
            ]
        );
    }

    #[test]
    fn machine_token_never_rendered_into_unit() {
        let base = vars(&[("FERRYX_DATA_DIR", "/home/u/.ferryx")]);
        let mut with_token = base.clone();
        with_token.push((
            OsString::from("FERRYX_MACHINE_TOKEN"),
            OsString::from("mt-super-secret-value"),
        ));
        let exe = Path::new("/opt/ferryx/ferryx");
        let unit = render_unit(exe, &with_token).unwrap();
        assert!(!unit.contains("FERRYX_MACHINE_TOKEN"), "{unit}");
        assert!(!unit.contains("mt-super-secret-value"), "{unit}");
        assert!(
            service_lines(&unit).contains(&"EnvironmentFile=-%t/ferryx/%N.env"),
            "the unit must read secrets from the optional runtime env file: {unit}"
        );
        assert_eq!(
            render_unit(exe, &base).unwrap(),
            unit,
            "a secret appearing or changing must not rewrite the unit"
        );
        let secret_keys: Vec<String> =
            secret_env(&with_token).into_iter().map(|(key, _)| key).collect();
        assert_eq!(secret_keys, ["FERRYX_MACHINE_TOKEN"]);
        let agent = vars(&[("FERRYX_AGENT_STATE_TOKEN", "per-session")]);
        assert!(secret_env(&agent).is_empty(), "per-session tokens are not daemon secrets");
    }

    #[test]
    fn secret_env_file_renders_0600_under_runtime_dir() {
        assert_eq!(
            secret_env_path(Some(OsStr::new("/run/user/1000")), "ferryx-daemon"),
            Some(PathBuf::from("/run/user/1000/ferryx/ferryx-daemon.env"))
        );
        assert_eq!(secret_env_path(None, "ferryx-daemon"), None);
        assert_eq!(secret_env_path(Some(OsStr::new("")), "ferryx-daemon"), None);
        assert_eq!(
            secret_env_path(Some(OsStr::new("run/user/1000")), "ferryx-daemon"),
            None,
            "a relative runtime dir must not fall back to a persistent path"
        );

        let env = vars(&[
            ("FERRYX_DATA_DIR", "/home/u/.ferryx"),
            ("FERRYX_MACHINE_TOKEN", "a$b\"c\\d`e"),
        ]);
        let content = render_secret_env(&env).unwrap();
        assert_eq!(
            content,
            "# Managed by Ferryx: daemon secrets for supervisor restarts.\n\
             FERRYX_MACHINE_TOKEN=\"a\\$b\\\"c\\\\d\\`e\"\n"
        );
        assert!(!content.contains("FERRYX_DATA_DIR"), "{content}");

        let runtime = tempfile::tempdir().unwrap();
        let path = secret_env_path(Some(runtime.path().as_os_str()), "ferryx-daemon").unwrap();
        assert!(write_secret_env(Some(&content), &path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "secret env file mode {mode:o}");
        let dir_mode = fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "secret env dir mode {dir_mode:o}");
        let entries: Vec<OsString> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [OsString::from("ferryx-daemon.env")], "no temp file left behind");

        let without = vars(&[("FERRYX_DATA_DIR", "/home/u/.ferryx")]);
        assert_eq!(render_secret_env(&without), None);
        assert!(write_secret_env(render_secret_env(&without).as_deref(), &path).unwrap());
        assert!(!path.exists(), "a secret that went away must remove the file");
        assert!(!write_secret_env(None, &path).unwrap(), "removing a missing file is a no-op");
    }

    #[test]
    fn unchanged_secret_env_is_not_rewritten() {
        use std::os::unix::fs::MetadataExt as _;
        let runtime = tempfile::tempdir().unwrap();
        let path = secret_env_path(Some(runtime.path().as_os_str()), "ferryx-daemon").unwrap();
        let content = render_secret_env(&vars(&[("FERRYX_MACHINE_TOKEN", "mt-1")])).unwrap();
        assert!(write_secret_env(Some(&content), &path).unwrap());
        let inode = fs::metadata(&path).unwrap().ino();
        assert!(
            !write_secret_env(Some(&content), &path).unwrap(),
            "an unchanged secret env must not be rewritten"
        );
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode, "the file was replaced");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            write_secret_env(Some(&content), &path).unwrap(),
            "a widened mode must be repaired"
        );
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        let next = render_secret_env(&vars(&[("FERRYX_MACHINE_TOKEN", "mt-2")])).unwrap();
        assert!(write_secret_env(Some(&next), &path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), next);
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
