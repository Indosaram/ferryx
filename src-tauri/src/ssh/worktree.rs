use super::runtime::{
    parse_fields, powershell_data, RemoteEnvironment, RemotePlatform, POWERSHELL_GIT,
};
use super::{direct, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::worktree::WorktreeIdentity;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
}

pub fn parse_worktree_porcelain(output: &str) -> Vec<RemoteWorktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<RemoteWorktree> = None;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(entry) = current.take() {
                worktrees.push(entry);
            }
            continue;
        }
        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            None => (line, None),
        };
        let entry = match key {
            "worktree" => {
                let Some(path) = value else { continue };
                current.get_or_insert_with(|| RemoteWorktree {
                    path: path.to_string(),
                    head: None,
                    branch: None,
                    bare: false,
                    detached: false,
                })
            }
            _ => match current.as_mut() {
                Some(entry) => entry,
                None => continue,
            },
        };
        match (key, value) {
            ("HEAD", Some(value)) => entry.head = Some(value.to_string()),
            ("branch", Some(value)) => {
                entry.branch = Some(value.trim_start_matches("refs/heads/").to_string())
            }
            ("bare", _) => entry.bare = true,
            ("detached", _) => entry.detached = true,
            _ => {}
        }
    }
    if let Some(entry) = current.take() {
        worktrees.push(entry);
    }
    worktrees
}

/// Derives a sanitized worktree branch namespace segment from a remote workspace ID (`ssh:<hex>`).
/// Takes up to 12 leading hex characters and formats as `ssh-{segment}`.
pub fn derive_ws_segment(workspace_id: &str) -> Result<String, IpcError> {
    let hex = workspace_id
        .strip_prefix(crate::ssh::projects::REMOTE_PREFIX)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::InvalidNamespace,
                format!(
                    "Remote workspace ID must start with '{}'",
                    crate::ssh::projects::REMOTE_PREFIX
                ),
            )
        })?;
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidNamespace,
            "Remote workspace ID contains invalid hex characters",
        ));
    }
    let segment = if hex.len() > 12 { &hex[..12] } else { hex };
    Ok(format!("ssh-{segment}"))
}

pub fn remote_worktree_path(platform: RemotePlatform, repo_root: &str, slug: &str) -> String {
    match platform {
        RemotePlatform::Posix => {
            let root = repo_root.trim_end_matches('/');
            format!("{root}/.orca-worktrees/wt-{slug}")
        }
        RemotePlatform::Windows => {
            let root = repo_root.trim_end_matches(['/', '\\']);
            format!("{root}\\.orca-worktrees\\wt-{slug}")
        }
    }
}

pub fn validate_base_ref(base_ref: Option<&str>) -> Result<(), IpcError> {
    if let Some(base) = base_ref {
        if base.starts_with('-') {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "base_ref must not start with '-'",
            ));
        }
    }
    Ok(())
}

pub fn validate_path_inside_root(
    platform: RemotePlatform,
    root: &str,
    path: &str,
) -> Result<(), IpcError> {
    match platform {
        RemotePlatform::Posix => {
            let norm_root = root.trim_end_matches('/');
            let norm_path = path.trim_end_matches('/');
            if norm_path.split('/').any(|seg| seg == "..")
                || !norm_path.starts_with(&format!("{norm_root}/"))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Worktree path must be inside the project repository root",
                ));
            }
        }
        RemotePlatform::Windows => {
            let norm_root = root.replace('\\', "/").trim_end_matches('/').to_lowercase();
            let norm_path = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
            if norm_path.split('/').any(|seg| seg == "..")
                || !norm_path.starts_with(&format!("{norm_root}/"))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Worktree path must be inside the project repository root",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_cwd_inside_root(
    platform: RemotePlatform,
    root: &str,
    path: &str,
) -> Result<(), IpcError> {
    match platform {
        RemotePlatform::Posix => {
            let norm_root = root.trim_end_matches('/');
            let norm_path = path.trim_end_matches('/');
            if norm_path.split('/').any(|seg| seg == "..")
                || (norm_path != norm_root && !norm_path.starts_with(&format!("{norm_root}/")))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Working directory must be inside the project repository root",
                ));
            }
        }
        RemotePlatform::Windows => {
            let norm_root = root.replace('\\', "/").trim_end_matches('/').to_lowercase();
            let norm_path = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
            if norm_path.split('/').any(|seg| seg == "..")
                || (norm_path != norm_root && !norm_path.starts_with(&format!("{norm_root}/")))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Working directory must be inside the project repository root",
                ));
            }
        }
    }
    Ok(())
}

