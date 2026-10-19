//! Linux side of the daemon supervisor: the systemd user units that own the daemon's lifetime, and
//! the `sd_notify` messages the daemon sends to it.
//!
//! Each daemon generation (`daemon::runtime_stage`) gets its own unit
//! `<base>-<generation key>.service` whose `ExecStart` is that generation's staged executable,
//! never the app install. Starting is idempotent and never disturbs a running unit: an active unit
//! gets zero `systemctl` mutations, an idle one is written, reloaded and `start --no-block`ed.
//! Only the newest generation is enabled for login; older ones are disabled without `--now`, so
//! they keep running until their daemon exits on its own.
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

/// The unit of one daemon generation: `<base>-<key>`, validated like any unit name.
pub(crate) fn generation_unit_name(base: &str, key: &str) -> Result<String, String> {
    unit_name(Some(OsStr::new(&format!("{base}-{key}"))))
}

/// Whether `text` is a generation key `<version>-<8 lowercase hex>` (`runtime_stage::generation_key`).
/// The version must start with a digit and hold no `-`, so a QA base such as `ferryx-daemon-qa-1`
/// is never mistaken for a generation of `ferryx-daemon`.
pub(crate) fn is_generation_key(text: &str) -> bool {
    let Some((version, hash)) = text.rsplit_once('-') else {
        return false;
    };
    let hash_ok = hash.len() == 8
        && hash
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, 'a'..='f'));
    let version_ok = version.starts_with(|ch: char| ch.is_ascii_digit())
        && version.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.');
    hash_ok && version_ok
}

/// The generation units of `base` that have a unit file in `unit_dir`, sorted by name. The legacy
/// single unit `<base>.service` is not a generation and is not listed.
pub(crate) fn generation_units_in(unit_dir: &Path, base: &str) -> Vec<(String, PathBuf)> {
    let prefix = format!("{base}-");
    let Ok(entries) = fs::read_dir(unit_dir) else {
        return Vec::new();
    };
    let mut units: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name();
            let stem = file_name.to_str()?.strip_suffix(".service")?;
            let key = stem.strip_prefix(&prefix)?;
            is_generation_key(key).then(|| (stem.to_string(), entry.path()))
        })
        .collect();
    units.sort();
    units
}

/// The executable a rendered unit's `ExecStart=` runs, undoing the quoting of [`render_unit`].
pub(crate) fn unit_exec_start(content: &str) -> Option<PathBuf> {
    let value = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("ExecStart="))?;
    let mut chars = value.chars().peekable();
    let mut exe = String::new();
    if chars.peek() == Some(&'"') {
        chars.next();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => exe.push(chars.next()?),
                ch @ ('%' | '$') => {
                    if chars.peek() == Some(&ch) {
                        chars.next();
                    }
                    exe.push(ch);
                }
                ch => exe.push(ch),
            }
        }
    } else {
        exe = chars.take_while(|ch| !ch.is_whitespace()).collect();
    }
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// Executables referenced by every unit file in `unit_dir` (any name, so a QA unit and the legacy
/// unit count too). Garbage collection of staged runtimes never removes a generation one of these
/// points into.
pub(crate) fn registered_job_executables(unit_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(unit_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension() != Some(OsStr::new("service")) {
                return None;
            }
            unit_exec_start(&fs::read_to_string(path).ok()?)
        })
        .collect()
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
const FORWARDED_FERRYX_KEYS: [&str; 9] = [
    "FERRYX_ALLOW_INSECURE_DIRECT",
    "FERRYX_ALLOW_INSECURE_LAN",
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

/// `systemctl --user`, injectable so the start and registration rules are testable without a
/// user manager.
pub(crate) trait Systemctl {
    fn run(&self, args: &[&str]) -> Result<String, String>;
}

/// The real `systemctl --user`.
#[cfg(target_os = "linux")]
pub(crate) struct UserSystemctl;

#[cfg(target_os = "linux")]
impl Systemctl for UserSystemctl {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        systemctl(args)
    }
}

/// Whether the unit is anything but idle. Only `inactive` and `failed` count as idle: an active,
/// activating, reloading or deactivating unit, or any state systemd adds later, is left alone. A
/// failed query is an error, never "idle".
fn unit_is_busy(systemctl: &dyn Systemctl, name: &str) -> Result<bool, String> {
    let unit = format!("{name}.service");
    let state = systemctl.run(&["show", "-p", "ActiveState", "--value", &unit])?;
    Ok(!matches!(state.trim(), "inactive" | "failed"))
}

