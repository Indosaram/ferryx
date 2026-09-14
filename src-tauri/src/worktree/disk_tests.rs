use super::disk::*;
use super::{git, WorktreeManager};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::TempDir;

#[test]
fn disk_scan_counts_nested_and_hidden_files() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("nested/empty")).unwrap();
    fs::write(dir.path().join(".hidden"), b"abc").unwrap();
    fs::write(dir.path().join("nested/data"), b"12345").unwrap();
    let mut progress = Vec::new();
    let result = scan_path(dir.path(), &[], &AtomicBool::new(false), |p| {
        progress.push(p);
        Ok(())
    })
    .unwrap();
    assert_eq!(result.bytes, 8);
    assert_eq!(result.files, 2);
    assert_eq!(progress.last().unwrap(), &result);
}

#[test]
fn disk_scan_cancellation_stops_at_the_next_entry() {
    let dir = TempDir::new().unwrap();
    for n in 0..20 {
        fs::write(dir.path().join(n.to_string()), b"data").unwrap();
    }
    let cancelled = AtomicBool::new(false);
    let mut calls = 0;
    let error = scan_path(dir.path(), &[], &cancelled, |_| {
        calls += 1;
        cancelled.store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, crate::ipc::IpcErrorCode::ScanCancelled);
    assert_eq!(calls, 1);
    let complete = scan_path(dir.path(), &[], &AtomicBool::new(false), |_| Ok(())).unwrap();
    assert_eq!(complete.bytes, 80);
    assert_eq!(complete.files, 20);
}

#[test]
fn disk_scan_excludes_nested_worktree_roots() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("child");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("payload"), b"excluded").unwrap();
    fs::write(dir.path().join("payload"), b"own").unwrap();
    let result = scan_path(dir.path(), &[nested], &AtomicBool::new(false), |_| Ok(())).unwrap();
    assert_eq!(result.bytes, 3);
}

#[test]
fn disk_scan_missing_root_is_an_error_not_a_zero_size() {
    let dir = TempDir::new().unwrap();
    let error = scan_path(
        &dir.path().join("missing"),
        &[],
        &AtomicBool::new(false),
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, crate::ipc::IpcErrorCode::IoError);
    assert!(error.details.is_some());
}

#[cfg(unix)]
#[test]
fn disk_scan_does_not_follow_looping_or_external_symlinks() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(dir.path().join("payload"), b"abc").unwrap();
    fs::write(outside.path().join("payload"), b"excluded").unwrap();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("external")).unwrap();
    let result = scan_path(dir.path(), &[], &AtomicBool::new(false), |_| Ok(())).unwrap();
    assert_eq!(result.bytes, 3);
    assert_eq!(result.files, 1);
}

fn repository() -> (TempDir, WorktreeManager) {
    let dir = TempDir::new().unwrap();
    git::run_git(dir.path(), &["init"]).unwrap();
    git::run_git(dir.path(), &["config", "user.name", "Disk Test"]).unwrap();
    git::run_git(dir.path(), &["config", "user.email", "disk@example.test"]).unwrap();
    fs::write(dir.path().join("tracked"), b"tracked").unwrap();
    git::run_git(dir.path(), &["add", "tracked"]).unwrap();
    // Fixed commit metadata, not a timing-dependent age assertion.
    let output = crate::util::no_window_command("git")
        .current_dir(dir.path())
        .args(["commit", "-m", "fixture"])
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let manager = WorktreeManager::new(dir.path());
    (dir, manager)
}

#[test]
fn disk_scan_merges_dirty_locked_prunable_and_detached_metadata() {
    let (_dir, manager) = repository();
    let locked = manager.worktree_path_for("ws", "locked").unwrap();
    let missing = manager.worktree_path_for("ws", "missing").unwrap();
    let detached = manager.repo_root().join("detached");
    for (slug, path) in [("locked", &locked), ("missing", &missing)] {
        manager
            .create_worktree(super::CreateWorktreeOptions::new("ws", slug, path))
            .unwrap();
    }
    git::run_git(
        manager.repo_root(),
        &["worktree", "lock", locked.to_str().unwrap()],
    )
    .unwrap();
    git::run_git(
        manager.repo_root(),
        &["worktree", "add", "--detach", detached.to_str().unwrap()],
    )
    .unwrap();
    fs::write(locked.join("scratch"), b"dirty").unwrap();
    fs::remove_dir_all(&missing).unwrap();
    let rows = collect_workspace(&manager, &AtomicBool::new(false), |_| Ok(())).unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows
        .iter()
        .all(|row| row.last_commit_at == Some(1700000000)));
    let locked_row = rows.iter().find(|row| row.worktree.path == locked).unwrap();
    assert!(locked_row.worktree.locked.is_some());
    assert_eq!(locked_row.is_dirty, Some(true));
    assert!(locked_row
        .dirty_files
        .iter()
        .any(|file| file.path == "scratch"));
    assert!(locked_row.size_bytes.unwrap() >= 12);
    let missing_row = rows
        .iter()
        .find(|row| row.worktree.path == missing)
        .unwrap();
    assert!(missing_row.worktree.prunable.is_some());
    assert_eq!(missing_row.size_bytes, None);
    assert_eq!(missing_row.is_dirty, None);
    assert_eq!(
        missing_row.error.as_ref().unwrap().code,
        crate::ipc::IpcErrorCode::IoError
    );
    let detached_row = rows.iter().find(|row| row.worktree.detached).unwrap();
    assert_eq!(detached_row.is_dirty, Some(false));
}

#[test]
fn disk_scan_plain_folder_has_no_worktrees() {
    let dir = TempDir::new().unwrap();
    let manager = WorktreeManager::new(dir.path());
    assert!(
        collect_workspace(&manager, &AtomicBool::new(false), |_| Ok(()))
            .unwrap()
            .is_empty()
    );
}
