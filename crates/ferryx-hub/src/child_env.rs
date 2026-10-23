//! PATH for every process the hub spawns.
//!
//! The daemon that hosts the hub is started by launchd or the GUI, so its own PATH is the
//! minimal `/usr/bin:/bin:/usr/sbin:/sbin`. Inbox watchers (`imsg`, `zele`), cron scripts
//! (`bun`, `node`), the judge and button actions live in Homebrew, bun, or cargo directories.
//! omo-gateway got those from its launchd plist PATH plus `/opt/homebrew/bin:/usr/local/bin`
//! (`OMON_EXTRA_PATH` overrides the extra part). The hub resolves the user's login-shell PATH
//! once, then adds the same well-known directories, and every child gets that PATH. Programs
//! configured with an absolute path are run as given.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long the login shell may take to print its PATH before the fallback is used.
pub const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);

const MARKER: &str = "__FERRYX_HUB_PATH__=";

static CHILD_PATH: OnceLock<OsString> = OnceLock::new();

/// Resolves the child PATH now (blocking up to [`LOGIN_SHELL_TIMEOUT`]) and returns it.
/// Later calls return the first result.
pub fn init() -> &'static OsString {
    CHILD_PATH.get_or_init(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| OsString::from(default_shell()));
        let login = login_shell_path(Path::new(&shell), LOGIN_SHELL_TIMEOUT);
        if login.is_none() {
            tracing::warn!(shell = %Path::new(&shell).display(), "login shell PATH was not read; using the current PATH plus the well-known tool directories");
        }
        let extra = std::env::var_os("FERRYX_HUB_EXTRA_PATH")
            .or_else(|| std::env::var_os("OMON_EXTRA_PATH"))
            .or_else(|| std::env::var_os("EXTRA_PATH"));
        let current = std::env::var_os("PATH");
        let path = build_child_path(
            extra.as_deref(),
            login.as_deref(),
            current.as_deref(),
            home.as_deref(),
        );
        tracing::info!(path = %Path::new(&path).display(), from_login_shell = login.is_some(), "hub child PATH resolved");
        path
    })
}

/// PATH given to hub child processes.
pub fn child_path() -> &'static OsStr {
    init().as_os_str()
}

/// `tokio::process::Command` for `program` with the hub child PATH. A bare program name is
/// looked up in that PATH (std searches the PATH set on the command, not the daemon's).
pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.env("PATH", child_path());
    command
}

/// Blocking variant of [`command`].
pub fn std_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", child_path());
    command
}

/// First executable named `name` on the hub child PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(name, child_path())
}

pub fn which_in(name: &str, path: &OsStr) -> Option<PathBuf> {
    if name.contains('/') {
        let candidate = PathBuf::from(name);
        return is_executable(&candidate).then_some(candidate);
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn default_shell() -> &'static str {
    if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/sh"
    }
}

/// Well-known tool directories appended after the login and current PATH, in omo's order
/// (bun first, as in its plist), then cargo.
pub fn fallback_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".bun/bin"));
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    if let Some(home) = home {
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join(".local/bin"));
    }
    dirs.extend(
        ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .into_iter()
            .map(PathBuf::from),
    );
    dirs
}

/// `extra` (an explicit override), then the login-shell PATH, then the current PATH, then
/// [`fallback_dirs`]. Empty and repeated entries are dropped; the first occurrence wins.
pub fn build_child_path(
    extra: Option<&OsStr>,
    login: Option<&OsStr>,
    current: Option<&OsStr>,
    home: Option<&Path>,
) -> OsString {
    let mut entries: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && !entries.contains(&dir) {
            entries.push(dir);
        }
    };
    for list in [extra, login, current].into_iter().flatten() {
        std::env::split_paths(list).for_each(&mut push);
    }
    fallback_dirs(home).into_iter().for_each(&mut push);
    std::env::join_paths(entries).unwrap_or_default()
}

