use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenWithApp {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenWithApps {
    pub apps: Vec<OpenWithApp>,
    pub supports_chooser: bool,
}

pub(crate) fn validate_local_file(path: &str) -> Result<PathBuf, IpcError> {
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidPath,
            format!("path must be absolute: '{path}'"),
        ));
    }
    if !p.exists() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidPath,
            format!("file does not exist: '{path}'"),
        ));
    }
    if !p.is_file() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidPath,
            format!("path is not a regular file: '{path}'"),
        ));
    }
    Ok(p)
}

/// "Open with" hands the file to an arbitrary app, so a file fetched from a
/// remote host gets the same refusal as the system opener: only passive
/// documents may leave the preview.
fn ensure_open_with_allowed(path: &Path, remote_files_root: Option<&Path>) -> Result<(), IpcError> {
    crate::ipc::file_preview_contract::ensure_remote_cache_launchable(
        path,
        crate::ipc::file_link::EditorTarget::System,
        remote_files_root,
        false,
    )
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn desktop_entry_for_mime(contents: &str, mime: &str) -> Option<String> {
    let trimmed_mime = mime.trim();
    if trimmed_mime.is_empty() {
        return None;
    }
    let mut in_desktop_entry = false;
    let mut name: Option<String> = None;
    let mut mime_matches = false;
    let mut no_display = false;

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim();
            if key == "Name" && name.is_none() {
                name = Some(val.to_string());
            } else if key == "NoDisplay" {
                if val.eq_ignore_ascii_case("true") {
                    no_display = true;
                }
            } else if key == "MimeType"
                && val
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .any(|m| m == trimmed_mime)
            {
                mime_matches = true;
            }
        }
    }

    if !no_display && mime_matches {
        name
    } else {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn is_valid_desktop_id(id: &str) -> bool {
    let Some(stem) = id.strip_suffix(".desktop") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

#[cfg(target_os = "linux")]
pub(crate) fn desktop_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        if !data_home.is_empty() {
            dirs.push(PathBuf::from(data_home).join("applications"));
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(home).join(".local/share/applications"));
        }
    }

    let data_dirs = std::env::var_os("XDG_DATA_DIRS");
    let data_dirs_str = data_dirs.as_ref().and_then(|s| s.to_str()).unwrap_or("");
    let paths: Vec<&str> = if data_dirs_str.trim().is_empty() {
        vec!["/usr/local/share", "/usr/share"]
    } else {
        data_dirs_str
            .split(':')
            .filter(|s| !s.trim().is_empty())
            .collect()
    };

    for base in paths {
        dirs.push(PathBuf::from(base).join("applications"));
    }

    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        if seen.insert(dir.clone()) {
            deduped.push(dir);
        }
    }
    deduped
}

#[cfg(target_os = "macos")]
fn get_macos_open_with_apps(file_path: &Path) -> Result<OpenWithApps, IpcError> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let path_str = file_path.to_str().ok_or_else(|| {
        IpcError::new(IpcErrorCode::InvalidPath, "file path is not valid UTF-8")
    })?;
    let ns_path = NSString::from_str(path_str);
    let ns_url = NSURL::fileURLWithPath(&ns_path);
    let workspace = NSWorkspace::sharedWorkspace();
    let app_urls = workspace.URLsForApplicationsToOpenURL(&ns_url);

    let mut apps = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for app_url in app_urls.iter() {
        if let Some(path_ns) = app_url.path() {
            let app_path_str = path_ns.to_string();
            if seen_ids.insert(app_path_str.clone()) {
                let trimmed_path = app_path_str.trim_end_matches('/');
                let file_name = Path::new(trimmed_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(trimmed_path);
                let name = file_name
                    .strip_suffix(".app")
                    .unwrap_or(file_name)
                    .to_string();
                apps.push(OpenWithApp {
                    id: app_path_str,
                    name,
                });
                if apps.len() >= 30 {
                    break;
                }
            }
        }
    }

    Ok(OpenWithApps {
        apps,
        supports_chooser: false,
    })
}

#[cfg(target_os = "linux")]
fn get_linux_open_with_apps(file_path: &Path) -> Result<OpenWithApps, IpcError> {
    let output = match std::process::Command::new("xdg-mime")
        .arg("query")
        .arg("filetype")
        .arg(file_path)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => {
            return Ok(OpenWithApps {
                apps: Vec::new(),
                supports_chooser: false,
            });
        }
    };

    let mime = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if mime.is_empty() {
        return Ok(OpenWithApps {
            apps: Vec::new(),
            supports_chooser: false,
        });
    }

    let mut apps = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for dir in desktop_application_dirs() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            if !file_name.ends_with(".desktop") {
                continue;
            }
            if seen_ids.contains(&file_name) {
                continue;
            }
            let contents = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Some(name) = desktop_entry_for_mime(&contents, &mime) {
                seen_ids.insert(file_name.clone());
                apps.push(OpenWithApp {
                    id: file_name,
                    name,
                });
            }
        }
    }

    Ok(OpenWithApps {
        apps,
        supports_chooser: false,
    })
}

