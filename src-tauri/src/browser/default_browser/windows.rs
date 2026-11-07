//! Windows: per-user `HKCU` registration through `reg.exe`, then the Default Apps page, since
//! Windows only lets the user pick the default browser.

use std::os::windows::process::CommandExt;
use std::process::Command;

use super::{DefaultBrowserStatus, MakeDefaultOutcome};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn reg(args: &[String]) -> Result<std::process::Output, String> {
    Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not run reg.exe: {error}"))
}

fn user_choice(scheme: &str) -> Option<String> {
    let output = reg(&[
        "query".into(),
        super::windows_user_choice_key(scheme),
        "/v".into(),
        "ProgId".into(),
    ])
    .ok()?;
    output
        .status
        .success()
        .then(|| super::windows_prog_id_from_reg_query(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

pub fn status() -> DefaultBrowserStatus {
    super::windows_status_from_prog_ids(
        user_choice("http").as_deref(),
        user_choice("https").as_deref(),
    )
}

pub fn make_default() -> Result<MakeDefaultOutcome, String> {
    let exe =
        std::env::current_exe().map_err(|error| format!("could not locate Ferryx: {error}"))?;
    let exe = exe.to_string_lossy().into_owned();
    for value in super::windows_registry_values(&exe) {
        let output = reg(&super::windows_reg_add_args(&value))?;
        if !output.status.success() {
            return Err(format!(
                "reg.exe could not write HKCU\\{}: {}",
                value.key,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    crate::ipc::browser::open_system_target(std::ffi::OsStr::new(super::WINDOWS_DEFAULT_APPS_URI))
        .map_err(|error| error.message)?;
    Ok(MakeDefaultOutcome::OpenedSystemSettings)
}