/// PATH printed by `shell -lc`, or `None` when the shell fails, times out, or prints nothing.
/// The value is read from a marked line so login banners on stdout are ignored.
pub fn login_shell_path(shell: &Path, timeout: Duration) -> Option<OsString> {
    let script = format!("printf '\\n{MARKER}%s\\n' \"$PATH\"");
    let mut child = Command::new(shell)
        .arg("-lc")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A login shell can leave a background child holding stdout; never block past the deadline.
    let text = loop {
        if reader.is_finished() {
            break reader.join().ok()?;
        }
        if Instant::now() >= deadline + Duration::from_millis(200) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status?.success() {
        return None;
    }
    parse_marked_path(&text)
}

fn parse_marked_path(text: &str) -> Option<OsString> {
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix(MARKER))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(OsString::from)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("ferryx-child-env-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn split(path: &OsStr) -> Vec<String> {
        std::env::split_paths(path)
            .map(|dir| dir.display().to_string())
            .collect()
    }

    #[test]
    fn minimal_launchd_path_gains_login_and_well_known_dirs_in_order() {
        let path = build_child_path(
            None,
            Some(OsStr::new("/login/bin:/usr/bin:/bin")),
            Some(OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin")),
            Some(Path::new("/Users/u")),
        );
        assert_eq!(
            split(&path),
            [
                "/login/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
                "/Users/u/.bun/bin",
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/Users/u/.cargo/bin",
                "/Users/u/.local/bin",
            ]
        );
    }

    #[test]
    fn without_login_shell_the_fallback_still_has_homebrew_bun_and_cargo() {
        let path = build_child_path(
            None,
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            Some(Path::new("/h")),
        );
        let dirs = split(&path);
        for dir in [
            "/h/.bun/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/h/.cargo/bin",
        ] {
            assert!(dirs.iter().any(|d| d == dir), "{dir} missing from {dirs:?}");
        }
        assert_eq!(&dirs[..2], ["/usr/bin", "/bin"]);
    }

    #[test]
    fn explicit_extra_path_comes_first_and_empty_entries_are_dropped() {
        let path = build_child_path(
            Some(OsStr::new("/extra::/opt/homebrew/bin")),
            Some(OsStr::new("")),
            None,
            None,
        );
        let dirs = split(&path);
        assert_eq!(&dirs[..2], ["/extra", "/opt/homebrew/bin"]);
        assert_eq!(dirs.iter().filter(|d| *d == "/opt/homebrew/bin").count(), 1);
        assert!(!dirs.iter().any(String::is_empty));
    }

    #[test]
    fn login_shell_path_ignores_banners_and_reads_the_marked_line() {
        let dir = TempDir::new();
        let shell = script(
            dir.path(),
            "fake-shell",
            "echo 'Welcome banner'\nPATH=/from/login:/usr/bin\nexport PATH\nshift\nexec /bin/sh -c \"$1\"",
        );
        let path = login_shell_path(&shell, Duration::from_secs(5)).unwrap();
        assert_eq!(path, OsString::from("/from/login:/usr/bin"));
    }

    #[test]
    fn a_hanging_or_failing_login_shell_falls_back_within_the_timeout() {
        let dir = TempDir::new();
        let hang = script(dir.path(), "hang", "exec sleep 30");
        let started = Instant::now();
        assert!(login_shell_path(&hang, Duration::from_millis(300)).is_none());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let fail = script(dir.path(), "fail", "exit 3");
        assert!(login_shell_path(&fail, Duration::from_secs(2)).is_none());
        assert!(
            login_shell_path(Path::new("/nonexistent/shell"), Duration::from_secs(1)).is_none()
        );
    }

    #[test]
    fn which_in_finds_executables_and_honors_absolute_paths() {
        let dir = TempDir::new();
        let tool = script(dir.path(), "imsg", "exit 0");
        std::fs::write(dir.path().join("plain"), "x").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), dir.path()]).unwrap();
        assert_eq!(which_in("imsg", &path), Some(tool.clone()));
        assert_eq!(which_in("plain", &path), None);
        assert_eq!(which_in(tool.to_str().unwrap(), OsStr::new("")), Some(tool));
    }

    #[tokio::test]
    async fn a_command_built_here_finds_a_program_absent_from_the_daemon_path() {
        let dir = TempDir::new();
        script(dir.path(), "ferryx-child-env-probe", "printf ok");
        let path = build_child_path(
            Some(dir.path().as_os_str()),
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            None,
        );
        let mut command = tokio::process::Command::new("ferryx-child-env-probe");
        command.env("PATH", &path);
        let output = command.output().await.unwrap();
        assert_eq!(output.stdout, b"ok");
    }
}
