//! Launch configuration for per-turn mhc RPC children.
use std::path::{Path, PathBuf};

use crate::error::Result;

/// mhc binary for hub turns: `FERRYX_HUB_MHC_BIN`, else `mhc` on the hub child PATH.
pub fn mhc_bin() -> String {
    crate::hub_env::var("FERRYX_HUB_MHC_BIN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "mhc".into())
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
