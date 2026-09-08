//! Read-only remote directory selection, independent of the local filesystem.

use super::runtime::{self, RemoteEnvironment, RemotePlatform};
use super::{direct, projects, SshHost};
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListDirectoriesRequest {
    pub host_id: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub path: String,
    pub parent_path: Option<String>,
    pub home_path: String,
    pub entries: Vec<DirectoryEntry>,
    pub truncated: bool,
}

struct CachedEnvironment {
    host: SshHost,
    environment: RemoteEnvironment,
    checked_at: Instant,
}

static ENVIRONMENTS: Mutex<Vec<CachedEnvironment>> = Mutex::new(Vec::new());

pub async fn list_directories(
    host_store: PathBuf,
    request: ListDirectoriesRequest,
) -> Result<DirectoryListing, IpcError> {
    let lookup = host_store.clone();
    let host = run_blocking(move || projects::enabled_host(&lookup, &request.host_id)).await?;
    let cached = {
        let mut cache = ENVIRONMENTS
            .lock()
            .map_err(|e| IpcError::internal(e.to_string()))?;
        cache.retain(|entry| entry.checked_at.elapsed() < Duration::from_secs(60));
        cache
            .iter()
            .find(|entry| entry.host == host)
            .map(|entry| entry.environment.clone())
    };
    let environment = match cached {
        Some(environment) => environment,
        None => {
            let environment = runtime::detect(&host).await?;
            let mut cache = ENVIRONMENTS
                .lock()
                .map_err(|e| IpcError::internal(e.to_string()))?;
            cache.retain(|entry| entry.host.id != host.id);
            if cache.len() >= 32 {
                cache.remove(0);
            }
            cache.push(CachedEnvironment {
                host: host.clone(),
                environment: environment.clone(),
                checked_at: Instant::now(),
            });
            environment
        }
    };
    let path = resolve_path(&environment, request.path.as_deref())?;
    let marker = format!("FERRYX_LIST_{}", uuid::Uuid::new_v4().simple());
    let script = listing_script(&environment, &path, &marker);
    let plan = direct::ssh_plan(&host, environment.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(12)).await?;
    let listing = parse_listing(&output, &marker, &environment)?;
    run_blocking(move || {
        if projects::enabled_host(&host_store, &host.id)? != host {
            return Err(runtime::error(
                IpcErrorCode::InvalidPath,
                "directory",
                "SSH host changed while browsing; open the folder again",
            ));
        }
        Ok(())
    })
    .await?;
    Ok(listing)
}

fn resolve_path(environment: &RemoteEnvironment, path: Option<&str>) -> Result<String, IpcError> {
    let path = match path {
        None | Some("~") => environment.home.clone(),
        Some(path) => {
            let relative = path.strip_prefix("~/").or_else(|| {
                (environment.platform == RemotePlatform::Windows)
                    .then(|| path.strip_prefix("~\\"))
                    .flatten()
            });
            match relative {
                Some(relative) => join_path(environment.platform, &environment.home, relative),
                None => path.to_string(),
            }
        }
    };
    environment.platform.validate_path(&path)?;
    Ok(path)
}

fn join_path(platform: RemotePlatform, root: &str, name: &str) -> String {
    match platform {
        RemotePlatform::Posix => format!("{}/{name}", root.trim_end_matches('/')),
        RemotePlatform::Windows => format!("{}\\{name}", root.trim_end_matches(['/', '\\'])),
    }
}

