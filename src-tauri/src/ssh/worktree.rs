use serde::{Deserialize, Serialize};

use super::SshHost;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

pub fn remote_list_argv(host: &SshHost) -> Vec<String> {
    let mut argv = vec!["ssh".to_string()];
    argv.push(host.target());
    argv.push(git_command(host, "worktree list --porcelain"));
    argv
}

/// Scopes git at the configured repo root; running bare `git worktree …`
/// from the ssh HOME would fail with "not a git repository".
fn git_command(host: &SshHost, git_args: &str) -> String {
    match host.repo_root.as_deref() {
        Some(root) if !root.trim().is_empty() => format!("git -C {root} {git_args}"),
        _ => format!("git {git_args}"),
    }
}

pub fn remote_add_argv(
    host: &SshHost,
    path: &str,
    ws_id: &str,
    slug: &str,
    base: Option<&str>,
) -> Result<Vec<String>, String> {
    let branch = crate::worktree::manager::WorktreeManager::format_branch_name(ws_id, slug)
        .map_err(|worktree_error| worktree_error.to_string())?;
    let mut argv = vec!["ssh".to_string()];
    argv.push(host.target());
    let mut command = git_command(host, &format!("worktree add -b {branch} {path}"));
    if let Some(base_ref) = base {
        command.push(' ');
        command.push_str(base_ref);
    }
    argv.push(command);
    Ok(argv)
}

pub fn remote_remove_argv(host: &SshHost, path: &str) -> Vec<String> {
    vec![
        "ssh".to_string(),
        host.target(),
        git_command(host, &format!("worktree remove {path}")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::SshAuthMethod;
    use crate::ssh::SshHostSource;

    fn host() -> SshHost {
        SshHost {
            id: "h".into(),
            label: "l".into(),
            hostname: "maho-win".into(),
            username: Some("sook".into()),
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Config,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            repo_root: None,
            remote_continuity: crate::ssh::RemoteContinuity::Auto,
        }
    }

    #[test]
    fn red_repo_root_scopes_git_commands() {
        let mut scoped = host();
        scoped.repo_root = Some("/srv/clone".into());
        assert_eq!(
            remote_list_argv(&scoped).last().map(String::as_str),
            Some("git -C /srv/clone worktree list --porcelain")
        );
        let add = remote_add_argv(&scoped, "/srv/clone/.orca-worktrees/wt-a", "ws1", "a", None)
            .expect("valid");
        assert_eq!(
            add.last().map(String::as_str),
            Some("git -C /srv/clone worktree add -b orca/ws1/a /srv/clone/.orca-worktrees/wt-a")
        );
        let remove = remote_remove_argv(&scoped, "/srv/clone/.orca-worktrees/wt-a");
        assert_eq!(
            remove.last().map(String::as_str),
            Some("git -C /srv/clone worktree remove /srv/clone/.orca-worktrees/wt-a")
        );
    }

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
    fn red_remote_argv_shapes() {
        assert_eq!(
            remote_list_argv(&host()),
            vec!["ssh", "sook@maho-win", "git worktree list --porcelain"]
        );
        let add = remote_add_argv(&host(), "/srv/wt", "ws1", "slug-a", Some("origin/main"))
            .expect("valid");
        assert_eq!(
            add,
            vec![
                "ssh",
                "sook@maho-win",
                "git worktree add -b orca/ws1/slug-a /srv/wt origin/main"
            ]
        );
        let no_base = remote_add_argv(&host(), "/srv/wt2", "ws1", "slug-b", None).expect("valid");
        assert_eq!(
            no_base.last().map(String::as_str),
            Some("git worktree add -b orca/ws1/slug-b /srv/wt2")
        );
        assert_eq!(
            remote_remove_argv(&host(), "/srv/wt"),
            vec!["ssh", "sook@maho-win", "git worktree remove /srv/wt"]
        );
    }

    #[test]
    fn red_invalid_slug_rejected() {
        assert!(remote_add_argv(&host(), "/srv/wt", "ws1", "bad..slug", None).is_err());
    }
}