/// Starts generation unit `name` when it is idle: writes `content` to `path`, reloads systemd when
/// the file changed, then `start --no-block`. A busy unit gets no `systemctl` mutation and no
/// rewrite. Never `enable --now`, `restart` or `stop`. Returns whether a start was issued.
pub(crate) fn ensure_generation_with(
    systemctl: &dyn Systemctl,
    name: &str,
    path: &Path,
    content: &str,
) -> Result<bool, String> {
    if unit_is_busy(systemctl, name)? {
        return Ok(false);
    }
    if write_unit(content, path)? {
        systemctl.run(&["daemon-reload"])?;
    }
    systemctl.run(&["start", "--no-block", &format!("{name}.service")])?;
    Ok(true)
}

/// Registers generation unit `newest` for the next login and drops login autostart from every
/// older generation of `base` in the same directory. Only `enable` and `disable` without `--now`:
/// no unit is started, stopped or restarted. Returns whether the newest unit file changed.
pub(crate) fn register_generation_with(
    systemctl: &dyn Systemctl,
    base: &str,
    newest: &str,
    path: &Path,
    content: &str,
) -> Result<bool, String> {
    let rewritten = write_unit(content, path)?;
    if rewritten {
        systemctl.run(&["daemon-reload"])?;
    }
    systemctl.run(&["enable", &format!("{newest}.service")])?;
    let unit_dir = path.parent().unwrap_or(Path::new(""));
    for (older, _) in generation_units_in(unit_dir, base) {
        if older == newest {
            continue;
        }
        if let Err(error) = systemctl.run(&["disable", &format!("{older}.service")]) {
            tracing::warn!(%error, unit = %older, "Could not drop login autostart from an older daemon generation");
        }
    }
    Ok(rewritten)
}

/// [`ensure_generation_with`] for the real user manager, keeping the unit's secret env file in sync
/// first.
#[cfg(target_os = "linux")]
pub(crate) fn ensure(name: &str, path: &Path, executable: &Path) -> Result<bool, String> {
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let content = render_unit(executable, &vars)?;
    // Before any start below, so a freshly started daemon already sees the secrets. A failure only
    // costs the token after a restart, so it does not stop the unit from supervising the daemon.
    if let Err(error) = sync_secret_env(name, &vars) {
        tracing::warn!(%error, unit = %name, "Could not update the daemon secret env file");
    }
    ensure_generation_with(&UserSystemctl, name, path, &content)
}

