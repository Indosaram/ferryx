//! Launch configuration for per-turn mhc RPC children.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Resource dir name of the bundled mhc (`scripts/fetch-mhc.mjs` fills it per target triple:
/// `mhc/<triple>/mhc/` is the extracted mhc release, `mhc/<triple>/bun/` the bun it needs).
pub const BUNDLED_MHC_DIR: &str = "mhc";

/// Rust target triple of this build, as `scripts/fetch-mhc.mjs` names the bundled payloads.
pub fn host_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

fn exe_file(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The bundled `mhc` resource dir (one holding `<host triple>/mhc/mhc[.exe]`) next to
/// `executable`: an app install (Windows `<install dir>/mhc`, macOS `Contents/Resources/mhc`,
/// Linux deb or AppImage `usr/lib/Ferryx/mhc`), a dev build (`target/<profile>/mhc`, copied by
/// tauri-build), or a staged daemon generation (`<key>/mhc`, which is `../../../mhc` from a macOS
/// staged bundle executable).
pub fn bundled_mhc_dir_near(executable: &Path) -> Option<PathBuf> {
    let exe_dir = executable.parent()?;
    [
        exe_dir.join(BUNDLED_MHC_DIR),
        exe_dir.join("../Resources").join(BUNDLED_MHC_DIR),
        exe_dir.join("../lib/Ferryx").join(BUNDLED_MHC_DIR),
        exe_dir.join("../../..").join(BUNDLED_MHC_DIR),
    ]
    .into_iter()
    .find(|dir| bundled_mhc_in(dir).is_some())
}

/// `<dir>/<host triple>/mhc/mhc[.exe]` when it exists. The release's `mhc` binary is run directly
/// from its extracted folder (siblings such as `skills/` are found next to it); `mhc-launcher`
/// needs the installer's `current.json` layout and is never used here.
pub fn bundled_mhc_in(dir: &Path) -> Option<PathBuf> {
    let binary = dir.join(host_triple()).join("mhc").join(exe_file("mhc"));
    binary.is_file().then_some(binary)
}

/// mhc binary for hub turns, in order: `FERRYX_HUB_MHC_BIN`, the mhc bundled next to the running
/// executable, else bare `mhc` resolved on the hub child PATH.
pub fn mhc_bin() -> String {
    resolve_mhc_bin(
        crate::hub_env::var("FERRYX_HUB_MHC_BIN").ok(),
        std::env::current_exe().ok().as_deref(),
    )
}

/// [`mhc_bin`] with its inputs explicit.
pub fn resolve_mhc_bin(override_bin: Option<String>, executable: Option<&Path>) -> String {
    if let Some(bin) = override_bin
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return bin;
    }
    executable
        .and_then(bundled_mhc_dir_near)
        .and_then(|dir| bundled_mhc_in(&dir))
        .map(|binary| binary.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mhc".into())
}

/// The bundled bun dir that belongs to a bundled mhc binary (`<triple>/mhc/mhc` ->
/// `<triple>/bun`), when `binary` is one and that dir holds `bun[.exe]`. mhc's eval kernel (and
/// bash, which runs through it) resolves `bun` from PATH.
pub fn bundled_bun_dir(binary: &Path) -> Option<PathBuf> {
    let release_dir = binary.parent()?;
    if release_dir.file_name()? != OsStr::new("mhc") {
        return None;
    }
    let bun_dir = release_dir.parent()?.join("bun");
    bun_dir.join(exe_file("bun")).is_file().then_some(bun_dir)
}

/// PATH for an mhc child: `prefix` dirs, then the bundled bun dir when `binary` is the bundled
/// mhc (so its eval kernel never picks an older bun from the user's PATH), then `base`.
pub fn mhc_child_path(binary: &Path, prefix: Vec<PathBuf>, base: &OsStr) -> std::result::Result<OsString, std::env::JoinPathsError> {
    let mut paths = prefix;
    paths.extend(bundled_bun_dir(binary));
    paths.extend(std::env::split_paths(base));
    std::env::join_paths(paths)
}

/// Reuse the existing OmO configuration and credentials unless explicitly overridden.
pub fn agent_dir() -> Option<PathBuf> {
    crate::hub_env::var_os("MAHO_CODING_AGENT_DIR")
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let home =
                crate::hub_env::var_os("HOME").or_else(|| crate::hub_env::var_os("USERPROFILE"))?;
            Some(Path::new(&home).join(".omo").join("agent"))
        })
}

/// A single stdio RPC child. The transport appends durable session/resume arguments.
pub fn rpc_command() -> Vec<String> {
    vec![mhc_bin(), "--mode".into(), "rpc".into()]
}

