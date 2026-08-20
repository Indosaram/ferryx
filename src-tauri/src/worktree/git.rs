use crate::worktree::model::{DirtyFile, DirtyState, Worktree, WorktreeError};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Executes a git command in the specified directory.
pub fn run_git<P: AsRef<Path>, S: AsRef<str>>(
    cwd: P,
    args: &[S],
) -> Result<String, WorktreeError> {
    let cwd_path = cwd.as_ref();
    let arg_strs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
    let command_str = format!("git {}", arg_strs.join(" "));

    let output = Command::new("git")
        .args(&arg_strs)
        .current_dir(cwd_path)
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
            // Ignore blank lines and branch header comments in porcelain format
            continue;
        }

        // Porcelain v2 ordinary changed entry: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
        if let Some(rest) = trimmed.strip_prefix("1 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 8 {
                let status_code = parts[0].to_string();
                let path = parts[7..].join(" ");
                files.push(DirtyFile { status_code, path });
                continue;
            }
        }

        // Porcelain v2 renamed/copied entry: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path><sep><origPath>
        if let Some(rest) = trimmed.strip_prefix("2 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 9 {
                let status_code = parts[0].to_string();
                let path = parts[8..].join(" ");
                files.push(DirtyFile { status_code, path });
                continue;
            }
        }

        // Porcelain v2 unmerged entry: u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
        if let Some(rest) = trimmed.strip_prefix("u ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 10 {
                let status_code = parts[0].to_string();
                let path = parts[9..].join(" ");
                files.push(DirtyFile { status_code, path });
                continue;
            }
        }

        // Porcelain v2 untracked entry: ? <path>
        if let Some(rest) = trimmed.strip_prefix("? ") {
            files.push(DirtyFile {
                status_code: "?".to_string(),
                path: rest.trim().to_string(),
            });
            continue;
        }

        // Porcelain v2 ignored entry: ! <path> (ignored files don't make it dirty by themselves, but let's check prefix)
        if trimmed.starts_with("! ") {
            continue;
        }

        // Porcelain v1 format: XY PATH (where XY is 2 characters)
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

/// Creates a worktree with the specified branch name and optional base commit/ref.
pub fn git_worktree_add(
    repo_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
) -> Result<(), WorktreeError> {
    let path_str = worktree_path
        .to_str()
        .ok_or_else(|| WorktreeError::ParseError("Invalid UTF-8 in worktree path".into()))?;

    let mut args = vec!["worktree", "add", "-b", branch_name, path_str];
    if let Some(base) = base_ref {
        args.push(base);
    }

    run_git(repo_root, &args)?;
    Ok(())
}

/// Lists all worktrees from repository using porcelain format.
pub fn git_worktree_list(repo_root: &Path) -> Result<Vec<Worktree>, WorktreeError> {
    let output = run_git(repo_root, &["worktree", "list", "--porcelain"])?;
    parse_worktree_list_porcelain(&output)
}

/// Removes a worktree path.
pub fn git_worktree_remove(
    repo_root: &Path,
    worktree_path: &Path,
    force: bool,
) -> Result<(), WorktreeError> {
    let path_str = worktree_path
        .to_str()
        .ok_or_else(|| WorktreeError::ParseError("Invalid UTF-8 in worktree path".into()))?;

    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path_str);

    run_git(repo_root, &args)?;
    Ok(())
}

/// Prunes stale worktree references.
pub fn git_worktree_prune(repo_root: &Path) -> Result<(), WorktreeError> {
    run_git(repo_root, &["worktree", "prune"])?;
    Ok(())
}

/// Checks dirty status inside a worktree directory using `git status --porcelain`.
pub fn git_status_porcelain(worktree_path: &Path) -> Result<DirtyState, WorktreeError> {
    let output = run_git(worktree_path, &["status", "--porcelain"])?;
    Ok(parse_status_porcelain(&output))
}

/// Deletes a git branch.
pub fn git_branch_delete(
    repo_root: &Path,
    branch_name: &str,
    force: bool,
) -> Result<(), WorktreeError> {
    let flag = if force { "-D" } else { "-d" };
    run_git(repo_root, &["branch", flag, branch_name])?;
    Ok(())
}