fn listing_script(environment: &RemoteEnvironment, path: &str, marker: &str) -> String {
    // Entry bytes plus two 4096-byte paths fit the transport's existing 16 KiB cap.
    match environment.platform {
        RemotePlatform::Posix => format!(
            r#"set -e
LC_ALL=C; export LC_ALL
cd {path}
[ -r . ] && [ -x . ] || {{ printf '%s\n' 'Directory is not readable' >&2; exit 1; }}
root=$(pwd -P)
parent=$(cd .. && pwd -P)
[ "$parent" != "$root" ] || parent=
printf '{marker}\000%s\000%s\000' "$root" "$parent"
n=0; bytes=0; truncated=0
for item in .[!.]* ..?* *; do
    [ -d "$item" ] || continue
    hidden=0; case "$item" in .*) hidden=1;; esac
    bytes=$((bytes + ${{#item}} + 3))
    if [ "$n" -ge 1000 ] || [ "$bytes" -gt 6000 ]; then truncated=1; break; fi
    printf '%s\000%s\000' "$item" "$hidden"
    n=$((n + 1))
done
printf 'END\000%s\000' "$truncated""#,
            path = direct::quote_posix(path),
        ),
        RemotePlatform::Windows => format!(
            r#"$item=Get-Item -LiteralPath {path} -Force
if (!$item.PSIsContainer -or $item.PSProvider.Name -ne 'FileSystem') {{ throw 'Remote path is not a filesystem directory' }}
$root=$item.FullName
$parent=''; if ($item.Parent) {{ $parent=$item.Parent.FullName }}
[Console]::Write(('{marker}',$root,$parent,'' -join [char]0))
$n=0; $bytes=0; $truncated='0'
Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction Stop | ForEach-Object {{
    $name=$_.Name
    $bytes += [Text.Encoding]::UTF8.GetByteCount($name)+3
    if ($n -lt 1000 -and $bytes -le 6000) {{
        $hidden='0'
        if ($name.StartsWith('.') -or ($_.Attributes -band [IO.FileAttributes]::Hidden)) {{ $hidden='1' }}
        [Console]::Write(($name,$hidden,'' -join [char]0))
    }} else {{ $truncated='1' }}
    $n++
}}
[Console]::Write(('END',$truncated,'' -join [char]0))"#,
            path = runtime::powershell_data(path),
        ),
    }
}

fn parse_listing(
    bytes: &[u8],
    marker: &str,
    environment: &RemoteEnvironment,
) -> Result<DirectoryListing, IpcError> {
    let invalid = || {
        runtime::error(
            IpcErrorCode::ParseError,
            "directory",
            "Malformed remote directory listing",
        )
    };
    let prefix = format!("{marker}\0");
    let start = bytes
        .windows(prefix.len())
        .position(|part| part == prefix.as_bytes())
        .ok_or_else(invalid)?;
    let fields: Vec<&[u8]> = bytes[start + prefix.len()..].split(|byte| *byte == 0).collect();
    if fields.len() < 5
        || (fields.len() - 5) % 2 != 0
        || fields.last() != Some(&b"".as_slice())
        || fields[fields.len() - 3] != b"END"
    {
        return Err(invalid());
    }
    let flag = |value: &[u8]| match value {
        b"0" => Ok(false),
        b"1" => Ok(true),
        _ => Err(invalid()),
    };
    let mut truncated = flag(fields[fields.len() - 2])?;
    let root = std::str::from_utf8(fields[0]).map_err(|_| invalid())?;
    environment.platform.validate_path(root)?;
    let parent_path = if fields[1].is_empty() {
        None
    } else {
        let parent = std::str::from_utf8(fields[1]).map_err(|_| invalid())?;
        environment.platform.validate_path(parent)?;
        Some(parent.to_string())
    };
    let mut entries = Vec::new();
    for pair in fields[2..fields.len() - 3].chunks_exact(2) {
        let hidden = flag(pair[1])?;
        let name = match std::str::from_utf8(pair[0]) {
            Ok(name) => name,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        if name.is_empty()
            || matches!(name, "." | "..")
            || name.contains('/')
            || (environment.platform == RemotePlatform::Windows && name.contains('\\'))
        {
            return Err(invalid());
        }
        let path = join_path(environment.platform, root, name);
        if environment.platform.validate_path(&path).is_err() {
            truncated = true;
            continue;
        }
        entries.push(DirectoryEntry {
            name: name.into(),
            path,
            hidden,
        });
    }
    if entries.len() > 1000 {
        return Err(invalid());
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(DirectoryListing {
        path: root.into(),
        parent_path,
        home_path: environment.home.clone(),
        entries,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::runtime::{RemoteEnvironment, RemoteExecutor, RemotePlatform};

    fn environment() -> RemoteEnvironment {
        RemoteEnvironment {
            platform: RemotePlatform::Posix,
            executor: RemoteExecutor::Sh,
            version: "test".into(),
            home: "/home/test".into(),
            temp: "/tmp".into(),
            git: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn lists_real_directories_without_files_and_preserves_literal_names() {
        // Given a real filesystem, including hidden folders and directory symlinks.
        let fixture = tempfile::tempdir().unwrap();
        for name in ["nested", ".hidden", "space ' $ & 한글"] {
            std::fs::create_dir(fixture.path().join(name)).unwrap();
        }
        std::fs::write(fixture.path().join("not-a-directory"), b"file").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(fixture.path().join("nested"), fixture.path().join("link"))
            .unwrap();
        let env = environment();
        // When the production POSIX listing script runs.
        let script = listing_script(&env, fixture.path().to_str().unwrap(), "TEST_FRAME");
        let output = std::process::Command::new("sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = parse_listing(&output.stdout, "TEST_FRAME", &env).unwrap();
        // Then only directories are returned, with exact names and canonical root.
        assert_eq!(
            result.path,
            fixture.path().canonicalize().unwrap().to_str().unwrap()
        );
        assert!(result
            .entries
            .iter()
            .any(|entry| entry.name == "space ' $ & 한글"));
        assert!(result
            .entries
            .iter()
            .any(|entry| entry.name == ".hidden" && entry.hidden));
        assert!(!result
            .entries
            .iter()
            .any(|entry| entry.name == "not-a-directory"));
        #[cfg(unix)]
        assert!(result.entries.iter().any(|entry| entry.name == "link"));
        assert!(!result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn empty_directory_succeeds_but_missing_directory_fails() {
        let fixture = tempfile::tempdir().unwrap();
        let env = environment();
        let output = std::process::Command::new("sh")
            .args([
                "-c",
                &listing_script(&env, fixture.path().to_str().unwrap(), "EMPTY"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(parse_listing(&output.stdout, "EMPTY", &env)
            .unwrap()
            .entries
            .is_empty());
        let output = std::process::Command::new("sh")
            .args([
                "-c",
                &listing_script(
                    &env,
                    fixture.path().join("missing").to_str().unwrap(),
                    "EMPTY",
                ),
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }

    #[test]
    fn resolves_home_using_remote_platform_not_local_os() {
        let mut env = environment();
        assert_eq!(resolve_path(&env, None).unwrap(), "/home/test");
        assert_eq!(
            resolve_path(&env, Some("~/project")).unwrap(),
            "/home/test/project"
        );
        assert!(resolve_path(&env, Some("relative")).is_err());
        env.platform = RemotePlatform::Windows;
        env.home = r"C:\Users\test".into();
        assert_eq!(
            resolve_path(&env, Some("~/project")).unwrap(),
            r"C:\Users\test\project"
        );
        assert!(resolve_path(&env, Some(r"\\server\share\project")).is_ok());
        assert!(resolve_path(&env, Some("/home/test")).is_err());
    }

    #[test]
    fn rejects_malformed_frames_and_reports_truncation() {
        let env = environment();
        assert!(parse_listing(b"wrong\0/home/test\0/home\0END\00\0", "FRAME", &env).is_err());
        assert!(parse_listing(
            b"FRAME\0/home/test\0/home\0../escape\00\0END\00\0",
            "FRAME",
            &env
        )
        .is_err());
        let result =
            parse_listing(b"banner\nFRAME\0/home/test\0/home\0END\01\0", "FRAME", &env).unwrap();
        assert!(result.truncated);
        assert!(result.entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn bounds_large_listings_and_distinguishes_unreadable_directories() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::tempdir().unwrap();
        for n in 0..1050 {
            std::fs::create_dir(fixture.path().join(format!("d{n:04}"))).unwrap();
        }
        let env = environment();
        let script = listing_script(&env, fixture.path().to_str().unwrap(), "BOUNDED");
        let output = std::process::Command::new("sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert!(output.status.success());
        let listing = parse_listing(&output.stdout, "BOUNDED", &env).unwrap();
        assert!(listing.truncated);
        assert!(listing.entries.len() <= 1000);
        assert!(output.stdout.len() <= 16384);
        std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0)).unwrap();
        let output = std::process::Command::new("sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!output.status.success());
    }

    #[test]
    fn decodes_windows_drive_roots_and_flags_unsupported_names() {
        let mut env = environment();
        env.platform = RemotePlatform::Windows;
        env.home = r"C:\Users\test".into();
        let frame = ["WIN", r"C:\", "", "Users", "0", "END", "0", ""].join("\0");
        let listing = parse_listing(frame.as_bytes(), "WIN", &env).unwrap();
        assert_eq!(listing.parent_path, None);
        assert_eq!(listing.entries[0].path, r"C:\Users");
        let frame = ["WIN", r"\\server\share", "", "folder", "1", "END", "0", ""].join("\0");
        let listing = parse_listing(frame.as_bytes(), "WIN", &env).unwrap();
        assert_eq!(listing.entries[0].path, r"\\server\share\folder");
        assert!(listing.entries[0].hidden);
        let frame = ["POSIX", "/home", "/", "bad\nname", "0", "END", "0", ""].join("\0");
        let listing = parse_listing(frame.as_bytes(), "POSIX", &environment()).unwrap();
        assert!(listing.entries.is_empty());
        assert!(listing.truncated);
    }

    #[test]
    fn rejects_missing_and_disabled_hosts_before_any_connection() {
        let fixture = tempfile::tempdir().unwrap();
        let store = fixture.path().join("ssh_hosts.json");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let request = || ListDirectoriesRequest {
            host_id: "disabled".into(),
            path: None,
        };
        let missing = runtime
            .block_on(list_directories(store.clone(), request()))
            .unwrap_err();
        assert_eq!(missing.details.unwrap()["reason"], "hostMissing");
        std::fs::write(
            &store,
            serde_json::to_vec(&serde_json::json!({
                "hosts": [{
                    "id": "disabled", "label": "disabled", "hostname": "never-contact.invalid",
                    "source": "manual", "authMethod": "agent", "disabled": true
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let disabled = runtime
            .block_on(list_directories(store, request()))
            .unwrap_err();
        assert_eq!(disabled.details.unwrap()["reason"], "hostDisabled");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preserves_access_to_valid_folders_beside_non_utf8_names() {
        use std::os::unix::ffi::OsStringExt;
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join("project")).unwrap();
        std::fs::create_dir(fixture.path().join(std::ffi::OsString::from_vec(vec![b'x', 0xff]))).unwrap();
        let env = environment();
        let output = std::process::Command::new("sh")
            .args(["-c", &listing_script(&env, fixture.path().to_str().unwrap(), "UTF8")])
            .output().unwrap();
        assert!(output.status.success());
        let listing = parse_listing(&output.stdout, "UTF8", &env).unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].name, "project");
        assert!(listing.truncated);
    }

    #[test]
    fn skips_non_utf8_entry_names_but_rejects_invalid_metadata() {
        let bytes = b"UTF8\0/home/test\0/home\0project\00\0bad\xff\00\0END\00\0";
        let listing = parse_listing(bytes, "UTF8", &environment()).unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].path, "/home/test/project");
        assert!(listing.truncated);
        assert!(parse_listing(b"UTF8\0/home/\xff\0/home\0END\00\0", "UTF8", &environment()).is_err());
        assert!(parse_listing(b"UTF8\0/home/test\0/home\0bad\xff\0x\0END\00\0", "UTF8", &environment()).is_err());
    }
}
