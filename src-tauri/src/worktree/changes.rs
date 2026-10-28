//! Per-worktree change operations: diff, stage, unstage, commit and push.
//!
//! Every path a caller passes is relative to the worktree root and is jailed to it; Git sees it
//! as a literal pathspec after `--`. Pushes never force.

use crate::worktree::git::run_git;
use crate::worktree::model::WorktreeError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// What a diff compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum GitDiffScope {
    /// Unstaged changes: working tree against the index.
    WorkingTree,
    /// Staged changes: index against HEAD.
    Staged,
    /// The changes a single commit introduced.
    Commit { sha: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitPushResult {
    pub remote: String,
    pub branch: String,
    /// The push created the upstream (`push -u`) because the branch had none.
    pub set_upstream: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GitChangeError {
    #[error("Nothing is staged to commit")]
    NothingToCommit,
    #[error("Commit message cannot be empty")]
    EmptyMessage,
    #[error("The remote rejected the push: {stderr}")]
    PushRejected { stderr: String },
    #[error("The branch has no upstream and the repository has no 'origin' remote")]
    NoRemote,
    #[error("HEAD is detached; check out a branch before pushing")]
    DetachedHead,
    #[error("Invalid commit '{sha}'")]
    InvalidCommit { sha: String },
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
}

fn invalid_path(path: &str, reason: &str) -> GitChangeError {
    WorktreeError::InvalidPath {
        path: PathBuf::from(path),
        reason: reason.into(),
    }
    .into()
}

/// Validates a caller-supplied path and returns it as a worktree-relative Git path.
fn jailed_path(root: &Path, raw: &str) -> Result<String, GitChangeError> {
    if raw.is_empty() {
        return Err(invalid_path(raw, "path cannot be empty"));
    }
    if raw.chars().any(char::is_control) {
        return Err(invalid_path(raw, "path cannot contain control characters"));
    }
    let candidate = Path::new(raw);
    // `/x` is not absolute on Windows and `C:x` is not absolute anywhere, so reject roots and
    // prefixes by component as well as by `is_absolute`.
    if candidate.is_absolute()
        || raw.starts_with(['/', '\\'])
        || candidate
            .components()
            .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        return Err(invalid_path(raw, "path must be relative to the worktree"));
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
        || raw.split(['/', '\\']).any(|part| part == "..")
    {
        return Err(invalid_path(raw, "parent traversal ('..') is forbidden"));
    }
    let relative: Vec<&str> = candidate
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    if relative.is_empty() {
        return Err(invalid_path(raw, "path must name a file or directory"));
    }
    // A symlinked parent may still point outside the worktree; resolve what exists.
    let joined = root.join(relative.join("/"));
    let mut existing = joined.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    if let (Ok(resolved), Ok(canonical_root)) = (
        std::fs::canonicalize(existing),
        std::fs::canonicalize(root),
    ) {
        if !resolved.starts_with(&canonical_root) {
            return Err(WorktreeError::PathOutsideWorkspace {
                path: PathBuf::from(raw),
                root: canonical_root,
            }
            .into());
        }
    }
    Ok(relative.join("/"))
}

fn jailed_paths(root: &Path, paths: &[String]) -> Result<Vec<String>, GitChangeError> {
    if paths.is_empty() {
        return Err(invalid_path("", "at least one path is required"));
    }
    paths.iter().map(|p| jailed_path(root, p)).collect()
}

fn validate_sha(sha: &str) -> Result<(), GitChangeError> {
    if (4..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitChangeError::InvalidCommit { sha: sha.into() })
    }
}

/// Returns the unified patch for `scope`, optionally limited to one file.
pub fn git_diff(
    worktree: &Path,
    scope: &GitDiffScope,
    file: Option<&str>,
) -> Result<String, GitChangeError> {
    let file = file.map(|f| jailed_path(worktree, f)).transpose()?;
    let mut args: Vec<String> = vec!["--literal-pathspecs".into()];
    match scope {
        GitDiffScope::WorkingTree => args.extend(["diff".into()]),
        GitDiffScope::Staged => args.extend(["diff".into(), "--cached".into()]),
        GitDiffScope::Commit { sha } => {
            validate_sha(sha)?;
            let commit = format!("{sha}^{{commit}}");
            run_git(worktree, &["rev-parse", "--verify", "--quiet", commit.as_str()])
                .map_err(|_| GitChangeError::InvalidCommit { sha: sha.clone() })?;
            args.extend(["show".into(), "--format=".into()]);
        }
    }
    args.extend(["--no-color".into(), "--no-ext-diff".into(), "--no-textconv".into()]);
    if let GitDiffScope::Commit { sha } = scope {
        args.push(sha.clone());
    }
    args.push("--".into());
    args.extend(file);
    Ok(run_git(worktree, &args)?)
}

pub fn git_stage(worktree: &Path, paths: &[String]) -> Result<(), GitChangeError> {
    let paths = jailed_paths(worktree, paths)?;
    let mut args = vec!["--literal-pathspecs", "add", "--all", "--"];
    args.extend(paths.iter().map(String::as_str));
    run_git(worktree, &args)?;
    Ok(())
}

pub fn git_unstage(worktree: &Path, paths: &[String]) -> Result<(), GitChangeError> {
    let paths = jailed_paths(worktree, paths)?;
    // `reset -- <paths>` also works on an unborn branch, unlike `restore --staged`.
    let mut args = vec!["--literal-pathspecs", "reset", "-q", "--"];
    args.extend(paths.iter().map(String::as_str));
    run_git(worktree, &args)?;
    Ok(())
}

/// Commits the index and returns the new commit sha.
pub fn git_commit(worktree: &Path, message: &str) -> Result<String, GitChangeError> {
    if message.trim().is_empty() {
        return Err(GitChangeError::EmptyMessage);
    }
    match run_git(worktree, &["diff", "--cached", "--quiet"]) {
        Ok(_) => return Err(GitChangeError::NothingToCommit),
        Err(WorktreeError::GitError { code: Some(1), .. }) => {}
        Err(error) => return Err(error.into()),
    }
    run_git(worktree, &["commit", "-q", "-m", message])?;
    Ok(run_git(worktree, &["rev-parse", "HEAD"])?.trim().to_string())
}

fn config_value(worktree: &Path, key: &str) -> Result<Option<String>, GitChangeError> {
    match run_git(worktree, &["config", "--get", key]) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|v| !v.is_empty())),
        Err(WorktreeError::GitError { code: Some(1), .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn is_rejection(stderr: &str) -> bool {
    ["[rejected]", "[remote rejected]", "non-fast-forward", "fetch first"]
        .iter()
        .any(|marker| stderr.contains(marker))
}

/// Pushes the current branch to its upstream, or to `origin` with `-u` when it has none.
/// Never forces.
pub fn git_push(worktree: &Path) -> Result<GitPushResult, GitChangeError> {
    let branch = match run_git(worktree, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(branch) if !branch.trim().is_empty() => branch.trim().to_string(),
        Ok(_) | Err(WorktreeError::GitError { code: Some(1), .. }) => {
            return Err(GitChangeError::DetachedHead)
        }
        Err(error) => return Err(error.into()),
    };
    let upstream_remote = config_value(worktree, &format!("branch.{branch}.remote"))?;
    let upstream_merge = config_value(worktree, &format!("branch.{branch}.merge"))?;
    let (remote, refspec, set_upstream) = match (upstream_remote, upstream_merge) {
        (Some(remote), Some(merge)) if remote != "." => {
            (remote, format!("refs/heads/{branch}:{merge}"), false)
        }
        _ => {
            let remotes = run_git(worktree, &["remote"])?;
            if !remotes.lines().any(|name| name.trim() == "origin") {
                return Err(GitChangeError::NoRemote);
            }
            (
                "origin".to_string(),
                format!("refs/heads/{branch}:refs/heads/{branch}"),
                true,
            )
        }
    };
    if remote.starts_with('-') {
        return Err(GitChangeError::NoRemote);
    }
    // A GUI-launched Git has no terminal to prompt on; fail instead of waiting for credentials.
    let mut args = vec!["-c", "credential.interactive=never", "push", "--porcelain"];
    if set_upstream {
        args.push("-u");
    }
    args.extend([remote.as_str(), refspec.as_str()]);
    match run_git(worktree, &args) {
        Ok(_) => Ok(GitPushResult {
            remote,
            branch,
            set_upstream,
        }),
        Err(WorktreeError::GitError { stderr, stdout, .. })
            if is_rejection(&stderr) || is_rejection(&stdout) =>
        {
            Err(GitChangeError::PushRejected {
                stderr: if stderr.is_empty() { stdout } else { stderr },
            })
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_repo(path: &Path) {
        run_git(path, &["init", "-q", "-b", "main"]).unwrap();
        configure_identity(path);
    }

    fn configure_identity(path: &Path) {
        run_git(path, &["config", "user.name", "Changes Test"]).unwrap();
        run_git(path, &["config", "user.email", "changes@ferryx.test"]).unwrap();
        run_git(path, &["config", "commit.gpgsign", "false"]).unwrap();
    }

    /// A repository with one commit and a bare `origin` remote that has not seen it yet.
    fn repo_with_bare_remote() -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("repo");
        let bare = dir.path().join("remote.git");
        fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        fs::write(repo.join("file.txt"), "first\n").unwrap();
        run_git(&repo, &["add", "file.txt"]).unwrap();
        run_git(&repo, &["commit", "-q", "-m", "base"]).unwrap();
        run_git(dir.path(), &["init", "-q", "--bare", "-b", "main", "remote.git"]).unwrap();
        run_git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
        (dir, repo, bare)
    }

    #[test]
    fn diff_stage_commit_and_push_round_trip() {
        let (_dir, repo, bare) = repo_with_bare_remote();
        fs::write(repo.join("file.txt"), "first\nline\n").unwrap();

        let working = git_diff(&repo, &GitDiffScope::WorkingTree, None).unwrap();
        assert!(working.contains("+line"), "{working}");
        assert!(git_diff(&repo, &GitDiffScope::Staged, None).unwrap().is_empty());

        git_stage(&repo, &["file.txt".into()]).unwrap();
        let staged = git_diff(&repo, &GitDiffScope::Staged, Some("file.txt")).unwrap();
        assert!(staged.contains("+line"), "{staged}");
        assert!(git_diff(&repo, &GitDiffScope::WorkingTree, None).unwrap().is_empty());

        git_unstage(&repo, &["file.txt".into()]).unwrap();
        assert!(git_diff(&repo, &GitDiffScope::Staged, None).unwrap().is_empty());
        git_stage(&repo, &["file.txt".into()]).unwrap();

        let sha = git_commit(&repo, "msg").unwrap();
        assert_eq!(sha.len(), 40);
        assert_eq!(run_git(&repo, &["status", "--porcelain"]).unwrap(), "");
        let shown = git_diff(&repo, &GitDiffScope::Commit { sha: sha.clone() }, None).unwrap();
        assert!(shown.contains("+line"), "{shown}");

        // No upstream yet: pushes to origin and records the upstream.
        let first = git_push(&repo).unwrap();
        assert_eq!(first.remote, "origin");
        assert_eq!(first.branch, "main");
        assert!(first.set_upstream);
        let log = run_git(&bare, &["log", "--format=%H", "main"]).unwrap();
        assert!(log.contains(&sha), "{log}");

        // With an upstream, the next push goes to it.
        fs::write(repo.join("second.txt"), "two\n").unwrap();
        git_stage(&repo, &["second.txt".into()]).unwrap();
        let second_sha = git_commit(&repo, "second").unwrap();
        assert!(!git_push(&repo).unwrap().set_upstream);
        let log = run_git(&bare, &["log", "--format=%H", "main"]).unwrap();
        assert!(log.contains(&second_sha), "{log}");
    }

    #[test]
    fn commit_refuses_an_empty_index_or_message() {
        let (_dir, repo, _bare) = repo_with_bare_remote();
        assert!(matches!(git_commit(&repo, "msg"), Err(GitChangeError::NothingToCommit)));
        fs::write(repo.join("file.txt"), "changed\n").unwrap();
        // Unstaged edits still leave nothing to commit.
        assert!(matches!(git_commit(&repo, "msg"), Err(GitChangeError::NothingToCommit)));
        git_stage(&repo, &["file.txt".into()]).unwrap();
        assert!(matches!(git_commit(&repo, "  \n"), Err(GitChangeError::EmptyMessage)));
    }

    #[test]
    fn paths_outside_the_worktree_are_rejected() {
        let (dir, repo, _bare) = repo_with_bare_remote();
        fs::write(dir.path().join("x"), "outside\n").unwrap();
        let outside = dir.path().join("x").to_str().unwrap().to_string();
        for bad in ["../x", "sub/../../x", "..", "", "./", outside.as_str()] {
            let result = git_stage(&repo, &[bad.to_string()]);
            assert!(
                matches!(result, Err(GitChangeError::Worktree(WorktreeError::InvalidPath { .. }))),
                "{bad}: {result:?}"
            );
        }
        assert!(git_diff(&repo, &GitDiffScope::WorkingTree, Some("../x")).is_err());
        assert!(git_unstage(&repo, &["../x".into()]).is_err());
        assert!(matches!(
            git_diff(&repo, &GitDiffScope::Commit { sha: "--output=/tmp/x".into() }, None),
            Err(GitChangeError::InvalidCommit { .. })
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), repo.join("escape")).unwrap();
            assert!(matches!(
                git_stage(&repo, &["escape/x".into()]),
                Err(GitChangeError::Worktree(WorktreeError::PathOutsideWorkspace { .. }))
            ));
        }
        // Pathspec magic is taken literally, so it cannot widen the selection.
        fs::write(repo.join("file.txt"), "changed\n").unwrap();
        assert!(git_stage(&repo, &[":(top)file.txt".into()]).is_err());
        assert!(git_diff(&repo, &GitDiffScope::Staged, None).unwrap().is_empty());
    }

    #[test]
    fn diverged_push_is_rejected_without_force() {
        let (dir, repo, bare) = repo_with_bare_remote();
        git_push(&repo).unwrap();
        let other = dir.path().join("other");
        run_git(dir.path(), &["clone", "-q", bare.to_str().unwrap(), "other"]).unwrap();
        configure_identity(&other);
        fs::write(other.join("theirs.txt"), "theirs\n").unwrap();
        run_git(&other, &["add", "theirs.txt"]).unwrap();
        run_git(&other, &["commit", "-q", "-m", "theirs"]).unwrap();
        run_git(&other, &["push", "-q", "origin", "main"]).unwrap();
        let remote_head = run_git(&bare, &["rev-parse", "main"]).unwrap();

        fs::write(repo.join("ours.txt"), "ours\n").unwrap();
        git_stage(&repo, &["ours.txt".into()]).unwrap();
        git_commit(&repo, "ours").unwrap();
        let result = git_push(&repo);
        assert!(matches!(result, Err(GitChangeError::PushRejected { .. })), "{result:?}");
        assert_eq!(run_git(&bare, &["rev-parse", "main"]).unwrap(), remote_head);
    }

    #[test]
    fn push_without_upstream_or_origin_reports_no_remote() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        git_stage(dir.path(), &["a.txt".into()]).unwrap();
        git_commit(dir.path(), "a").unwrap();
        assert!(matches!(git_push(dir.path()), Err(GitChangeError::NoRemote)));
    }
}
