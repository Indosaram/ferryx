use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PLIST_LABEL: &str = "com.rorca.daemon";

pub fn get_launchd_plist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library/LaunchAgents")
            .join(format!("{PLIST_LABEL}.plist"))
    })
}

pub fn generate_launchd_plist(executable_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{executable_path}</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/rorca-daemon.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/rorca-daemon.err</string>
</dict>
</plist>
"#
    )
}

pub fn install_launchd_agent_for_path(
    executable_path: &Path,
    plist_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create launch agent directory: {e}"))?;
    }

    let content = generate_launchd_plist(&executable_path.to_string_lossy());
    fs::write(plist_path, content).map_err(|e| format!("Failed to write plist: {e}"))?;

    let output = Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .output()
        .map_err(|e| format!("Failed to execute launchctl load: {e}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_error = !output.status.success()
        || stderr.contains("failed")
        || stderr.contains("Failed")
        || stderr.contains("error")
        || stderr.contains("Error");

    if has_error {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim()
        } else if !stdout.trim().is_empty() {
            stdout.trim()
        } else {
            "launchctl load failed with unknown error"
        };
        return Err(format!("launchctl load failed: {msg}"));
    }

    Ok(())
}

pub fn install_launchd_agent() -> Result<PathBuf, String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;

    install_launchd_agent_for_path(&current_exe, &plist_path)?;
    Ok(plist_path)
}

pub fn uninstall_launchd_agent_from_path(plist_path: &Path) -> Result<(), String> {
    if plist_path.exists() {
        let output = Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .output()
            .map_err(|e| format!("Failed to execute launchctl unload: {e}"))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let has_error = !output.status.success()
            || stderr.contains("failed")
            || stderr.contains("Failed")
            || stderr.contains("error")
            || stderr.contains("Error");

        if has_error {
            let msg = if !stderr.trim().is_empty() {
                stderr.trim()
            } else if !stdout.trim().is_empty() {
                stdout.trim()
            } else {
                "launchctl unload failed with unknown error"
            };
            return Err(format!("launchctl unload failed: {msg}"));
        }

        fs::remove_file(plist_path).map_err(|e| format!("Failed to remove plist file: {e}"))?;
    }
    Ok(())
}

pub fn uninstall_launchd_agent() -> Result<(), String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    uninstall_launchd_agent_from_path(&plist_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_generate_plist_contains_required_fields() {
        let plist = generate_launchd_plist("/opt/ferryx/bin/ferryx");
        assert!(plist.contains("<string>com.rorca.daemon</string>"));
        assert!(plist.contains("<string>/opt/ferryx/bin/ferryx</string>"));
        assert!(plist.contains("<string>--daemon</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn test_uninstall_surfaces_launchctl_failure() {
        let dir = tempdir().unwrap();
        let fake_plist = dir.path().join("com.rorca.daemon.plist");
        fs::write(&fake_plist, "invalid plist").unwrap();

        let result = uninstall_launchd_agent_from_path(&fake_plist);
        assert!(
            result.is_err(),
            "Expected error when unloading non-loaded/invalid plist via launchctl"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("launchctl unload failed"),
            "Error message should indicate launchctl unload failed: {err}"
        );
    }
}