pub fn resolve_remote_spawn_root(
    platform: RemotePlatform,
    repo_root: &str,
    worktree: Option<&WorktreeIdentity>,
    cwd: Option<&str>,
) -> Result<String, IpcError> {
    if let Some(cwd) = cwd {
        validate_cwd_inside_root(platform, repo_root, cwd)?;
        let is_root = match platform {
            RemotePlatform::Posix => cwd.trim_end_matches('/') == repo_root.trim_end_matches('/'),
            RemotePlatform::Windows => {
                cwd.replace('\\', "/").trim_end_matches('/').to_lowercase()
                    == repo_root.replace('\\', "/").trim_end_matches('/').to_lowercase()
            }
        };
        if is_root {
            Ok(repo_root.to_string())
        } else {
            Ok(cwd.to_string())
        }
    } else if let Some(identity) = worktree {
        if identity.slug.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Worktree slug cannot be empty",
            ));
        }
        if identity.slug.contains("..") {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                "Worktree slug must not contain '..'",
            ));
        }
        let path = remote_worktree_path(platform, repo_root, &identity.slug);
        validate_path_inside_root(platform, repo_root, &path)?;
        Ok(path)
    } else {
        Ok(repo_root.to_string())
    }
}

pub fn worktree_list_script(platform: RemotePlatform, repo_root: &str, marker: &str) -> String {
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            format!(
                "out=$(git -C {quoted_root} worktree list --porcelain 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '%s\\n' \"$out\" >&2; exit $rc; }}; printf '{marker}\\000%s\\000' \"$out\""
            )
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            format!(
                "{POWERSHELL_GIT}\n$p={p}; \
                 $g=Invoke-FerryxGit @('-C',$p,'worktree','list','--porcelain'); \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree list failed' }}; throw $err }}; \
                 [Console]::Write(('{marker}',$g.Output,'' -join [char]0))"
            )
        }
    }
}

pub fn worktree_create_script(
    platform: RemotePlatform,
    repo_root: &str,
    branch: &str,
    path: &str,
    base_ref: Option<&str>,
) -> String {
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            let quoted_branch = direct::quote_posix(branch);
            let quoted_path = direct::quote_posix(path);
            let quoted_base = match base_ref {
                Some(base) => format!(" {}", direct::quote_posix(base)),
                None => String::new(),
            };
            format!(
                "out=$(git -C {quoted_root} worktree add -b {quoted_branch} {quoted_path}{quoted_base} 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '%s\\n' \"$out\" >&2; exit $rc; }}"
            )
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            let wt = powershell_data(path);
            let br = powershell_data(branch);
            let base_code = match base_ref {
                Some(base) => format!(" $args += {};", powershell_data(base)),
                None => String::new(),
            };
            format!(
                "{POWERSHELL_GIT}\n$p={p}; $wt={wt}; $br={br}; \
                 $args=@('-C',$p,'worktree','add','-b',$br,$wt);{base_code} \
                 $g=Invoke-FerryxGit $args; \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree add failed' }}; throw $err }}"
            )
        }
    }
}

pub fn worktree_remove_script(platform: RemotePlatform, repo_root: &str, path: &str) -> String {
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            let quoted_path = direct::quote_posix(path);
            format!(
                "out=$(git -C {quoted_root} worktree remove {quoted_path} 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '%s\\n' \"$out\" >&2; exit $rc; }}"
            )
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            let wt = powershell_data(path);
            format!(
                "{POWERSHELL_GIT}\n$p={p}; $wt={wt}; \
                 $g=Invoke-FerryxGit @('-C',$p,'worktree','remove',$wt); \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree remove failed' }}; throw $err }}"
            )
        }
    }
}

fn tag_error(mut err: IpcError, stage: &'static str) -> IpcError {
    match err.details.as_mut() {
        Some(details) => {
            details["stage"] = stage.into();
        }
        None => {
            err = err.with_details(serde_json::json!({"stage": stage}));
        }
    }
    err
}

pub async fn list_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
) -> Result<Vec<RemoteWorktree>, IpcError> {
    environment.platform.validate_path(repo_root)?;
    let marker = format!("FERRYX_WT_LIST_V1_{}", uuid::Uuid::new_v4().simple());
    let script = worktree_list_script(environment.platform, repo_root, &marker);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(30))
        .await
        .map_err(|err| tag_error(err, "worktree-list"))?;
    let fields = parse_fields(&output, &marker, 1).map_err(|err| tag_error(err, "worktree-list"))?;
    Ok(parse_worktree_porcelain(fields[0]))
}