/// Command for the supervisor pane. A terminal pane takes a single program and no env, so when
/// [`mhc_bin`] is the bundled mhc with a bundled bun, this is a launcher written under
/// `<state dir>/pane-bin` that puts that bun first on PATH and execs the bundled mhc. Otherwise
/// it is [`mhc_bin`] itself.
pub fn supervisor_pane_command() -> Vec<String> {
    let binary = mhc_bin();
    let Some(state_dir) = state_dir_from_home() else {
        return vec![binary];
    };
    match install_pane_launcher(&state_dir, Path::new(&binary)) {
        Ok(Some(launcher)) => vec![launcher.to_string_lossy().into_owned()],
        Ok(None) => vec![binary],
        Err(error) => {
            tracing::warn!(%error, "supervisor pane launcher was not written; the pane runs mhc without the bundled bun");
            vec![binary]
        }
    }
}

/// Writes `<state_dir>/pane-bin/mhc-pane` (`mhc-pane.cmd` on Windows) when `binary` is a bundled
/// mhc with a bundled bun, and returns its path; `None` when there is no bundled bun.
pub fn install_pane_launcher(state_dir: &Path, binary: &Path) -> Result<Option<PathBuf>> {
    let Some(bun_dir) = bundled_bun_dir(binary) else {
        return Ok(None);
    };
    let dir = state_dir.join("pane-bin");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(if cfg!(windows) { "mhc-pane.cmd" } else { "mhc-pane" });
    let script = pane_launcher_script(binary, &bun_dir, cfg!(windows));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(script.as_str()) {
        std::fs::write(&path, &script)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(Some(path))
}

/// Launcher text: POSIX sh, or a Windows batch file (never a POSIX `env` wrapper on Windows).
pub fn pane_launcher_script(binary: &Path, bun_dir: &Path, windows: bool) -> String {
    if windows {
        format!(
            "@echo off\r\nrem Installed by the ferryx hub: the bundled mhc with its bundled bun first on PATH.\r\nset \"PATH={};%PATH%\"\r\n\"{}\" %*\r\n",
            bun_dir.display(),
            binary.display()
        )
    } else {
        let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"));
        format!(
            "#!/bin/sh\n# Installed by the ferryx hub: the bundled mhc with its bundled bun first on PATH.\nPATH={}:\"$PATH\"\nexport PATH\nexec {} \"$@\"\n",
            quote(bun_dir),
            quote(binary)
        )
    }
}

pub fn state_dir_from_home() -> Option<PathBuf> {
    let home = crate::hub_env::var_os("HOME").or_else(|| crate::hub_env::var_os("USERPROFILE"))?;
    Some(Path::new(&home).join(".ferryx").join("hub"))
}

/// `launchctl` wrapper for RPC children that refuses to restart the retired gateway.
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

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ferryx-hub-{tag}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"bin").unwrap();
    }

    /// Writes a fetched payload (`<mhc dir>/<triple>/{mhc,bun}`) and returns the mhc binary.
    fn write_payload(mhc_dir: &Path, with_bun: bool) -> PathBuf {
        let triple = mhc_dir.join(host_triple());
        let binary = triple.join("mhc").join(exe_file("mhc"));
        touch(&binary);
        touch(&triple.join("mhc").join("skills").join("SKILL.md"));
        if with_bun {
            touch(&triple.join("bun").join(exe_file("bun")));
        }
        binary
    }

    #[test]
    fn mhc_resolution_prefers_the_override_then_the_bundle_then_path() {
        let temp = TempDir::new("mhc-order");
        let exe = temp.0.join("Ferryx.app/Contents/MacOS/ferryx");
        touch(&exe);
        assert_eq!(resolve_mhc_bin(None, Some(&exe)), "mhc", "no bundle: bare mhc on PATH");
        assert_eq!(resolve_mhc_bin(Some("  ".into()), Some(&exe)), "mhc", "a blank override is ignored");

        let bundled = write_payload(&temp.0.join("Ferryx.app/Contents/Resources/mhc"), true);
        let resolved = PathBuf::from(resolve_mhc_bin(None, Some(&exe)));
        assert_eq!(
            std::fs::canonicalize(resolved).unwrap(),
            std::fs::canonicalize(&bundled).unwrap(),
            "bundled mhc beats PATH"
        );
        assert_eq!(
            resolve_mhc_bin(Some(" /opt/mhc/bin/mhc ".into()), Some(&exe)),
            "/opt/mhc/bin/mhc",
            "FERRYX_HUB_MHC_BIN beats the bundle"
        );
        assert_eq!(resolve_mhc_bin(None, None), "mhc");
    }

    #[test]
    fn bundled_mhc_is_found_next_to_every_install_and_staged_layout() {
        let temp = TempDir::new("mhc-layouts");
        let layouts = [
            ("win/ferryx.exe", "win/mhc"),
            ("Ferryx.app/Contents/MacOS/ferryx", "Ferryx.app/Contents/Resources/mhc"),
            ("usr/bin/ferryx", "usr/lib/Ferryx/mhc"),
            ("target/debug/ferryx", "target/debug/mhc"),
            ("gen/Ferryx Daemon.app/Contents/MacOS/ferryx", "gen/mhc"),
        ];
        for (exe, mhc_dir) in layouts {
            let exe = temp.0.join(exe);
            touch(&exe);
            write_payload(&temp.0.join(mhc_dir), false);
            let found = bundled_mhc_dir_near(&exe).unwrap_or_else(|| panic!("no bundled mhc for {}", exe.display()));
            assert_eq!(
                std::fs::canonicalize(found).unwrap(),
                std::fs::canonicalize(temp.0.join(mhc_dir)).unwrap(),
                "{}",
                exe.display()
            );
        }
        let bare = temp.0.join("bare/ferryx");
        touch(&bare);
        assert_eq!(bundled_mhc_dir_near(&bare), None);
        // A payload for another triple only is not this build's mhc.
        touch(&temp.0.join("other/mhc/riscv64-unknown-linux-gnu/mhc/mhc"));
        touch(&temp.0.join("other/ferryx"));
        assert_eq!(bundled_mhc_dir_near(&temp.0.join("other/ferryx")), None);
    }

    #[test]
    fn the_bundled_bun_dir_goes_after_the_guard_and_before_the_user_path() {
        let temp = TempDir::new("mhc-bun-path");
        let binary = write_payload(&temp.0.join("mhc"), true);
        let guard = temp.0.join("guard-bin");
        let base = std::env::join_paths(["/opt/homebrew/bin", "/usr/bin"]).unwrap();
        let path = mhc_child_path(&binary, vec![guard.clone()], &base).unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(
            entries,
            vec![
                guard.clone(),
                temp.0.join("mhc").join(host_triple()).join("bun"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/bin"),
            ]
        );

        // An mhc from PATH or an override keeps the user's PATH untouched.
        let path = mhc_child_path(Path::new("mhc"), vec![guard.clone()], &base).unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(entries, vec![guard.clone(), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/bin")]);

        // A bundled mhc whose bun is missing adds nothing.
        let no_bun = write_payload(&temp.0.join("nobun"), false);
        let path = mhc_child_path(&no_bun, Vec::new(), &base).unwrap();
        assert_eq!(path, base);
    }

    #[test]
    fn the_pane_launcher_puts_the_bundled_bun_first_only_for_a_bundled_mhc() {
        let temp = TempDir::new("mhc-pane");
        let state = temp.0.join("state");
        assert_eq!(install_pane_launcher(&state, Path::new("mhc")).unwrap(), None);
        let no_bun = write_payload(&temp.0.join("nobun"), false);
        assert_eq!(install_pane_launcher(&state, &no_bun).unwrap(), None);

        let windows = pane_launcher_script(Path::new(r"C:\Ferryx\mhc\t\mhc\mhc.exe"), Path::new(r"C:\Ferryx\mhc\t\bun"), true);
        assert!(windows.contains("set \"PATH=C:\\Ferryx\\mhc\\t\\bun;%PATH%\""), "{windows}");
        assert!(windows.contains("\"C:\\Ferryx\\mhc\\t\\mhc\\mhc.exe\" %*"), "{windows}");
        assert!(!windows.contains("env "), "no POSIX env wrapper on Windows");

        let unix = pane_launcher_script(Path::new("/A p/it's/mhc"), Path::new("/A p/bun"), false);
        assert!(unix.contains("PATH='/A p/bun':\"$PATH\""), "{unix}");
        assert!(unix.contains("exec '/A p/it'\\''s/mhc' \"$@\""), "{unix}");
    }

    #[cfg(unix)]
    #[test]
    fn the_pane_launcher_runs_the_bundled_mhc_with_the_bundled_bun_on_path() {
        let temp = TempDir::new("mhc-pane-run");
        let binary = write_payload(&temp.0.join("Ferryx it's/mhc"), true);
        std::fs::write(&binary, "#!/bin/sh\nprintf '%s|%s' \"$PATH\" \"$*\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let launcher = install_pane_launcher(&temp.0.join("state"), &binary).unwrap().expect("launcher");
        assert_eq!(launcher, temp.0.join("state/pane-bin/mhc-pane"));
        let output = std::process::Command::new(&launcher)
            .args(["--mode", "x"])
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let bun_dir = binary.parent().unwrap().parent().unwrap().join("bun");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{}:/usr/bin:/bin|--mode x", bun_dir.display())
        );
    }

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
}
