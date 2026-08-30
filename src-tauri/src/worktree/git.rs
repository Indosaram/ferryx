use crate::worktree::model::{DirtyFile, DirtyState, Worktree, WorktreeError};
use std::path::{Path, PathBuf};

fn escape_git_arg_for_log(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len());
    for ch in arg.chars() {
        match ch {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(&mut escaped, "\\u{{{:x}}}", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}
fn validate_git_value(value: &str, label: &str) -> Result<(), WorktreeError> {
    if value.is_empty() {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot be empty"),
        });
    }
    if value.starts_with('-') {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot start with a dash"),
        });
    }
    if value.chars().any(char::is_control) {
        return Err(WorktreeError::InvalidNamespace {
            reason: format!("{label} cannot contain control characters"),
        });
    }
    Ok(())
}

pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else if let Some(stripped) = path.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else if let Some(stripped) = path.strip_prefix("//?/") {
        stripped.to_string()
    } else if let Some(stripped) = path.strip_prefix(r"\\.\") {
        stripped.to_string()
    } else {
        path.to_string()
    }
}

pub fn normalize_path_for_git(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        PathBuf::from(strip_verbatim_prefix(path_str))
    } else {
        path.to_path_buf()
    }
}

fn validate_git_path_argument(path: &Path) -> Result<String, WorktreeError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| WorktreeError::ParseError("Invalid UTF-8 in worktree path".into()))?;
    let normalized = strip_verbatim_prefix(path_str);
    if normalized.starts_with('-') {
        return Err(WorktreeError::InvalidPath {
            path: path.to_path_buf(),
            reason: "Git path argument cannot start with a dash".into(),
        });
    }
    if normalized.chars().any(char::is_control) {
        return Err(WorktreeError::InvalidPath {
            path: path.to_path_buf(),
            reason: "Git path argument cannot contain control characters".into(),
        });
    }
    Ok(normalized)
}

fn validate_base_ref(repo_root: &Path, base_ref: &str) -> Result<(), WorktreeError> {
    validate_git_value(base_ref, "Base ref")?;
    let commit_ref = format!("{base_ref}^{{commit}}");
    run_git(
        repo_root,
        &["rev-parse", "--verify", "--quiet", commit_ref.as_str()],
    )?;
    Ok(())
}

/// Executes a git command in the specified directory.
pub fn run_git<P: AsRef<Path>, S: AsRef<str>>(cwd: P, args: &[S]) -> Result<String, WorktreeError> {
    let cwd_path = cwd.as_ref();
    let normalized_cwd = normalize_path_for_git(cwd_path);
    let arg_strs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
    let command_str = format!(
        "git {}",
        arg_strs
            .iter()
            .map(|arg| escape_git_arg_for_log(arg))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let output = crate::util::no_window_command("git")
        .args(&arg_strs)
        .current_dir(&normalized_cwd)
        .output()
        .map_err(WorktreeError::Io)?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(WorktreeError::GitError {
            command: command_str,
            stderr: stderr.trim().to_string(),
            stdout: stdout.trim().to_string(),
            code: output.status.code(),
        });
    }

    Ok(stdout)
}

/// Parses the output of `git worktree list --porcelain`.
pub fn parse_worktree_list_porcelain(output: &str) -> Result<Vec<Worktree>, WorktreeError> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_head = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_detached = false;
    let mut locked_reason: Option<String> = None;
    let mut prunable_reason: Option<String> = None;

    let flush = |worktrees: &mut Vec<Worktree>,
                 path: &mut Option<PathBuf>,
                 head: &mut String,
                 branch: &mut Option<String>,
                 bare: &mut bool,
                 detached: &mut bool,
                 locked: &mut Option<String>,
                 prunable: &mut Option<String>| {
        if let Some(p) = path.take() {
            worktrees.push(Worktree {
                path: p,
                head: std::mem::take(head),
                branch: branch.take(),
                bare: *bare,
                detached: *detached,
                locked: locked.take(),
                prunable: prunable.take(),
            });
            *bare = false;
            *detached = false;
        }
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(
                &mut worktrees,
                &mut current_path,
                &mut current_head,
                &mut current_branch,
                &mut is_bare,
                &mut is_detached,
                &mut locked_reason,
                &mut prunable_reason,
            );
            continue;
        }

        if let Some(path_str) = trimmed.strip_prefix("worktree ") {
            if current_path.is_some() {
                flush(
                    &mut worktrees,
                    &mut current_path,
                    &mut current_head,
                    &mut current_branch,
                    &mut is_bare,
                    &mut is_detached,
                    &mut locked_reason,
                    &mut prunable_reason,
                );
            }
            current_path = Some(PathBuf::from(path_str.trim()));
        } else if let Some(head_str) = trimmed.strip_prefix("HEAD ") {
            current_head = head_str.trim().to_string();
        } else if let Some(branch_str) = trimmed.strip_prefix("branch ") {
            current_branch = Some(branch_str.trim().to_string());
        } else if trimmed == "bare" {
            is_bare = true;
        } else if trimmed == "detached" {
            is_detached = true;
        } else if let Some(reason) = trimmed.strip_prefix("locked") {
            let reason_str = reason.trim();
            locked_reason = Some(if reason_str.is_empty() {
                "locked".to_string()
            } else {
                reason_str.to_string()
            });
        } else if let Some(reason) = trimmed.strip_prefix("prunable") {
            let reason_str = reason.trim();
            prunable_reason = Some(if reason_str.is_empty() {
                "prunable".to_string()
            } else {
                reason_str.to_string()
            });
        }
    }

    flush(
        &mut worktrees,
        &mut current_path,
        &mut current_head,
        &mut current_branch,
        &mut is_bare,
        &mut is_detached,
        &mut locked_reason,
        &mut prunable_reason,
    );

    Ok(worktrees)
}