pub async fn create_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
    ws_segment: &str,
    slug: &str,
    base_ref: Option<&str>,
    path: &str,
) -> Result<(), IpcError> {
    validate_base_ref(base_ref)?;
    environment.platform.validate_path(repo_root)?;
    environment.platform.validate_path(path)?;
    let branch = crate::worktree::manager::WorktreeManager::format_branch_name(ws_segment, slug)
        .map_err(|err| IpcError::new(IpcErrorCode::InvalidNamespace, err.to_string()))?;
    let script = worktree_create_script(environment.platform, repo_root, &branch, path, base_ref);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    direct::bounded_output(&plan, Duration::from_secs(30))
        .await
        .map_err(|err| tag_error(err, "worktree-create"))?;
    Ok(())
}

pub async fn remove_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
    path: &str,
) -> Result<(), IpcError> {
    environment.platform.validate_path(repo_root)?;
    environment.platform.validate_path(path)?;
    let script = worktree_remove_script(environment.platform, repo_root, path);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    direct::bounded_output(&plan, Duration::from_secs(30))
        .await
        .map_err(|err| tag_error(err, "worktree-remove"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::manager::WorktreeManager;

    #[test]
    fn red_parse_porcelain_entries() {
        let output = "worktree /home/sook/repo\nHEAD abc1234\nbranch refs/heads/main\n\nworktree /home/sook/repo/.orca-worktrees/wt-feat\nHEAD def5678\nbranch refs/heads/orca/ws1/wt-feat\n\nworktree /tmp/detached-wt\ndetached\n\n";
        let parsed = parse_worktree_porcelain(output);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert_eq!(parsed[1].branch.as_deref(), Some("orca/ws1/wt-feat"));
        assert!(parsed[2].detached);
        assert!(!parsed[0].bare);
    }

    #[test]
    fn red_malformed_porcelain_tolerated() {
        let parsed = parse_worktree_porcelain(
            "noworktreekey here\n\nworktree /only/path\n\nworktree /b\nbare\n",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].path, "/only/path");
        assert!(parsed[1].bare);
    }

    #[test]
    fn red_list_script_marker_framed_both_platforms() {
        let posix = worktree_list_script(RemotePlatform::Posix, "/home/user/repo", "MARKER_LIST");
        assert!(posix.contains("git -C '/home/user/repo' worktree list --porcelain"));
        assert!(posix.contains("MARKER_LIST\\000%s\\000"));

        let windows = worktree_list_script(RemotePlatform::Windows, r"C:\Users\sook\repo", "MARKER_WIN");
        assert!(windows.contains("Invoke-FerryxGit"));
        assert!(windows.contains("'worktree','list','--porcelain'"));
        assert!(windows.contains("MARKER_WIN"));
    }

    #[test]
    fn red_create_script_quoting_and_sanitized_branch() {
        let raw_ws_id = "ssh:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let ws_segment = derive_ws_segment(raw_ws_id).expect("valid segment");
        assert_eq!(ws_segment, "ssh-0123456789ab");

        let branch = WorktreeManager::format_branch_name(&ws_segment, "my-feature").expect("valid branch");
        assert_eq!(branch, "orca/ssh-0123456789ab/my-feature");

        // Posix create script with base_ref
        let posix = worktree_create_script(
            RemotePlatform::Posix,
            "/home/user/repo with spaces",
            &branch,
            "/home/user/repo with spaces/.orca-worktrees/wt-my-feature",
            Some("origin/main"),
        );
        // The raw hash with colon must never reach the script
        assert!(!posix.contains("ssh:0123456789ab"));
        assert!(posix.contains("orca/ssh-0123456789ab/my-feature"));
        assert!(posix.contains("'/home/user/repo with spaces'"));
        assert!(posix.contains("'/home/user/repo with spaces/.orca-worktrees/wt-my-feature'"));
        assert!(posix.contains("'origin/main'"));

        // Windows create script without base_ref
        let win = worktree_create_script(
            RemotePlatform::Windows,
            r"C:\Users\sook\repo",
            &branch,
            r"C:\Users\sook\repo\.orca-worktrees\wt-my-feature",
            None,
        );
        assert!(!win.contains("ssh:0123456789ab"));
        assert!(win.contains("Invoke-FerryxGit"));
    }

    #[test]
    fn red_derive_ws_segment_malformed_rejected() {
        assert!(derive_ws_segment("not-ssh:1234").is_err());
        assert!(derive_ws_segment("ssh:").is_err());
        assert!(derive_ws_segment("ssh:bad..hex").is_err());
        assert!(derive_ws_segment("ssh:0123456789abcdef").is_ok());
        assert_eq!(
            derive_ws_segment("ssh:0123456789abcdef").unwrap(),
            "ssh-0123456789ab"
        );
        assert_eq!(derive_ws_segment("ssh:abc").unwrap(), "ssh-abc");
    }

    #[test]
    fn red_path_inside_root_guard() {
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/home/user/repo/.orca-worktrees/wt-1").is_ok());
        assert!(validate_path_inside_root(RemotePlatform::Windows, r"C:\Users\sook\repo", r"C:\Users\sook\repo\.orca-worktrees\wt-1").is_ok());
        assert!(validate_path_inside_root(RemotePlatform::Windows, r"C:/Users/sook/repo", r"C:\Users\sook\repo\.orca-worktrees\wt-1").is_ok());

        // Repo root itself cannot be deleted
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/home/user/repo").is_err());
        // Sibling folder cannot be deleted
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/home/user/repo-other/wt").is_err());
        // Traversal cannot be deleted
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/home/user/repo/../secret").is_err());
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/etc/passwd").is_err());

        // Fix 1 Regression tests:
        // Posix: no backslash translation
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", r"/srv/repo\outside").is_err());
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", "/srv/repo-other/x").is_err());
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", "/srv/repo/.orca-worktrees/wt-1").is_ok());
        assert!(validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", "/srv/repo").is_err());
        // Windows: case and separator insensitive, traversal rejected
        assert!(validate_path_inside_root(RemotePlatform::Windows, r"C:\Repo", r"c:\repo\.orca-worktrees\wt-1").is_ok());
        assert!(validate_path_inside_root(RemotePlatform::Windows, r"C:\Repo", r"C:\Repo\..\secret").is_err());
    }

    #[test]
    fn red_create_remote_rejects_base_ref_option_injection() {
        assert_eq!(
            validate_base_ref(Some("--no-checkout")).unwrap_err().code,
            IpcErrorCode::InvalidArgument
        );
        assert!(validate_base_ref(Some("origin/main")).is_ok());
        assert!(validate_base_ref(None).is_ok());
    }

    #[tokio::test]
    async fn red_create_remote_option_injection_async_guard() {
        use crate::ssh::runtime::RemoteExecutor;
        use crate::ssh::{SshAuthMethod, SshHostSource};

        let host = SshHost {
            id: "h1".into(),
            label: "test".into(),
            hostname: "localhost".into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Config,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
        };
        let env = RemoteEnvironment {
            platform: RemotePlatform::Posix,
            executor: RemoteExecutor::Sh,
            version: "test".into(),
            home: "/home/user".into(),
            temp: "/tmp".into(),
            git: true,
        };
        let err = create_remote(
            &host,
            &env,
            "/srv/repo",
            "ssh-abc",
            "wt-1",
            Some("--no-checkout"),
            "/srv/repo/.orca-worktrees/wt-1",
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
    }

    #[test]
    fn red_posix_script_builders_executable_behavior() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo_root_path = dir.path().canonicalize().expect("canonicalize");
        let repo_root = repo_root_path.to_str().expect("repo root str");

        let init_status = std::process::Command::new("git")
            .args(["init", repo_root])
            .status()
            .expect("git init");
        assert!(init_status.success());

        let name_status = std::process::Command::new("git")
            .args(["-C", repo_root, "config", "user.name", "test"])
            .status()
            .expect("git config user.name");
        assert!(name_status.success());

        let email_status = std::process::Command::new("git")
            .args(["-C", repo_root, "config", "user.email", "test@example.com"])
            .status()
            .expect("git config user.email");
        assert!(email_status.success());

        let commit_status = std::process::Command::new("git")
            .args(["-C", repo_root, "commit", "--allow-empty", "-m", "init"])
            .status()
            .expect("git commit");
        assert!(commit_status.success());

        let ws_segment = derive_ws_segment("ssh:0123456789abcdef").expect("valid segment");
        let branch = WorktreeManager::format_branch_name(&ws_segment, "feat-test").expect("valid branch");
        let wt_path = format!("{repo_root}/.orca-worktrees/wt-feat-test");

        let create_script = worktree_create_script(
            RemotePlatform::Posix,
            repo_root,
            &branch,
            &wt_path,
            None,
        );

        let create_output = std::process::Command::new("bash")
            .args(["-c", &create_script])
            .output()
            .expect("run create_script");
        assert!(
            create_output.status.success(),
            "create_script failed: {}",
            String::from_utf8_lossy(&create_output.stderr)
        );

        let marker = format!("FERRYX_WT_LIST_V1_{}", uuid::Uuid::new_v4().simple());
        let list_script = worktree_list_script(RemotePlatform::Posix, repo_root, &marker);

        let list_output = std::process::Command::new("bash")
            .args(["-c", &list_script])
            .output()
            .expect("run list_script");
        assert!(
            list_output.status.success(),
            "list_script failed: {}",
            String::from_utf8_lossy(&list_output.stderr)
        );

        let fields = parse_fields(&list_output.stdout, &marker, 1).expect("parse_fields");
        let worktrees = parse_worktree_porcelain(fields[0]);

        let created_wt = worktrees
            .iter()
            .find(|wt| wt.path == wt_path)
            .expect("created worktree in porcelain listing");
        assert_eq!(created_wt.branch.as_deref(), Some("orca/ssh-0123456789ab/feat-test"));
    }

    #[test]
    fn red_porcelain_parsing_of_created_output() {
        let output = "worktree /srv/repo/.orca-worktrees/wt-new\nHEAD e4d8c2\nbranch refs/heads/orca/ssh-abc/new\n\n";
        let parsed = parse_worktree_porcelain(output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "/srv/repo/.orca-worktrees/wt-new");
        assert_eq!(parsed[0].head.as_deref(), Some("e4d8c2"));
        assert_eq!(parsed[0].branch.as_deref(), Some("orca/ssh-abc/new"));
        assert!(!parsed[0].bare);
        assert!(!parsed[0].detached);
    }

    #[test]
    fn test_resolve_remote_spawn_root() {
        // cwd inside root ok
        let res = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo/.orca-worktrees/wt-1"),
        )
        .unwrap();
        assert_eq!(res, "/srv/repo/.orca-worktrees/wt-1");

        // cwd outside (posix backslash-sibling /srv/repo\outside) rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some(r"/srv/repo\outside"),
        )
        .is_err());

        // cwd outside (/srv/repo-other/x) rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo-other/x"),
        )
        .is_err());

        // .. segment rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo/../outside"),
        )
        .is_err());
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            None,
            Some(r"C:\Repo\..\outside"),
        )
        .is_err());

        // worktree-derived path equals <root>/.orca-worktrees/wt-<slug> (posix)
        let wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "feature-1".into(),
        };
        let res_posix = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&wt),
            None,
        )
        .unwrap();
        assert_eq!(res_posix, "/srv/repo/.orca-worktrees/wt-feature-1");

        // backslash variant (windows)
        let res_win = resolve_remote_spawn_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            Some(&wt),
            None,
        )
        .unwrap();
        assert_eq!(res_win, r"C:\Repo\.orca-worktrees\wt-feature-1");

        // empty slug rejected with IpcErrorCode::InvalidArgument
        let empty_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "".into(),
        };
        let err = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&empty_wt),
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);

        // slug containing .. rejected
        let dotdot_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "../escape".into(),
        };
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&dotdot_wt),
            None,
        )
        .is_err());

        let dotdot_mid_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "sub/../escape".into(),
        };
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&dotdot_mid_wt),
            None,
        )
        .is_err());

        // none -> repo_root
        let res_none = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            None,
        )
        .unwrap();
        assert_eq!(res_none, "/srv/repo");

        // cwd equal to repo_root ok (posix and windows)
        let res_root_posix = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo"),
        )
        .unwrap();
        assert_eq!(res_root_posix, "/srv/repo");

        let res_root_slash = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo/"),
        )
        .unwrap();
        assert_eq!(res_root_slash, "/srv/repo");

        let res_root_win = resolve_remote_spawn_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            None,
            Some(r"C:\Repo"),
        )
        .unwrap();
        assert_eq!(res_root_win, r"C:\Repo");

        let res_root_win_ci = resolve_remote_spawn_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            None,
            Some(r"c:\repo\"),
        )
        .unwrap();
        assert_eq!(res_root_win_ci, r"C:\Repo");
    }
}
