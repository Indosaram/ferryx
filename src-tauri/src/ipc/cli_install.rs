use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ipc::{run_blocking, IpcError};

#[derive(Debug, thiserror::Error)]
pub enum CliInstallError {
    #[error(
        "CLI launcher target already exists as a regular file and cannot be overwritten: {path}"
    )]
    FileCollision { path: PathBuf },

    #[error("CLI launcher target already exists as a directory and cannot be overwritten: {path}")]
    DirectoryCollision { path: PathBuf },

    #[error("CLI launcher parent directory is a symlink and cannot be used: {path}")]
    ParentSymlinkDenied { path: PathBuf },

    #[error("CLI launcher parent path is not a directory: {path}")]
    ParentNotDirectory { path: PathBuf },

    #[error("CLI launcher installation is not supported on platform: {platform}")]
    PlatformUnsupported { platform: String },

    #[error("Failed to determine canonical active executable: {0}")]
    ExecutableNotFound(String),

    #[error("User home directory could not be determined")]
    HomeDirNotFound,

    #[error("Filesystem IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLauncherStatus {
    pub launcher_path: String,
    pub is_installed: bool,
    pub is_symlink: bool,
    pub current_target: Option<String>,
    pub active_executable: Option<String>,
    pub is_supported: bool,
}

pub fn get_default_launcher_path_from(home: Option<&Path>) -> Result<PathBuf, CliInstallError> {
    let home = home.ok_or(CliInstallError::HomeDirNotFound)?;
    if home.as_os_str().is_empty() {
        return Err(CliInstallError::HomeDirNotFound);
    }
    Ok(home.join(".local").join("bin").join("ferryx"))
}

pub fn get_default_launcher_path() -> Result<PathBuf, CliInstallError> {
    let home = std::env::var_os("HOME");
    get_default_launcher_path_from(home.as_deref().map(Path::new))
}

pub fn resolve_launcher_status(
    is_unix: bool,
    home: Option<&Path>,
    active_exe: Option<&Path>,
) -> Result<CliLauncherStatus, CliInstallError> {
    let active_exe_canonical = active_exe.and_then(|p| {
        std::fs::canonicalize(p)
            .ok()
            .or_else(|| Some(p.to_path_buf()))
    });
    let active_exe_str = active_exe_canonical
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    if !is_unix {
        return Ok(CliLauncherStatus {
            launcher_path: String::new(),
            is_installed: false,
            is_symlink: false,
            current_target: None,
            active_executable: active_exe_str,
            is_supported: false,
        });
    }

    let launcher_path = get_default_launcher_path_from(home)?;
    Ok(check_launcher_status(
        &launcher_path,
        active_exe_canonical.as_deref(),
    ))
}

pub fn get_canonical_active_executable() -> Result<PathBuf, CliInstallError> {
    let current =
        std::env::current_exe().map_err(|e| CliInstallError::ExecutableNotFound(e.to_string()))?;
    let canonical = std::fs::canonicalize(&current).unwrap_or(current);
    Ok(canonical)
}

pub fn check_launcher_status(launcher_path: &Path, active_exe: Option<&Path>) -> CliLauncherStatus {
    let is_supported = cfg!(unix);
    let launcher_str = launcher_path.to_string_lossy().to_string();
    let active_exe_canonical = active_exe.and_then(|p| {
        std::fs::canonicalize(p)
            .ok()
            .or_else(|| Some(p.to_path_buf()))
    });
    let active_exe_str = active_exe_canonical
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    match std::fs::symlink_metadata(launcher_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                let target_path = std::fs::read_link(launcher_path).ok();
                let current_target = target_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string());
                let is_installed = if let (Some(canonical_active), Ok(resolved_launcher)) =
                    (&active_exe_canonical, std::fs::canonicalize(launcher_path))
                {
                    &resolved_launcher == canonical_active
                } else {
                    false
                };
                CliLauncherStatus {
                    launcher_path: launcher_str,
                    is_installed,
                    is_symlink: true,
                    current_target,
                    active_executable: active_exe_str,
                    is_supported,
                }
            } else {
                CliLauncherStatus {
                    launcher_path: launcher_str,
                    is_installed: false,
                    is_symlink: false,
                    current_target: None,
                    active_executable: active_exe_str,
                    is_supported,
                }
            }
        }
        Err(_) => CliLauncherStatus {
            launcher_path: launcher_str,
            is_installed: false,
            is_symlink: false,
            current_target: None,
            active_executable: active_exe_str,
            is_supported,
        },
    }
}

