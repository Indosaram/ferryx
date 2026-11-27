//! Git-safe per-turn workspace snapshots for "Revert files too" (lane H4).
//!
//! A snapshot is a git tree object written from a **temporary index file**, so the repository's
//! real `.git/index`, `HEAD`, branches, reflog, and `git stash` list are never touched. Restore
//! lists the working tree against that tree and deletes only files the snapshot does not have,
//! then materializes the tree's blobs through another temporary index. Ignored files
//! (`node_modules`, `target`, anything in `.gitignore`) are never read into a snapshot and never
//! deleted by a restore: `git add -A` and `git ls-files -co --exclude-standard` both honour
//! `.gitignore`, and `.git` is never a path git reports.
//!
//! Every git subprocess runs inside [`crate::ipc::run_blocking`]. These functions are called from
//! the session store on Tokio worker threads; spawning a process on the reactor thread would
//! block unrelated sessions for the length of a `git add -A` over a large worktree.

use std::path::{Path, PathBuf};

use super::types::StructuredChatError;

/// One captured tree. `tree_oid` is the git tree object id of the whole non-ignored worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSnapshot {
    pub tree_oid: String,
}

/// Whether `cwd` can be snapshotted and restored: it must be inside a git worktree.
///
/// The check walks up for a `.git` directory or file and spawns no subprocess, so a history read
/// on a non-git pane costs one `stat` per ancestor. A `.git` **file** is a linked worktree or a
/// submodule, both of which git resolves normally; both are accepted. A bare repository is not a
/// worktree and is rejected, because there is no working tree to snapshot.
pub fn supports_revert(cwd: &Path) -> bool {
    if !cwd.is_dir() {
        return false;
    }
    if cwd.join("HEAD").is_file() && cwd.join("objects").is_dir() && !cwd.join(".git").exists() {
        return false;
    }
    let mut current = Some(cwd);
    while let Some(dir) = current {
        let marker = dir.join(".git");
        if marker.is_dir() {
            return true;
        }
        if marker.is_file() {
            // `gitdir: <path>` is a linked worktree or a submodule. A bare repo's `HEAD` file is
            // not a `.git` file, so this arm only ever matches a real worktree pointer.
            return std::fs::read_to_string(&marker)
                .map(|text| text.trim_start().starts_with("gitdir:"))
                .unwrap_or(false);
        }
        current = dir.parent();
    }
    false
}

/// Capture the whole non-ignored working tree as one tree object, or `None` where there is
/// nothing to capture (not a worktree, no git, or an empty repository with no HEAD and no files).
pub async fn capture_snapshot(cwd: &Path) -> Result<Option<TurnSnapshot>, StructuredChatError> {
    if !supports_revert(cwd) {
        return Ok(None);
    }
    let cwd = cwd.to_path_buf();
    let captured = crate::ipc::run_blocking(move || {
        let index = TempIndex::new()?;
        let root = worktree_root(&index, &cwd)?;
        let _ = index.run(&root, &["read-tree", "HEAD"]);
        index.run(&root, &["add", "-A", "--", "."])?;
        let tree = index.run(&root, &["write-tree"])?;
        Ok(Some(tree))
    })
    .await
    .map_err(|error| StructuredChatError::Provider(format!("snapshot failed: {error}")))?;

    let tree_oid = match captured {
        Some(tree) => tree.trim().to_string(),
        None => return Ok(None),
    };
    if !is_tree_oid(&tree_oid) {
        return Err(StructuredChatError::Provider(
            "snapshot failed: git write-tree did not return a tree id".into(),
        ));
    }
    Ok(Some(TurnSnapshot { tree_oid }))
}

