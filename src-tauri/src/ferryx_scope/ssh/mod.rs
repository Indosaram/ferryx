pub mod config;
pub mod helper;
pub mod process;

pub fn private_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(path, std::fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 })).map_err(|e| e.to_string())?; }
    #[cfg(windows)] {
        let user = std::env::var("USERNAME").map_err(|e| e.to_string())?;
        let result = std::process::Command::new("icacls").arg(path).args(["/inheritance:r", "/grant:r", &format!("{user}:(F)")]).output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err("REMOTE_PERMISSION_DENIED: cannot restrict runtime ACL".into()); }
    }
    Ok(())
}
