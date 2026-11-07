//! Linux: a per-user `.desktop` entry for `x-scheme-handler/http(s)`, then `xdg-settings`
//! (falling back to `xdg-mime`) to make it the default web browser.

use std::process::Command;

use super::{DefaultBrowserStatus, MakeDefaultOutcome, LINUX_DESKTOP_FILE};

fn applications_dir() -> Option<std::path::PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".local/share"))
        })?;
    Some(data_home.join("applications"))
}

pub fn status() -> DefaultBrowserStatus {
    match Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
    {
        Ok(output) if output.status.success() => {
            super::linux_status_from_xdg(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(_) => DefaultBrowserStatus::NotDefault { current: None },
        Err(_) => DefaultBrowserStatus::Unsupported,
    }
}

pub fn make_default() -> Result<MakeDefaultOutcome, String> {
    // An AppImage runs from a temporary mount; the stable path is the image itself.
    let exe = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .map(Ok)
        .unwrap_or_else(std::env::current_exe)
        .map_err(|error| format!("could not locate Ferryx: {error}"))?;
    let dir = applications_dir().ok_or("no XDG data directory (HOME is unset)")?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let path = dir.join(LINUX_DESKTOP_FILE);
    std::fs::write(&path, super::linux_desktop_entry(&exe.to_string_lossy()))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    // Desktops read the MIME cache; a missing update-desktop-database is not an error.
    let _ = Command::new("update-desktop-database").arg(&dir).status();

    let mut commands = super::linux_registration_commands().into_iter();
    let run =
        |(program, args): (&str, Vec<String>)| match Command::new(program).args(&args).status() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("{program} exited with {status}")),
            Err(error) => Err(format!("{program}: {error}")),
        };
    // xdg-settings is the standard; desktops without it still honour the xdg-mime pair.
    let Err(xdg_settings) = commands.next().map_or(Ok(()), run) else {
        return Ok(MakeDefaultOutcome::Requested);
    };
    commands
        .map(run)
        .collect::<Result<Vec<()>, String>>()
        .map(|_| MakeDefaultOutcome::Requested)
        .map_err(|fallback| {
            format!("could not set the default browser: {xdg_settings}; {fallback}")
        })
}