/// Autostart opt-out: `disable` (never `--now`) on every generation unit of `base` in `unit_dir`
/// and on the legacy base unit `<base>.service` when its file exists. No unit is stopped, no unit
/// file is removed, and nothing is reloaded, so a running daemon keeps its process, PTYs and
/// definition, and an opted-out GUI can still `start` the newest generation. Every unit is tried
/// even when one fails; the failures are reported together.
pub(crate) fn remove_login_autostart_with(
    systemctl: &dyn Systemctl,
    unit_dir: &Path,
    base: &str,
) -> Result<(), String> {
    let mut units: Vec<String> = generation_units_in(unit_dir, base)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    if unit_dir.join(format!("{base}.service")).exists() {
        units.push(base.to_string());
    }
    let failures: Vec<String> = units
        .iter()
        .filter_map(|name| systemctl.run(&["disable", &format!("{name}.service")]).err())
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Starts the unit again; a unit stopped by its start limit is reset first.
#[cfg(target_os = "linux")]
pub(crate) fn start(name: &str) -> Result<(), String> {
    let unit = format!("{name}.service");
    let _ = systemctl(&["reset-failed", &unit]);
    systemctl(&["start", "--no-block", &unit]).map(|_| ())
}

/// [`register_generation_with`] for the real user manager. Never `--now`, `start`, or `restart`:
/// a running daemon keeps its process and PTYs.
#[cfg(target_os = "linux")]
pub(crate) fn register_for_login(
    base: &str,
    name: &str,
    path: &Path,
    executable: &Path,
) -> Result<bool, String> {
    let vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let content = render_unit(executable, &vars)?;
    register_generation_with(&UserSystemctl, base, name, path, &content)
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

/// Migrates the legacy single unit `name` (`ferryx-daemon`, or the QA base) at `path` for the
/// owner admission classified under its lock (todo 14).
///
/// Enablement is read from the `default.target.wants` link next to the unit file, so a unit that
/// is already disabled costs no `systemctl` call. An enabled legacy unit is `disable`d without
/// `--now`, so the next login starts only the newest generation and a running legacy daemon keeps
/// its process and PTYs. With an Alive or Unknown owner that is all. With an Absent owner the unit
/// file is also removed and systemd reloaded, but only when the unit itself is idle (`inactive` or
/// `failed`); a busy unit, or one whose state cannot be read (an error), keeps its file. Never
/// `stop`, `restart`, `kill` or `--now`. A second run after the migration issues nothing.
/// Returns whether anything changed.
pub(crate) fn migrate_legacy_unit_with(
    systemctl: &dyn Systemctl,
    name: &str,
    path: &Path,
    owner_absent: bool,
) -> Result<bool, String> {
    let unit = format!("{name}.service");
    let wants = path
        .parent()
        .unwrap_or(Path::new(""))
        .join("default.target.wants")
        .join(&unit);
    let present = |path: &Path| match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Failed to inspect {}: {error}", path.display())),
    };
    let has_file = present(path)?;
    let enabled = present(&wants)?;
    let mut changed = false;
    if enabled {
        systemctl.run(&["disable", &unit])?;
        changed = true;
    }
    if !owner_absent || !has_file {
        return Ok(changed);
    }
    if unit_is_busy(systemctl, name)? {
        tracing::info!(unit = %name, "The legacy daemon unit is not idle; it keeps running and only lost login autostart");
        return Ok(changed);
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(changed),
        Err(error) => return Err(format!("Failed to remove {}: {error}", path.display())),
    }
    systemctl.run(&["daemon-reload"])?;
    Ok(true)
}

/// [`migrate_legacy_unit_with`] for the real user manager; a removed unit also loses its secret
/// env file.
#[cfg(target_os = "linux")]
pub(crate) fn migrate_legacy_unit(name: &str, path: &Path, owner_absent: bool) -> Result<bool, String> {
    let changed = migrate_legacy_unit_with(&UserSystemctl, name, path, owner_absent)?;
    if owner_absent && !path.exists() {
        remove_secret_env(name)?;
    }
    Ok(changed)
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
    fn allow_insecure_direct_and_lan_are_forwarded_only_when_set() {
        let exe = Path::new("/opt/ferryx/ferryx");
        let env = vars(&[
            ("FERRYX_ALLOW_INSECURE_LAN", "1"),
            ("FERRYX_ALLOW_INSECURE_DIRECT", "1"),
        ]);
        let keys: Vec<String> = forwarded_env(&env).into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["FERRYX_ALLOW_INSECURE_DIRECT", "FERRYX_ALLOW_INSECURE_LAN"]);
        let unit = render_unit(exe, &env).unwrap();
        let environment: Vec<&str> = unit
            .lines()
            .filter(|line| line.starts_with("Environment="))
            .collect();
        assert_eq!(
            environment,
            [
                "Environment=\"FERRYX_ALLOW_INSECURE_DIRECT=1\"",
                "Environment=\"FERRYX_ALLOW_INSECURE_LAN=1\"",
            ]
        );

        let unset = render_unit(exe, &vars(&[("FERRYX_DATA_DIR", "/home/u/.ferryx")])).unwrap();
        assert!(!unset.contains("FERRYX_ALLOW_INSECURE_DIRECT"), "{unset}");
        assert!(!unset.contains("FERRYX_ALLOW_INSECURE_LAN"), "{unset}");
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

    /// Records every `systemctl --user` call; `show` answers from `states` (a missing unit is an
    /// error, like a user manager that cannot be queried).
    struct FakeSystemctl {
        states: HashMap<String, &'static str>,
        calls: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl FakeSystemctl {
        fn with_states(states: &[(&str, &'static str)]) -> Self {
            Self {
                states: states
                    .iter()
                    .map(|(unit, state)| (format!("{unit}.service"), *state))
                    .collect(),
                calls: Default::default(),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }

        /// Every call that is not a read-only `show`.
        fn mutations(&self) -> Vec<Vec<String>> {
            self.calls()
                .into_iter()
                .filter(|call| call.first().map(String::as_str) != Some("show"))
                .collect()
        }
    }

    impl Systemctl for FakeSystemctl {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| arg.to_string()).collect());
            if args.first() == Some(&"show") {
                let unit = args.last().unwrap();
                return self
                    .states
                    .get(*unit)
                    .map(|state| state.to_string())
                    .ok_or_else(|| format!("no state for {unit}"));
            }
            Ok(String::new())
        }
    }

    const KEY: &str = "2026.1013.2-0a1b2c3d";
    const OLDER_KEY: &str = "2026.1012.1-ffee0011";
    const STAGED: &str = "/home/u/.local/share/ferryx/daemon-runtime/2026.1013.2-0a1b2c3d/ferryx";

    fn call(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn generation_unit_runs_the_staged_executable() {
        let name = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        assert_eq!(name, "ferryx-daemon-2026.1013.2-0a1b2c3d");
        assert_eq!(
            generation_unit_name("ferryx-daemon-qa", KEY).unwrap(),
            "ferryx-daemon-qa-2026.1013.2-0a1b2c3d",
            "a QA base keeps its own prefix"
        );
        let unit = render(STAGED);
        assert!(unit.contains(&format!("ExecStart=\"{STAGED}\" --daemon\n")), "{unit}");
        assert!(!unit.contains(".AppImage"), "{unit}");
        let lines = service_lines(&unit);
        assert!(lines.contains(&"KillMode=process"), "{unit}");
        assert!(lines.contains(&"Restart=on-failure"), "{unit}");
        assert!(lines.contains(&"WantedBy=default.target"), "{unit}");
        assert_eq!(unit_exec_start(&unit), Some(PathBuf::from(STAGED)));
        assert_eq!(
            secret_env_path(Some(OsStr::new("/run/user/1000")), &name),
            Some(PathBuf::from(
                "/run/user/1000/ferryx/ferryx-daemon-2026.1013.2-0a1b2c3d.env"
            )),
            "each generation unit keeps its own secret env file"
        );
    }

    #[test]
    fn running_generation_gets_zero_systemctl_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let name = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let path = dir.path().join(format!("{name}.service"));
        for state in ["active", "activating", "reloading", "deactivating", "refreshing"] {
            let systemctl = FakeSystemctl::with_states(&[(&name, state)]);
            assert_eq!(
                ensure_generation_with(&systemctl, &name, &path, &render(STAGED)),
                Ok(false),
                "{state}"
            );
            assert_eq!(systemctl.mutations(), Vec::<Vec<String>>::new(), "{state}");
            assert!(!path.exists(), "a busy unit's file must not be rewritten ({state})");
        }
    }

    #[test]
    fn unqueryable_generation_is_an_error_without_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let name = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let path = dir.path().join(format!("{name}.service"));
        let systemctl = FakeSystemctl::with_states(&[]);
        assert!(ensure_generation_with(&systemctl, &name, &path, &render(STAGED)).is_err());
        assert_eq!(systemctl.mutations(), Vec::<Vec<String>>::new());
        assert!(!path.exists());
    }

    #[test]
    fn idle_generation_is_written_reloaded_and_started_without_enable_now() {
        let dir = tempfile::tempdir().unwrap();
        let name = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let path = dir.path().join(format!("{name}.service"));
        let content = render(STAGED);
        for state in ["inactive", "failed"] {
            let _ = fs::remove_file(&path);
            let systemctl = FakeSystemctl::with_states(&[(&name, state)]);
            assert_eq!(ensure_generation_with(&systemctl, &name, &path, &content), Ok(true));
            assert_eq!(
                systemctl.mutations(),
                [
                    call(&["daemon-reload"]),
                    call(&["start", "--no-block", &format!("{name}.service")]),
                ],
                "{state}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        // An unchanged unit file needs no daemon-reload.
        let systemctl = FakeSystemctl::with_states(&[(&name, "inactive")]);
        assert_eq!(ensure_generation_with(&systemctl, &name, &path, &content), Ok(true));
        assert_eq!(
            systemctl.mutations(),
            [call(&["start", "--no-block", &format!("{name}.service")])]
        );
        for call in systemctl.calls() {
            for forbidden in ["--now", "restart", "stop", "enable", "disable", "kill"] {
                assert!(!call.iter().any(|arg| arg == forbidden), "{call:?}");
            }
        }
    }

    #[test]
    fn older_generation_gets_disable_without_now() {
        let dir = tempfile::tempdir().unwrap();
        let newest = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let older = generation_unit_name(DEFAULT_UNIT_NAME, OLDER_KEY).unwrap();
        let older_path = dir.path().join(format!("{older}.service"));
        let older_unit = render("/home/u/.local/share/ferryx/daemon-runtime/2026.1012.1-ffee0011/ferryx");
        fs::write(&older_path, &older_unit).unwrap();
        // Neither the legacy single unit nor a QA unit sharing the prefix is a generation.
        fs::write(dir.path().join("ferryx-daemon.service"), render("/opt/ferryx/ferryx")).unwrap();
        fs::write(dir.path().join("ferryx-daemon-qa-1.service"), render("/opt/qa/ferryx")).unwrap();

        let systemctl = FakeSystemctl::with_states(&[(&older, "active")]);
        let path = dir.path().join(format!("{newest}.service"));
        assert_eq!(
            register_generation_with(&systemctl, DEFAULT_UNIT_NAME, &newest, &path, &render(STAGED)),
            Ok(true)
        );
        assert_eq!(
            systemctl.calls(),
            [
                call(&["daemon-reload"]),
                call(&["enable", &format!("{newest}.service")]),
                call(&["disable", &format!("{older}.service")]),
            ],
            "registration only enables the newest and disables older generations"
        );
        assert_eq!(
            fs::read_to_string(&older_path).unwrap(),
            older_unit,
            "the older generation's unit file stays, so its running daemon keeps its definition"
        );
    }

    /// Opt-out disables every generation and the legacy unit, never with `--now` and never with
    /// `stop`/`restart`/`kill`, and leaves every unit file in place (a running daemon keeps its
    /// definition). A unit with no file (no legacy unit) is not touched.
    #[test]
    fn opt_out_disables_every_generation_and_the_legacy_unit_without_now() {
        let dir = tempfile::tempdir().unwrap();
        let newest = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let older = generation_unit_name(DEFAULT_UNIT_NAME, OLDER_KEY).unwrap();
        let newest_unit = render(STAGED);
        let older_unit = render("/home/u/.local/share/ferryx/daemon-runtime/2026.1012.1-ffee0011/ferryx");
        let legacy_unit = render("/opt/ferryx/ferryx");
        fs::write(dir.path().join(format!("{newest}.service")), &newest_unit).unwrap();
        fs::write(dir.path().join(format!("{older}.service")), &older_unit).unwrap();
        fs::write(dir.path().join("ferryx-daemon.service"), &legacy_unit).unwrap();
        // Another base's unit sharing the prefix is not ours.
        fs::write(dir.path().join("ferryx-daemon-qa-1.service"), render("/opt/qa/ferryx")).unwrap();

        let systemctl = FakeSystemctl::with_states(&[(&newest, "active"), (&older, "active")]);
        assert_eq!(remove_login_autostart_with(&systemctl, dir.path(), DEFAULT_UNIT_NAME), Ok(()));
        let mut calls = systemctl.calls();
        calls.sort();
        let mut expected = vec![
            call(&["disable", &format!("{newest}.service")]),
            call(&["disable", &format!("{older}.service")]),
            call(&["disable", "ferryx-daemon.service"]),
        ];
        expected.sort();
        assert_eq!(calls, expected);
        for call in systemctl.calls() {
            for forbidden in ["--now", "stop", "restart", "kill", "daemon-reload", "start"] {
                assert!(!call.iter().any(|arg| arg == forbidden), "{call:?}");
            }
        }
        assert_eq!(fs::read_to_string(dir.path().join(format!("{newest}.service"))).unwrap(), newest_unit);
        assert_eq!(fs::read_to_string(dir.path().join(format!("{older}.service"))).unwrap(), older_unit);
        assert_eq!(fs::read_to_string(dir.path().join("ferryx-daemon.service")).unwrap(), legacy_unit);

        // Without a legacy unit file, only the generations are disabled.
        fs::remove_file(dir.path().join("ferryx-daemon.service")).unwrap();
        let systemctl = FakeSystemctl::with_states(&[]);
        remove_login_autostart_with(&systemctl, dir.path(), DEFAULT_UNIT_NAME).unwrap();
        assert_eq!(systemctl.calls().len(), 2, "{:?}", systemctl.calls());
    }

    /// After opt-out the GUI still starts an idle newest generation with `start` through the user
    /// manager, never `enable`, so the opt-out survives the start.
    #[test]
    fn opted_out_launch_starts_the_idle_newest_unit_without_enabling_it() {
        let dir = tempfile::tempdir().unwrap();
        let name = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let path = dir.path().join(format!("{name}.service"));
        let systemctl = FakeSystemctl::with_states(&[(&name, "inactive")]);
        remove_login_autostart_with(&systemctl, dir.path(), DEFAULT_UNIT_NAME).unwrap();
        assert_eq!(ensure_generation_with(&systemctl, &name, &path, &render(STAGED)), Ok(true));
        assert_eq!(
            systemctl.mutations(),
            [call(&["daemon-reload"]), call(&["start", "--no-block", &format!("{name}.service")])]
        );
        // Opt-out after the start disables it again without stopping it.
        let systemctl = FakeSystemctl::with_states(&[(&name, "active")]);
        remove_login_autostart_with(&systemctl, dir.path(), DEFAULT_UNIT_NAME).unwrap();
        assert_eq!(systemctl.calls(), [call(&["disable", &format!("{name}.service")])]);
    }

    #[test]
    fn opt_out_tries_every_unit_and_reports_each_failure() {
        struct Failing(std::cell::RefCell<Vec<Vec<String>>>);
        impl Systemctl for Failing {
            fn run(&self, args: &[&str]) -> Result<String, String> {
                self.0.borrow_mut().push(call(args));
                Err(format!("refused {}", args[1]))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let newest = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let older = generation_unit_name(DEFAULT_UNIT_NAME, OLDER_KEY).unwrap();
        fs::write(dir.path().join(format!("{newest}.service")), render(STAGED)).unwrap();
        fs::write(dir.path().join(format!("{older}.service")), render(STAGED)).unwrap();
        let systemctl = Failing(Default::default());
        let error = remove_login_autostart_with(&systemctl, dir.path(), DEFAULT_UNIT_NAME).unwrap_err();
        assert_eq!(systemctl.0.borrow().len(), 2, "a failure must not skip the remaining units");
        assert!(error.contains(&newest) && error.contains(&older), "{error}");
    }

    #[test]
    fn generation_units_and_registered_executables_are_discovered() {
        assert!(is_generation_key(KEY));
        assert!(is_generation_key("2026.1013.2-00000000"));
        assert!(!is_generation_key("qa-1"), "a QA suffix is not a generation key");
        assert!(!is_generation_key("2026.1013.2-0A1B2C3D"), "the hash is lowercase hex");
        assert!(!is_generation_key("2026.1013.2-0a1b2c3"));
        assert!(!is_generation_key("v2026-0a1b2c3d"));

        let dir = tempfile::tempdir().unwrap();
        let newest = generation_unit_name(DEFAULT_UNIT_NAME, KEY).unwrap();
        let older = generation_unit_name(DEFAULT_UNIT_NAME, OLDER_KEY).unwrap();
        fs::write(dir.path().join(format!("{newest}.service")), render(STAGED)).unwrap();
        fs::write(dir.path().join(format!("{older}.service")), render("/data/100%/$x/fe\"r")).unwrap();
        fs::write(dir.path().join("ferryx-daemon.service"), render("/opt/ferryx/ferryx")).unwrap();
        fs::write(dir.path().join("notes.txt"), "ExecStart=/not/a/unit").unwrap();

        let names: Vec<String> = generation_units_in(dir.path(), DEFAULT_UNIT_NAME)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let mut expected = vec![newest.clone(), older.clone()];
        expected.sort();
        assert_eq!(names, expected);

        let mut executables = registered_job_executables(dir.path());
        executables.sort();
        let mut expected = vec![
            PathBuf::from("/data/100%/$x/fe\"r"),
            PathBuf::from(STAGED),
            PathBuf::from("/opt/ferryx/ferryx"),
        ];
        expected.sort();
        assert_eq!(
            executables, expected,
            "every unit's ExecStart (generation, legacy) protects its runtime from garbage collection"
        );
        assert!(registered_job_executables(&dir.path().join("missing")).is_empty());
    }

    /// The legacy single unit as installed before per-generation units: `ferryx-daemon.service`,
    /// enabled through its `default.target.wants` link.
    fn install_legacy_unit(unit_dir: &Path) -> (PathBuf, PathBuf) {
        let path = unit_dir.join(format!("{DEFAULT_UNIT_NAME}.service"));
        fs::write(&path, render("/opt/ferryx/ferryx")).unwrap();
        let wants_dir = unit_dir.join("default.target.wants");
        fs::create_dir_all(&wants_dir).unwrap();
        let wants = wants_dir.join(format!("{DEFAULT_UNIT_NAME}.service"));
        std::os::unix::fs::symlink(&path, &wants).unwrap();
        (path, wants)
    }

    /// Commands that could end a running legacy daemon.
    fn assert_never_stops(calls: &[Vec<String>]) {
        for call in calls {
            for forbidden in ["--now", "stop", "restart", "kill", "try-restart", "reload-or-restart"] {
                assert!(!call.iter().any(|arg| arg == forbidden), "forbidden systemctl call {call:?}");
            }
        }
    }

    /// Todo 14: an Alive or Unknown legacy owner keeps its running unit and unit file; the unit only
    /// loses login autostart (`disable` without `--now`), and a second run issues no call.
    #[test]
    fn legacy_unit_with_alive_or_unknown_owner_is_only_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let (path, wants) = install_legacy_unit(dir.path());
        let unit = format!("{DEFAULT_UNIT_NAME}.service");
        let systemctl = FakeSystemctl::with_states(&[(DEFAULT_UNIT_NAME, "active")]);
        assert_eq!(migrate_legacy_unit_with(&systemctl, DEFAULT_UNIT_NAME, &path, false), Ok(true));
        assert_eq!(systemctl.calls(), [call(&["disable", &unit])]);
        assert_never_stops(&systemctl.calls());
        assert!(path.exists(), "the running legacy unit keeps its file");

        // The real `disable` drops the wants link; the next run sees a disabled unit.
        fs::remove_file(&wants).unwrap();
        let again = FakeSystemctl::with_states(&[(DEFAULT_UNIT_NAME, "active")]);
        assert_eq!(migrate_legacy_unit_with(&again, DEFAULT_UNIT_NAME, &path, false), Ok(false));
        assert!(again.calls().is_empty(), "the second run issues nothing: {:?}", again.calls());
    }

    /// Todo 14: an Absent legacy owner's idle unit is disabled and removed once (no stop), and the
    /// second run issues no call.
    #[test]
    fn legacy_unit_with_absent_owner_is_removed_once() {
        for state in ["inactive", "failed"] {
            let dir = tempfile::tempdir().unwrap();
            let (path, wants) = install_legacy_unit(dir.path());
            let unit = format!("{DEFAULT_UNIT_NAME}.service");
            let systemctl = FakeSystemctl::with_states(&[(DEFAULT_UNIT_NAME, state)]);
            assert_eq!(migrate_legacy_unit_with(&systemctl, DEFAULT_UNIT_NAME, &path, true), Ok(true));
            assert_eq!(
                systemctl.mutations(),
                [call(&["disable", &unit]), call(&["daemon-reload"])],
                "{state}"
            );
            assert_never_stops(&systemctl.calls());
            assert!(!path.exists(), "{state}: the idle legacy unit is removed");

            fs::remove_file(&wants).unwrap();
            let again = FakeSystemctl::with_states(&[]);
            assert_eq!(migrate_legacy_unit_with(&again, DEFAULT_UNIT_NAME, &path, true), Ok(false));
            assert!(again.calls().is_empty(), "the second run issues nothing: {:?}", again.calls());
        }
    }

    /// A legacy unit that is still busy, or whose state cannot be read, is never removed or stopped
    /// even when admission saw no owner; it only loses login autostart.
    #[test]
    fn busy_or_unreadable_legacy_unit_is_kept_for_an_absent_owner() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = install_legacy_unit(dir.path());
        let systemctl = FakeSystemctl::with_states(&[(DEFAULT_UNIT_NAME, "activating")]);
        assert_eq!(migrate_legacy_unit_with(&systemctl, DEFAULT_UNIT_NAME, &path, true), Ok(true));
        assert_eq!(
            systemctl.mutations(),
            [call(&["disable", &format!("{DEFAULT_UNIT_NAME}.service")])]
        );
        assert!(path.exists());

        let unreadable = FakeSystemctl::with_states(&[]);
        assert!(migrate_legacy_unit_with(&unreadable, DEFAULT_UNIT_NAME, &path, true).is_err());
        assert_never_stops(&unreadable.calls());
        assert!(path.exists(), "an unreadable unit state is never treated as idle");
    }
}