#[tauri::command]
pub async fn cmd_file_open_with_apps<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    path: String,
) -> Result<OpenWithApps, IpcError> {
    crate::ipc::file_preview::ensure_trusted_window(webview.label())?;
    crate::ipc::run_blocking(move || {
        let file_path = validate_local_file(&path)?;

        #[cfg(target_os = "macos")]
        {
            get_macos_open_with_apps(&file_path)
        }
        #[cfg(target_os = "linux")]
        {
            get_linux_open_with_apps(&file_path)
        }
        #[cfg(target_os = "windows")]
        {
            let _ = file_path;
            Ok(OpenWithApps {
                apps: Vec::new(),
                supports_chooser: true,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            let _ = file_path;
            Ok(OpenWithApps {
                apps: Vec::new(),
                supports_chooser: false,
            })
        }
    })
    .await
}

#[cfg(target_os = "macos")]
fn launch_macos_open_with(file_path: &Path, app_id: &str) -> Result<(), IpcError> {
    // Only an application LaunchServices itself offered for this file may be
    // used; any other folder ending in ".app" is refused.
    let offered = get_macos_open_with_apps(file_path)?;
    if !offered.apps.iter().any(|app| app.id == app_id) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            format!("invalid application bundle '{app_id}'"),
        ));
    }

    // `open -a` returns as soon as LaunchServices accepts the request; waiting
    // reaps it (no zombie) and surfaces its failure.
    let status = std::process::Command::new("open")
        .arg("-a")
        .arg(app_id)
        .arg(file_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("failed to spawn open: {e}"),
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(IpcError::new(
            IpcErrorCode::IoError,
            format!("open -a exited with {status}"),
        ))
    }
}

#[cfg(target_os = "linux")]
fn launch_linux_open_with(file_path: &Path, app_id: &str) -> Result<(), IpcError> {
    if !is_valid_desktop_id(app_id) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            format!("invalid desktop application id '{app_id}'"),
        ));
    }

    let stem = &app_id[..app_id.len() - ".desktop".len()];

    // gtk-launch exits once the app is started; a non-zero exit (unknown id,
    // no display) falls through to gio instead of reporting success.
    let gtk_status = std::process::Command::new("gtk-launch")
        .arg(stem)
        .arg(file_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if gtk_status.is_ok_and(|status| status.success()) {
        return Ok(());
    }

    let full_desktop_path = desktop_application_dirs()
        .into_iter()
        .map(|dir| dir.join(app_id))
        .find(|candidate| candidate.is_file());

    if let Some(desktop_file) = full_desktop_path {
        let status = std::process::Command::new("gio")
            .arg("launch")
            .arg(&desktop_file)
            .arg(file_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("failed to launch application with gio: {e}"),
                )
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(IpcError::new(
                IpcErrorCode::IoError,
                format!("gio launch exited with {status}"),
            ))
        }
    } else {
        Err(IpcError::new(
            IpcErrorCode::IoError,
            format!(
                "gtk-launch failed and desktop file '{app_id}' was not found in application directories"
            ),
        ))
    }
}

#[cfg(target_os = "windows")]
fn launch_windows_open_with(file_path: &Path, app_id: &str) -> Result<(), IpcError> {
    if app_id != "chooser" {
        return Err(IpcError::new(
            IpcErrorCode::InvalidRequest,
            format!("unsupported app_id '{app_id}' on Windows; only 'chooser' is supported"),
        ));
    }

    std::process::Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(file_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("failed to spawn rundll32.exe: {e}"),
            )
        })?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn launch_fallback_open_with(_file_path: &Path, _app_id: &str) -> Result<(), IpcError> {
    Err(IpcError::new(
        IpcErrorCode::InvalidRequest,
        "platform open with is unsupported",
    ))
}

#[tauri::command]
pub async fn cmd_file_open_with<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    path: String,
    app_id: String,
) -> Result<(), IpcError> {
    crate::ipc::file_preview::ensure_trusted_window(webview.label())?;
    let remote_files_root = crate::ipc::file_preview_contract::app_remote_files_root(&webview);
    crate::ipc::run_blocking(move || {
        let file_path = validate_local_file(&path)?;
        ensure_open_with_allowed(&file_path, remote_files_root.as_deref())?;

        #[cfg(target_os = "macos")]
        {
            launch_macos_open_with(&file_path, &app_id)
        }
        #[cfg(target_os = "linux")]
        {
            launch_linux_open_with(&file_path, &app_id)
        }
        #[cfg(target_os = "windows")]
        {
            launch_windows_open_with(&file_path, &app_id)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            launch_fallback_open_with(&file_path, &app_id)
        }
    })
    .await
}

