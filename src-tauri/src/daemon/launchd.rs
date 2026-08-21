use std::fs;
use std::path::PathBuf;
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

pub fn install_launchd_agent() -> Result<PathBuf, String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    if let Some(parent) = plist_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let current_exe = std::env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;
    let content = generate_launchd_plist(&current_exe.to_string_lossy());
    fs::write(&plist_path, content).map_err(|e| format!("Failed to write plist: {e}"))?;

    let _ = Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .output();

    Ok(plist_path)
}

pub fn uninstall_launchd_agent() -> Result<(), String> {
    let plist_path = get_launchd_plist_path().ok_or("Cannot determine HOME directory")?;
    if plist_path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .output();
        let _ = fs::remove_file(&plist_path);
    }
    Ok(())
}