/// Put the working tree back to `snapshot`, deleting the files that were created after it.
///
/// Files the snapshot has are restored byte-for-byte from the tree's blobs. Files the working
/// tree has that the snapshot does not are deleted - but only non-ignored, non-`.git` paths, and
/// only empty directories the delete leaves behind inside `cwd` are pruned. Ignored files and the
/// repository's own metadata are left exactly as they were.
pub async fn restore_snapshot(
    cwd: &Path,
    snapshot: &TurnSnapshot,
) -> Result<(), StructuredChatError> {
    let tree_oid = snapshot.tree_oid.trim().to_string();
    if !is_tree_oid(&tree_oid) {
        return Err(StructuredChatError::InvalidRequest(
            "REVERT_FILES_UNSUPPORTED: the recorded snapshot is not a git tree id".into(),
        ));
    }
    if !supports_revert(cwd) {
        return Err(StructuredChatError::Unsupported(
            "REVERT_FILES_UNSUPPORTED: the workspace is not a git worktree".into(),
        ));
    }
    let cwd = cwd.to_path_buf();
    crate::ipc::run_blocking(move || {
        let index = TempIndex::new()?;
        let root = worktree_root(&index, &cwd)?;
        index.run(&root, &["read-tree", &tree_oid])?;
        let in_tree = split_z(&index.run(&root, &["ls-tree", "-r", "--name-only", "-z", &tree_oid])?);
        let present = split_z(&index.run(
            &root,
            &["ls-files", "-co", "--exclude-standard", "-z"],
        )?);

        let mut remove_errors = Vec::new();
        for relative in &present {
            if in_tree.iter().any(|known| known == relative) {
                continue;
            }
            let Some(path) = safe_join(&root, relative) else {
                continue;
            };
            if let Err(err) = std::fs::remove_file(&path) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    remove_errors.push(format!("{}: {err}", path.display()));
                }
                continue;
            }
            prune_empty_parents(&root, path.parent());
        }
        if !remove_errors.is_empty() {
            return Err(crate::ipc::IpcError::internal(format!(
                "revert incomplete: could not remove {}",
                remove_errors.join("; ")
            )));
        }

        index.run(&root, &["checkout-index", "-a", "-f"])?;
        Ok(())
    })
    .await
    .map_err(|error| StructuredChatError::Provider(format!("restore failed: {error}")))
}

/// A git object id: 40 (SHA-1) or 64 (SHA-256) lowercase hex characters. Anything else is
/// refused before it can reach a `git` argv.
fn is_tree_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn split_z(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

fn worktree_root(index: &TempIndex, cwd: &Path) -> Result<PathBuf, crate::ipc::IpcError> {
    let top = index.run(cwd, &["rev-parse", "--show-toplevel"])?;
    let trimmed = top.trim();
    if trimmed.is_empty() {
        return Err(crate::ipc::IpcError::internal(
            "git rev-parse --show-toplevel returned empty path",
        ));
    }
    Ok(PathBuf::from(trimmed))
}

fn safe_join(cwd: &Path, relative: &str) -> Option<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return None;
    }
    if candidate.components().any(|component| {
        matches!(component, std::path::Component::ParentDir) || component.as_os_str() == ".git"
    }) {
        return None;
    }
    candidate.components().next()?;
    Some(cwd.join(candidate))
}

