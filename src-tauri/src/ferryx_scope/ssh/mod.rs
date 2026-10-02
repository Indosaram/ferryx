pub use helper::{boot_identity, recovery};
pub mod config;
pub mod helper;
pub mod hygiene;
pub mod process;

pub fn private_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if path.is_dir() { 0o700 } else { 0o600 }),
        )
        .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").map_err(|e| e.to_string())?;
        let grant = if path.is_dir() {
            format!("{user}:(OI)(CI)(F)")
        } else {
            format!("{user}:(F)")
        };
        let result = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &grant])
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err("REMOTE_PERMISSION_DENIED: cannot restrict runtime ACL".into());
        }
    }
    Ok(())
}

pub fn validate_private(path: &std::path::Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("FORBIDDEN: path cannot be a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let uid = unsafe { libc::geteuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err("FORBIDDEN: path must be owned by current user and private".into());
        }
        if metadata.is_file() && metadata.nlink() != 1 {
            return Err("FORBIDDEN: path cannot have hard links".into());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("FORBIDDEN: path cannot be a reparse point".into());
        }
        validate_windows_acl(path)?;
    }
    Ok(metadata)
}

#[cfg(windows)]
fn validate_windows_acl(path: &std::path::Path) -> Result<(), String> {
    use base64::Engine as _;
    let path_str = path
        .to_str()
        .ok_or_else(|| "FORBIDDEN: helper IPC path is invalid UTF-8".to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(path_str.as_bytes());
    let script = format!(
        r#"$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop';$raw=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));if($raw.StartsWith('\\?\UNC\')){{$raw='\\'+$raw.Substring(8)}}elseif($raw.StartsWith('\\?\')){{$raw=$raw.Substring(4)}};$p=[System.IO.Path]::GetFullPath($raw);$item=Get-Item -LiteralPath $p -Force;$acl=$item.GetAccessControl();$u=[System.Security.Principal.WindowsIdentity]::GetCurrent();$allowed=@($u.User.Value,'S-1-5-18','S-1-5-32-544');$owner=try{{$acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$null}};if(-not $owner){{try{{$owner=(New-Object System.Security.Principal.NTAccount($acl.Owner)).Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$owner=$acl.Owner}}}};if($allowed -notcontains $owner){{exit 1}};$rules=$acl.Access;if($null -eq $rules -or $rules.Count -eq 0){{exit 2}};$hasAllowed=$false;foreach($r in $rules){{if($r.AccessControlType.ToString() -eq 'Allow'){{$sid=try{{$r.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value}}catch{{$r.IdentityReference.Value}};if($allowed -notcontains $sid){{exit 3}};$hasAllowed=$true}}}};if(-not $hasAllowed){{exit 4}};exit 0;"#
    );
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|e| format!("FORBIDDEN: cannot execute PowerShell ACL validation: {e}"))?;

    if !output.status.success() {
        return Err("FORBIDDEN: helper IPC must be owned by the current user and private".into());
    }
    Ok(())
}