pub fn install_launcher(
    launcher_path: &Path,
    active_exe: &Path,
) -> Result<CliLauncherStatus, CliInstallError> {
    if !cfg!(unix) {
        return Err(CliInstallError::PlatformUnsupported {
            platform: std::env::consts::OS.to_string(),
        });
    }

    let canonical_active_exe = std::fs::canonicalize(active_exe).map_err(|e| {
        CliInstallError::ExecutableNotFound(format!(
            "Failed to canonicalize active executable {}: {e}",
            active_exe.display()
        ))
    })?;

    let parent = launcher_path
        .parent()
        .ok_or_else(|| CliInstallError::ParentNotDirectory {
            path: launcher_path.to_path_buf(),
        })?;

    if let Ok(meta) = std::fs::symlink_metadata(parent) {
        if meta.file_type().is_symlink() {
            return Err(CliInstallError::ParentSymlinkDenied {
                path: parent.to_path_buf(),
            });
        }
        if !meta.is_dir() {
            return Err(CliInstallError::ParentNotDirectory {
                path: parent.to_path_buf(),
            });
        }
    }

    if let Some(grandparent) = parent.parent() {
        if let Ok(meta) = std::fs::symlink_metadata(grandparent) {
            if meta.file_type().is_symlink() {
                return Err(CliInstallError::ParentSymlinkDenied {
                    path: grandparent.to_path_buf(),
                });
            }
        }
    }

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        match std::fs::symlink_metadata(launcher_path) {
            Ok(meta) => {
                let ft = meta.file_type();
                if ft.is_symlink() {
                    let temp_name = format!(".ferryx-launcher-{}.tmp", uuid::Uuid::new_v4());
                    let temp_link = parent.join(temp_name);
                    std::os::unix::fs::symlink(&canonical_active_exe, &temp_link)?;
                    if let Err(e) = std::fs::rename(&temp_link, launcher_path) {
                        let _ = std::fs::remove_file(&temp_link);
                        return Err(CliInstallError::Io(e));
                    }
                } else if ft.is_dir() {
                    return Err(CliInstallError::DirectoryCollision {
                        path: launcher_path.to_path_buf(),
                    });
                } else {
                    return Err(CliInstallError::FileCollision {
                        path: launcher_path.to_path_buf(),
                    });
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let temp_name = format!(".ferryx-launcher-{}.tmp", uuid::Uuid::new_v4());
                let temp_link = parent.join(temp_name);
                std::os::unix::fs::symlink(&canonical_active_exe, &temp_link)?;
                if let Err(e) = std::fs::rename(&temp_link, launcher_path) {
                    let _ = std::fs::remove_file(&temp_link);
                    return Err(CliInstallError::Io(e));
                }
            }
            Err(err) => {
                return Err(CliInstallError::Io(err));
            }
        }
    }

    Ok(check_launcher_status(
        launcher_path,
        Some(&canonical_active_exe),
    ))
}