/// Parses output from `git status --porcelain` or `git status --porcelain=v2`.
pub fn parse_status_porcelain(output: &str) -> DirtyState {
    let mut files = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("1 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 8 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[7..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("2 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 9 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[8..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("u ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 10 {
                files.push(DirtyFile {
                    status_code: parts[0].to_string(),
                    path: parts[9..].join(" "),
                });
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("? ") {
            files.push(DirtyFile {
                status_code: "?".to_string(),
                path: rest.trim().to_string(),
            });
            continue;
        }

        if trimmed.starts_with("! ") {
            continue;
        }

        if line.len() >= 3 {
            let status_code = line[..2].to_string();
            let path = line[3..].trim().to_string();
            if !path.is_empty() {
                files.push(DirtyFile { status_code, path });
            }
        }
    }

    if files.is_empty() {
        DirtyState::clean()
    } else {
        DirtyState::dirty(files)
    }
}

pub fn git_worktree_add(
    repo_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
) -> Result<(), WorktreeError> {
    validate_git_value(branch_name, "Branch name")?;
    let path_str = validate_git_path_argument(worktree_path)?;
    if let Some(base) = base_ref {
        validate_base_ref(repo_root, base)?;
    }

    let mut args = vec!["worktree", "add", "-b", branch_name, "--", path_str.as_str()];
    if let Some(base) = base_ref {
        args.push(base);
    }
    run_git(repo_root, &args)?;
    Ok(())
}

pub fn git_worktree_list(repo_root: &Path) -> Result<Vec<Worktree>, WorktreeError> {
    let output = run_git(repo_root, &["worktree", "list", "--porcelain"])?;
    parse_worktree_list_porcelain(&output)
}

/// Inspect a single worktree directory without listing every worktree in the repo.
pub fn inspect_worktree(path: &Path) -> Result<Worktree, WorktreeError> {
    let head = run_git(path, &["rev-parse", "HEAD"])?.trim().to_string();
    let branch = run_git(path, &["symbolic-ref", "-q", "HEAD"])
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|value| !value.is_empty());
    let detached = branch.is_none();
    Ok(Worktree {
        path: path.to_path_buf(),
        head,
        branch,
        bare: false,
        detached,
        locked: None,
        prunable: None,
    })
}

/// Returns true when `ancestor` is an ancestor of `descendant` via `git merge-base --is-ancestor`.
pub fn git_merge_base_is_ancestor(
    repo_root: &Path,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, WorktreeError> {
    validate_git_value(ancestor, "Ancestor ref")?;
    validate_git_value(descendant, "Descendant ref")?;
    match run_git(
        repo_root,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    ) {
        Ok(_) => Ok(true),
        Err(WorktreeError::GitError { code: Some(1), .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Returns true when `branch` is an ancestor of HEAD (merged).
pub fn git_branch_is_ancestor_of_head(
    repo_root: &Path,
    branch: &str,
) -> Result<bool, WorktreeError> {
    git_merge_base_is_ancestor(repo_root, branch, "HEAD")
}

pub fn git_worktree_remove(
    repo_root: &Path,
    worktree_path: &Path,
    force: bool,
) -> Result<(), WorktreeError> {
    let path_str = validate_git_path_argument(worktree_path)?;
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path_str.as_str()]);
    run_git(repo_root, &args)?;
    Ok(())
}

pub fn git_worktree_prune(repo_root: &Path) -> Result<(), WorktreeError> {
    run_git(repo_root, &["worktree", "prune"])?;
    Ok(())
}

pub fn git_status_porcelain(worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
    let output = run_git(worktree_path, &["status", "--porcelain"])?;
    Ok(parse_status_porcelain(&output))
}

pub fn git_branch_delete(
    repo_root: &Path,
    branch_name: &str,
    force: bool,
) -> Result<(), WorktreeError> {
    validate_git_value(branch_name, "Branch name")?;
    let flag = if force { "-D" } else { "-d" };
    run_git(repo_root, &["branch", flag, "--", branch_name])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_path_argument_normalization_strips_windows_verbatim_prefixes() {
        assert_eq!(
            validate_git_path_argument(Path::new(r"\\?\C:\Users\orca\repo\.orca-worktrees\ws\task")).unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new("//?/C:/Users/orca/repo/.orca-worktrees/ws/task")).unwrap(),
            "C:/Users/orca/repo/.orca-worktrees/ws/task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(r"\\?\UNC\server\share\repo\.orca-worktrees\ws\task")).unwrap(),
            r"\\server\share\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new("//?/UNC/server/share/repo/.orca-worktrees/ws/task")).unwrap(),
            "//server/share/repo/.orca-worktrees/ws/task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(r"\\.\C:\Users\orca\repo\.orca-worktrees\ws\task")).unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new(r"C:\Users\orca\repo\.orca-worktrees\ws\task")).unwrap(),
            r"C:\Users\orca\repo\.orca-worktrees\ws\task"
        );
        assert_eq!(
            validate_git_path_argument(Path::new("/Users/orca/repo/.orca-worktrees/ws/task")).unwrap(),
            "/Users/orca/repo/.orca-worktrees/ws/task"
        );
        assert!(validate_git_path_argument(Path::new("-bad-path")).is_err());
        assert!(validate_git_path_argument(Path::new("bad\npath")).is_err());
    }
}