#[tauri::command]
pub async fn cmd_webview_print<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
) -> Result<bool, IpcError> {
    print_completion::print(webview).await
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "file_print_completion.rs"]
mod print_completion;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod print_completion {
pub async fn print<R: tauri::Runtime>(webview: tauri::Webview<R>) -> Result<bool, super::IpcError> {
    webview
        .print()
        .map(|()| false)
        .map_err(|e| super::IpcError::internal(e.to_string()))
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn open_with_refuses_active_files_fetched_from_a_remote_host() {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::ipc::remote_file::remote_files_root(dir.path());
        let fetched = root.join("abc").join("run.command");
        std::fs::create_dir_all(fetched.parent().unwrap()).unwrap();
        std::fs::write(&fetched, b"echo hi").unwrap();
        let err = ensure_open_with_allowed(&fetched, Some(&root)).unwrap_err();
        assert_eq!(err.details.unwrap()["reason"], "remoteFileNotLaunchable");

        let notes = root.join("abc").join("notes.txt");
        std::fs::write(&notes, b"hi").unwrap();
        assert!(ensure_open_with_allowed(&notes, Some(&root)).is_ok());

        let local = dir.path().join("run.command");
        std::fs::write(&local, b"echo hi").unwrap();
        assert!(ensure_open_with_allowed(&local, Some(&root)).is_ok());
    }

    #[test]
    fn test_desktop_entry_for_mime_matches_semicolon_list() {
        let contents = "\
[Desktop Entry]
Name=Markdown Studio
MimeType=text/plain;text/markdown;
NoDisplay=false
";
        let res = desktop_entry_for_mime(contents, "text/markdown");
        assert_eq!(res, Some("Markdown Studio".to_string()));
    }

    #[test]
    fn test_desktop_entry_for_mime_rejects_nodisplay_true() {
        let contents = "\
[Desktop Entry]
Name=Hidden Markdown
MimeType=text/plain;text/markdown;
NoDisplay=true
";
        let res = desktop_entry_for_mime(contents, "text/markdown");
        assert_eq!(res, None);
    }

    #[test]
    fn test_desktop_entry_for_mime_rejects_other_mimes() {
        let contents = "\
[Desktop Entry]
Name=Plain Text Only
MimeType=text/plain;
NoDisplay=false
";
        let res = desktop_entry_for_mime(contents, "text/markdown");
        assert_eq!(res, None);
    }

    #[test]
    fn test_desktop_entry_for_mime_returns_name() {
        let contents = "\
[Desktop Entry]
Type=Application
Name=Visual Studio Code
MimeType=text/plain;text/markdown;application/json;
";
        let res = desktop_entry_for_mime(contents, "text/markdown");
        assert_eq!(res, Some("Visual Studio Code".to_string()));
    }

    #[test]
    fn test_validate_local_file_rejects_relative_path() {
        let err = validate_local_file("relative/file.txt").unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidPath);
    }

    #[test]
    fn test_validate_local_file_rejects_missing_path() {
        let missing = std::env::temp_dir().join("ferryx_definitely_nonexistent_file_987654.txt");
        assert!(missing.is_absolute());
        assert!(!missing.exists());
        let err = validate_local_file(&missing.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidPath);
    }

    #[test]
    fn test_validate_local_file_rejects_directory() {
        let dir = std::env::temp_dir();
        assert!(dir.is_dir());
        let err = validate_local_file(&dir.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidPath);
    }

    #[test]
    fn test_validate_local_file_accepts_tempfile() {
        let mut temp = tempfile::NamedTempFile::new().expect("failed to create temp file");
        writeln!(temp, "test data").expect("failed to write to temp file");
        let path = temp.path();
        let validated = validate_local_file(&path.to_string_lossy()).expect("tempfile must be accepted");
        assert_eq!(validated, path);
    }

    #[test]
    fn test_is_valid_desktop_id() {
        assert!(is_valid_desktop_id("code.desktop"));
        assert!(is_valid_desktop_id("org.gnome.TextEditor.desktop"));
        assert!(is_valid_desktop_id("app-1.0_beta.desktop"));

        assert!(!is_valid_desktop_id(".desktop"));
        assert!(!is_valid_desktop_id("code"));
        assert!(!is_valid_desktop_id("code.desktop.bak"));
        assert!(!is_valid_desktop_id("dir/code.desktop"));
        assert!(!is_valid_desktop_id("code;.desktop"));
        assert!(!is_valid_desktop_id("code space.desktop"));
    }
}