#[tauri::command]
pub async fn cmd_cli_launcher_status() -> Result<CliLauncherStatus, IpcError> {
    run_blocking(|| {
        let home = std::env::var_os("HOME");
        let active_exe = get_canonical_active_executable().ok();
        let status = resolve_launcher_status(
            cfg!(unix),
            home.as_deref().map(Path::new),
            active_exe.as_deref(),
        )?;
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn cmd_cli_launcher_install() -> Result<CliLauncherStatus, IpcError> {
    run_blocking(|| {
        let launcher_path = get_default_launcher_path()?;
        let active_exe = get_canonical_active_executable()?;
        let status = install_launcher(&launcher_path, &active_exe)?;
        Ok(status)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_launcher_status_non_unix_without_home_returns_unsupported() {
        let status = resolve_launcher_status(false, None, None)
            .expect("non-Unix status must succeed even without HOME");
        assert!(!status.is_supported);
        assert!(!status.is_installed);
        assert!(!status.is_symlink);
        assert_eq!(status.launcher_path, "");
        assert_eq!(status.current_target, None);
        assert_eq!(status.active_executable, None);
    }

    #[test]
    fn test_resolve_launcher_status_non_unix_preserves_active_exe() {
        let fake_exe = Path::new("/custom/bin/ferryx.exe");
        let status = resolve_launcher_status(false, None, Some(fake_exe))
            .expect("non-Unix status must succeed even without HOME");
        assert!(!status.is_supported);
        assert!(!status.is_installed);
        assert!(!status.is_symlink);
        assert_eq!(status.launcher_path, "");
        assert_eq!(status.current_target, None);
        assert_eq!(
            status.active_executable,
            Some("/custom/bin/ferryx.exe".into())
        );
    }

    #[test]
    fn test_resolve_launcher_status_unix_requires_home() {
        let err = resolve_launcher_status(true, None, None)
            .expect_err("Unix status without HOME must return HomeDirNotFound");
        match err {
            CliInstallError::HomeDirNotFound => {}
            other => panic!("expected HomeDirNotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_launcher_status_unix_with_home() {
        let home = Path::new("/mock/home");
        let status = resolve_launcher_status(true, Some(home), None)
            .expect("Unix status with HOME should succeed");
        assert_eq!(PathBuf::from(status.launcher_path), home.join(".local/bin/ferryx"));
        assert_eq!(status.is_supported, cfg!(unix));
    }

    #[test]
    fn test_cli_launcher_status_dto_camel_case_contract() {
        let status = CliLauncherStatus {
            launcher_path: "/Users/test/.local/bin/ferryx".into(),
            is_installed: true,
            is_symlink: true,
            current_target: Some("/Applications/Ferryx.app/Contents/MacOS/ferryx".into()),
            active_executable: Some("/Applications/Ferryx.app/Contents/MacOS/ferryx".into()),
            is_supported: true,
        };

        let json = serde_json::to_string(&status).expect("serialize status");
        assert!(json.contains("\"launcherPath\":\"/Users/test/.local/bin/ferryx\""));
        assert!(json.contains("\"isInstalled\":true"));
        assert!(json.contains("\"isSymlink\":true"));
        assert!(
            json.contains("\"currentTarget\":\"/Applications/Ferryx.app/Contents/MacOS/ferryx\"")
        );
        assert!(json
            .contains("\"activeExecutable\":\"/Applications/Ferryx.app/Contents/MacOS/ferryx\""));
        assert!(json.contains("\"isSupported\":true"));

        let deserialized: CliLauncherStatus =
            serde_json::from_str(&json).expect("deserialize status");
        assert_eq!(deserialized, status);
    }

    #[test]
    #[cfg(unix)]
    fn test_install_creates_parent_directory_and_symlink() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary contents").expect("write fake exe");

        let launcher_path = temp_dir.path().join("sub").join("bin").join("ferryx");
        assert!(!launcher_path.parent().unwrap().exists());

        let status = install_launcher(&launcher_path, &fake_exe).expect("install launcher");
        assert!(status.is_installed);
        assert!(status.is_symlink);
        assert_eq!(
            status.current_target,
            Some(
                std::fs::canonicalize(&fake_exe)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            )
        );

        let metadata = std::fs::symlink_metadata(&launcher_path).expect("symlink metadata");
        assert!(metadata.file_type().is_symlink());

        let target = std::fs::read_link(&launcher_path).expect("read link");
        assert_eq!(
            std::fs::canonicalize(target).unwrap(),
            std::fs::canonicalize(&fake_exe).unwrap()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_install_refreshes_existing_symlink() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe_old = temp_dir.path().join("fake_ferryx_old");
        let fake_exe_new = temp_dir.path().join("fake_ferryx_new");
        std::fs::write(&fake_exe_old, b"old binary").expect("write old");
        std::fs::write(&fake_exe_new, b"new binary").expect("write new");

        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let launcher_path = bin_dir.join("ferryx");

        // Initial install with old binary
        let status1 = install_launcher(&launcher_path, &fake_exe_old).expect("install old");
        assert!(status1.is_installed);
        assert_eq!(
            status1.current_target,
            Some(
                std::fs::canonicalize(&fake_exe_old)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            )
        );

        // Before refresh, status checked against new binary reports not installed (targeting old)
        let status_pre = check_launcher_status(&launcher_path, Some(&fake_exe_new));
        assert!(!status_pre.is_installed);
        assert!(status_pre.is_symlink);

        // Refresh install with new binary
        let status2 = install_launcher(&launcher_path, &fake_exe_new).expect("refresh new");
        assert!(status2.is_installed);
        assert_eq!(
            status2.current_target,
            Some(
                std::fs::canonicalize(&fake_exe_new)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            )
        );

        let target = std::fs::read_link(&launcher_path).expect("read link");
        assert_eq!(
            std::fs::canonicalize(target).unwrap(),
            std::fs::canonicalize(&fake_exe_new).unwrap()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_install_rejects_regular_file_collision() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary").expect("write fake exe");

        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let launcher_path = bin_dir.join("ferryx");
        std::fs::write(&launcher_path, b"existing regular file").expect("write regular file");

        let err =
            install_launcher(&launcher_path, &fake_exe).expect_err("must reject regular file");
        match err {
            CliInstallError::FileCollision { path } => {
                assert_eq!(path, launcher_path);
            }
            other => panic!("expected FileCollision, got {other:?}"),
        }

        // Must not have overwritten the file
        let content = std::fs::read(&launcher_path).expect("read regular file");
        assert_eq!(content, b"existing regular file");
    }

    #[test]
    #[cfg(unix)]
    fn test_install_rejects_directory_collision() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary").expect("write fake exe");

        let bin_dir = temp_dir.path().join("bin");
        let launcher_path = bin_dir.join("ferryx");
        std::fs::create_dir_all(&launcher_path).expect("create directory at launcher path");

        let err = install_launcher(&launcher_path, &fake_exe).expect_err("must reject directory");
        match err {
            CliInstallError::DirectoryCollision { path } => {
                assert_eq!(path, launcher_path);
            }
            other => panic!("expected DirectoryCollision, got {other:?}"),
        }

        assert!(launcher_path.is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn test_install_rejects_symlinked_parent_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary").expect("write fake exe");

        let real_target_dir = temp_dir.path().join("real_bin");
        std::fs::create_dir_all(&real_target_dir).expect("create real dir");

        let symlinked_bin_dir = temp_dir.path().join("symlinked_bin");
        std::os::unix::fs::symlink(&real_target_dir, &symlinked_bin_dir).expect("symlink bin dir");

        let launcher_path = symlinked_bin_dir.join("ferryx");

        let err =
            install_launcher(&launcher_path, &fake_exe).expect_err("must reject symlinked parent");
        match err {
            CliInstallError::ParentSymlinkDenied { path } => {
                assert_eq!(path, symlinked_bin_dir);
            }
            other => panic!("expected ParentSymlinkDenied, got {other:?}"),
        }

        // Verify no file was created in real_bin
        assert!(!real_target_dir.join("ferryx").exists());
    }

    #[test]
    #[cfg(unix)]
    fn test_install_rejects_symlinked_grandparent_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary").expect("write fake exe");

        let real_local = temp_dir.path().join("real_local");
        std::fs::create_dir_all(&real_local).expect("create real local");

        let symlinked_local = temp_dir.path().join("symlinked_local");
        std::os::unix::fs::symlink(&real_local, &symlinked_local).expect("symlink local");

        let launcher_path = symlinked_local.join("bin").join("ferryx");

        let err = install_launcher(&launcher_path, &fake_exe)
            .expect_err("must reject symlinked grandparent");
        match err {
            CliInstallError::ParentSymlinkDenied { path } => {
                assert_eq!(path, symlinked_local);
            }
            other => panic!("expected ParentSymlinkDenied, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_status_detection_various_scenarios() {
        let temp_dir = TempDir::new().expect("temp dir");
        let fake_exe = temp_dir.path().join("fake_ferryx_exe");
        std::fs::write(&fake_exe, b"binary").expect("write fake exe");

        let launcher_path = temp_dir.path().join("bin").join("ferryx");

        // Scenario 1: Path does not exist
        let status_missing = check_launcher_status(&launcher_path, Some(&fake_exe));
        assert!(!status_missing.is_installed);
        assert!(!status_missing.is_symlink);
        assert_eq!(status_missing.current_target, None);

        // Scenario 2: Path is a regular file
        std::fs::create_dir_all(launcher_path.parent().unwrap()).expect("create bin");
        std::fs::write(&launcher_path, b"file").expect("write file");
        let status_file = check_launcher_status(&launcher_path, Some(&fake_exe));
        assert!(!status_file.is_installed);
        assert!(!status_file.is_symlink);
        assert_eq!(status_file.current_target, None);
        std::fs::remove_file(&launcher_path).expect("remove file");

        // Scenario 3: Symlink targeting the active executable
        install_launcher(&launcher_path, &fake_exe).expect("install");
        let status_installed = check_launcher_status(&launcher_path, Some(&fake_exe));
        assert!(status_installed.is_installed);
        assert!(status_installed.is_symlink);
        assert!(status_installed.current_target.is_some());

        // Scenario 4: Symlink targeting a different executable
        let other_exe = temp_dir.path().join("other_exe");
        std::fs::write(&other_exe, b"other binary").expect("write other exe");
        let status_other = check_launcher_status(&launcher_path, Some(&other_exe));
        assert!(!status_other.is_installed);
        assert!(status_other.is_symlink);

        // Scenario 5: Broken symlink (target deleted)
        std::fs::remove_file(&fake_exe).expect("remove fake exe");
        let status_broken = check_launcher_status(&launcher_path, Some(&other_exe));
        assert!(!status_broken.is_installed);
        assert!(status_broken.is_symlink);
    }

    #[test]
    fn test_cli_install_error_mapping_to_ipc_error() {
        use crate::ipc::error::IpcErrorCode;

        let path = PathBuf::from("/test/path/ferryx");
        let file_err: IpcError = CliInstallError::FileCollision { path: path.clone() }.into();
        assert_eq!(file_err.code, IpcErrorCode::CliFileCollision);
        assert_eq!(
            file_err.details.as_ref().unwrap()["path"],
            "/test/path/ferryx"
        );

        let dir_err: IpcError = CliInstallError::DirectoryCollision { path: path.clone() }.into();
        assert_eq!(dir_err.code, IpcErrorCode::CliDirectoryCollision);
        assert_eq!(
            dir_err.details.as_ref().unwrap()["path"],
            "/test/path/ferryx"
        );

        let parent_symlink_err: IpcError =
            CliInstallError::ParentSymlinkDenied { path: path.clone() }.into();
        assert_eq!(
            parent_symlink_err.code,
            IpcErrorCode::CliParentSymlinkDenied
        );
        assert_eq!(
            parent_symlink_err.details.as_ref().unwrap()["path"],
            "/test/path/ferryx"
        );

        let unsupported_err: IpcError = CliInstallError::PlatformUnsupported {
            platform: "windows".into(),
        }
        .into();
        assert_eq!(unsupported_err.code, IpcErrorCode::CliPlatformUnsupported);
        assert_eq!(
            unsupported_err.details.as_ref().unwrap()["platform"],
            "windows"
        );

        let exe_err: IpcError =
            CliInstallError::ExecutableNotFound("cannot find exe".into()).into();
        assert_eq!(exe_err.code, IpcErrorCode::CliExecutableNotFound);
        assert_eq!(
            exe_err.details.as_ref().unwrap()["reason"],
            "cannot find exe"
        );

        let home_err: IpcError = CliInstallError::HomeDirNotFound.into();
        assert_eq!(home_err.code, IpcErrorCode::InvalidPath);

        let io_err: IpcError = CliInstallError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        ))
        .into();
        assert_eq!(io_err.code, IpcErrorCode::IoError);
    }
}