/// Remove directories left empty by a delete, stopping at `cwd` and never removing it.
fn prune_empty_parents(cwd: &Path, mut dir: Option<&Path>) {
    while let Some(current) = dir {
        if current == cwd || !current.starts_with(cwd) {
            break;
        }
        if std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// A temporary `GIT_INDEX_FILE` that is deleted when it goes out of scope.
///
/// The path is unique per snapshot and lives in the system temp dir, never in the repository, so
/// a crash cannot leave a stray index inside a user's worktree.
struct TempIndex {
    path: PathBuf,
}

impl TempIndex {
    fn new() -> Result<Self, crate::ipc::IpcError> {
        let unique = format!(
            "ferryx-revert-{}-{}.index",
            std::process::id(),
            next_sequence()
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_file(&path);
        Ok(Self { path })
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Result<String, crate::ipc::IpcError> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            // Snapshots must round-trip worktree bytes verbatim on every OS; Windows defaults
            // `core.autocrlf=true`, which would otherwise rewrite LF files to CRLF on restore.
            .arg("-c")
            .arg("core.autocrlf=false")
            .args(args)
            .env("GIT_INDEX_FILE", &self.path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|error| {
                crate::ipc::IpcError::internal(format!(
                    "git {} could not start: {error}",
                    args.join(" ")
                ))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::ipc::IpcError::internal(format!(
                "git {} exited {}: {}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn next_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway git repository with one committed file and one ignored path.
    struct Repo {
        dir: PathBuf,
    }

    impl Repo {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ferryx-revert-{}-{}",
                std::process::id(),
                next_sequence()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let repo = Self { dir };
            repo.git(&["init", "--quiet"]);
            repo.git(&["config", "user.name", "Ferryx Test"]);
            repo.git(&["config", "user.email", "test@ferryx.invalid"]);
            repo.write(".gitignore", "ignored.txt\nbuild/\n");
            repo.write("tracked.txt", "before\n");
            repo.write("doomed.txt", "still here\n");
            repo.git(&["add", "-A"]);
            repo.git(&["commit", "--quiet", "-m", "base"]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.dir)
                .args(args)
                .output()
                .expect("git starts");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.dir.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parent dir");
            }
            std::fs::write(path, contents).expect("write");
        }

        fn read(&self, relative: &str) -> String {
            std::fs::read_to_string(self.dir.join(relative)).expect("read")
        }

        fn exists(&self, relative: &str) -> bool {
            self.dir.join(relative).exists()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn supports_revert_is_false_outside_a_worktree_and_true_inside_one() {
        let plain = std::env::temp_dir().join(format!(
            "ferryx-revert-plain-{}-{}",
            std::process::id(),
            next_sequence()
        ));
        std::fs::create_dir_all(&plain).expect("plain dir");
        assert!(!supports_revert(&plain), "a plain temp dir is not a worktree");
        let _ = std::fs::remove_dir_all(&plain);

        let repo = Repo::new();
        assert!(supports_revert(&repo.dir));
        let nested = repo.dir.join("a/b/c");
        std::fs::create_dir_all(&nested).expect("nested");
        assert!(supports_revert(&nested), "a subdirectory resolves to its worktree");

        let missing = repo.dir.join("does/not/exist");
        assert!(!supports_revert(&missing), "a path that is not a dir is refused");
    }

    #[test]
    fn a_linked_worktree_git_file_counts_and_a_bare_repository_does_not() {
        let repo = Repo::new();
        let linked = repo.dir.join("linked");
        repo.git(&[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().expect("utf8 path"),
        ]);
        assert!(
            repo.dir.join("linked/.git").is_file(),
            "a linked worktree has a .git file"
        );
        assert!(supports_revert(&linked), "a linked worktree is snapshottable");

        let bare = std::env::temp_dir().join(format!(
            "ferryx-revert-bare-{}-{}",
            std::process::id(),
            next_sequence()
        ));
        repo.git(&[
            "init",
            "--quiet",
            "--bare",
            bare.to_str().expect("utf8 path"),
        ]);
        assert!(
            !supports_revert(&bare),
            "a bare repository has no working tree to snapshot"
        );
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[tokio::test]
    async fn a_snapshot_is_none_outside_a_worktree() {
        let plain = std::env::temp_dir().join(format!(
            "ferryx-revert-none-{}-{}",
            std::process::id(),
            next_sequence()
        ));
        std::fs::create_dir_all(&plain).expect("plain dir");
        assert!(capture_snapshot(&plain).await.expect("no error").is_none());
        let _ = std::fs::remove_dir_all(&plain);
    }

    #[tokio::test]
    async fn capture_then_restore_puts_every_file_back_and_keeps_ignored_ones() {
        let repo = Repo::new();
        let snapshot = capture_snapshot(&repo.dir)
            .await
            .expect("capture")
            .expect("a worktree snapshots");
        assert!(
            is_tree_oid(&snapshot.tree_oid),
            "capture returns a real tree id: {}",
            snapshot.tree_oid
        );

        // The turn: edit a tracked file, delete another, add a new one, add an ignored one.
        repo.write("tracked.txt", "after the turn\n");
        std::fs::remove_file(repo.dir.join("doomed.txt")).expect("delete tracked");
        repo.write("created/nested/new.txt", "made by the turn\n");
        repo.write("ignored.txt", "keep me\n");
        repo.write("build/artifact.bin", "keep me too\n");

        let head_before = repo.git(&["rev-parse", "HEAD"]);
        let index_before = repo.git(&["ls-files", "-s"]);
        let stash_before = repo.git(&["stash", "list"]);

        restore_snapshot(&repo.dir, &snapshot).await.expect("restore");

        assert_eq!(repo.read("tracked.txt"), "before\n", "a modified file is restored");
        assert_eq!(repo.read("doomed.txt"), "still here\n", "a deleted file is restored");
        assert!(
            !repo.exists("created/nested/new.txt"),
            "a file created after the snapshot is removed"
        );
        assert!(
            !repo.exists("created"),
            "the empty directory the delete leaves is pruned"
        );
        assert_eq!(repo.read("ignored.txt"), "keep me\n", "an ignored file survives");
        assert_eq!(repo.read("build/artifact.bin"), "keep me too\n", "an ignored tree survives");
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head_before, "HEAD is untouched");
        assert_eq!(repo.git(&["ls-files", "-s"]), index_before, "the real index is untouched");
        assert_eq!(repo.git(&["stash", "list"]), stash_before, "the stash list is untouched");
    }

    #[tokio::test]
    async fn restore_leaves_a_gitignored_file_created_after_the_snapshot_alone() {
        let repo = Repo::new();
        let snapshot = capture_snapshot(&repo.dir).await.expect("capture").expect("tree");
        repo.write("ignored.txt", "written during the turn\n");
        restore_snapshot(&repo.dir, &snapshot).await.expect("restore");
        assert_eq!(
            repo.read("ignored.txt"),
            "written during the turn\n",
            "restore never deletes a path git ignores"
        );
    }

    #[tokio::test]
    async fn a_snapshot_of_a_dirty_worktree_restores_the_dirty_state() {
        let repo = Repo::new();
        repo.write("tracked.txt", "uncommitted edit\n");
        let snapshot = capture_snapshot(&repo.dir).await.expect("capture").expect("tree");
        repo.write("tracked.txt", "a later edit\n");
        restore_snapshot(&repo.dir, &snapshot).await.expect("restore");
        assert_eq!(
            repo.read("tracked.txt"),
            "uncommitted edit\n",
            "a snapshot is the worktree, not HEAD"
        );
    }

    #[tokio::test]
    async fn restore_preserves_lf_and_crlf_bytes_under_autocrlf_true() {
        let repo = Repo::new();
        repo.git(&["config", "core.autocrlf", "true"]);
        repo.write("lf.txt", "line one\nline two\n");
        repo.write("crlf.txt", "line one\r\nline two\r\n");
        let snapshot = capture_snapshot(&repo.dir).await.expect("capture").expect("tree");
        repo.write("lf.txt", "changed\n");
        repo.write("crlf.txt", "changed\r\n");
        restore_snapshot(&repo.dir, &snapshot).await.expect("restore");
        assert_eq!(repo.read("lf.txt"), "line one\nline two\n");
        assert_eq!(repo.read("crlf.txt"), "line one\r\nline two\r\n");
    }

    #[tokio::test]
    async fn an_invalid_tree_oid_is_rejected_before_any_git_runs() {
        let repo = Repo::new();
        for bad in [
            "",
            "HEAD",
            "HEAD~1",
            "not-a-tree",
            "0123456789012345678901234567890123456789; rm -rf /",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            let snapshot = TurnSnapshot { tree_oid: bad.to_string() };
            let error = restore_snapshot(&repo.dir, &snapshot)
                .await
                .expect_err("an invalid tree id is refused");
            assert_eq!(error.code(), "INVALID_REQUEST", "{bad:?} must be refused");
            assert!(repo.exists("tracked.txt"), "nothing was deleted for {bad:?}");
        }
    }

    #[tokio::test]
    async fn a_missing_tree_oid_is_a_provider_error_not_a_silent_success() {
        let repo = Repo::new();
        let snapshot = TurnSnapshot {
            tree_oid: "0".repeat(40),
        };
        let error = restore_snapshot(&repo.dir, &snapshot)
            .await
            .expect_err("a tree that is not in the object database cannot be restored");
        assert_eq!(error.code(), "PROVIDER_FAILED");
        assert!(repo.exists("tracked.txt"));
    }

    #[tokio::test]
    async fn capture_does_not_touch_the_real_index_or_create_a_stash() {
        let repo = Repo::new();
        repo.write("tracked.txt", "dirty\n");
        let index_before = std::fs::read(repo.dir.join(".git/index")).expect("real index");
        let _ = capture_snapshot(&repo.dir).await.expect("capture");
        let index_after = std::fs::read(repo.dir.join(".git/index")).expect("real index");
        assert_eq!(index_before, index_after, "the real .git/index is byte-identical");
        assert_eq!(repo.git(&["stash", "list"]), "", "no stash was created");
    }

    #[tokio::test]
    async fn a_temporary_index_is_removed_after_the_operation() {
        let index = TempIndex::new().expect("temp index");
        let path = index.path.clone();
        std::fs::write(&path, b"index-bytes").expect("write temp index");
        assert!(path.exists());
        drop(index);
        assert!(
            !path.exists(),
            "temporary index file must be removed on Drop"
        );
    }

    #[tokio::test]
    async fn capture_and_restore_from_subdirectory_is_symmetric() {
        let repo = Repo::new();
        let sub = repo.dir.join("packages").join("app");
        std::fs::create_dir_all(&sub).expect("subdir");
        repo.write("root.txt", "before-root");
        repo.write("packages/app/sub.txt", "before-sub");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", "sub"]);

        let snap = capture_snapshot(&sub).await.expect("capture").expect("tree");
        repo.write("root.txt", "after-root");
        repo.write("packages/app/sub.txt", "after-sub");
        repo.write("packages/app/extra.txt", "extra");

        restore_snapshot(&sub, &snap).await.expect("restore");
        assert_eq!(std::fs::read_to_string(repo.dir.join("root.txt")).unwrap(), "before-root");
        assert_eq!(std::fs::read_to_string(sub.join("sub.txt")).unwrap(), "before-sub");
        assert!(!sub.join("extra.txt").exists());
        assert!(safe_join(&repo.dir, "sub/.git/config").is_none());
    }

    #[test]
    fn safe_join_refuses_escapes_and_the_git_directory() {
        let root = Path::new("/work");
        assert_eq!(safe_join(root, "a/b.txt"), Some(PathBuf::from("/work/a/b.txt")));
        assert_eq!(safe_join(root, "../etc/passwd"), None);
        assert_eq!(safe_join(root, "/etc/passwd"), None);
        assert_eq!(safe_join(root, ".git/config"), None);
    }
}
