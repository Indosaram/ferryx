# Audit: worktree
Repo: /Users/indo/code/project/orca-lite
Scanned:
- src-tauri/src/worktree/git.rs
- src-tauri/src/worktree/manager.rs
- src-tauri/src/worktree/registry.rs
- src-tauri/src/worktree/mod.rs
- src-tauri/src/worktree/model.rs
- src-tauri/src/ipc/worktree.rs
- src-tauri/tests/worktree_safety.rs
- src-tauri/src/ipc/blocking_contract_tests.rs
- src-tauri/Cargo.toml
Date: 2026-08-22

## Findings

### F-worktree-01
- Severity: High
- File: src-tauri/src/worktree/registry.rs:68
- Mechanism: `WorkspaceRegistry::resolve_worktree` calls `manager.find_worktree_by_slug(&identity.ws_id, &identity.slug)`. In `WorktreeManager::find_worktree_by_slug`, the method calls `self.list_worktrees()?`, which executes an external `git worktree list --porcelain` subprocess and performs `fs::canonicalize` on every worktree in the repository. On common high-frequency operations such as worktree dirty status queries (`cmd_worktree_status`), deletion previews (`cmd_worktree_delete_preview`), delete requests (`cmd_worktree_delete`), and terminal target resolution (`resolve_terminal_target`), locating a single target worktree forces an O(N) Git process execution and N filesystem canonicalization syscalls even though the canonical worktree directory path is deterministically computed via `worktree_path_for(ws_id, slug)`.
- Hot path: yes
- Suggested fix: Query or validate the targeted worktree directly using `manager.worktree_path_for(&identity.ws_id, &identity.slug)` and check the single path's status rather than enumerating and canonicalizing every worktree across the repository via `git worktree list`.
- Write scope: src-tauri/src/worktree/registry.rs, src-tauri/src/worktree/manager.rs
- RED proof:
```rust
// src-tauri/src/worktree/registry.rs
pub fn resolve_worktree(
    &self,
    workspace_id: &str,
    identity: &WorktreeIdentity,
) -> Result<(WorktreeManager, Worktree), WorktreeError> {
    let manager = self.manager(workspace_id)?;
    let worktree = manager
        .find_worktree_by_slug(&identity.ws_id, &identity.slug)?
        .ok_or_else(|| WorktreeError::WorktreeIdentityNotFound {
            workspace_id: workspace_id.to_string(),
            ws_id: identity.ws_id.clone(),
            slug: identity.slug.clone(),
        })?;
    manager.canonical_allowed_path(&worktree.path)?;
    Ok((manager, worktree))
}

// src-tauri/src/worktree/manager.rs
pub fn find_worktree_by_slug(
    &self,
    ws_id: &str,
    slug: &str,
) -> Result<Option<Worktree>, WorktreeError> {
    let target_branch = Self::format_branch_name(ws_id, slug)?;
    for wt in self.list_worktrees()? {
        if wt.branch_short_name() == Some(target_branch.as_str()) {
            return Ok(Some(wt));
        }
    }
    Ok(None)
}
```
Why it is slow: Every worktree status or resolution request spawns `git worktree list --porcelain` and traverses/canonicalizes all worktrees linearly instead of querying the specific worktree path directly.

---

### F-worktree-02
- Severity: Medium
- File: src-tauri/src/worktree/manager.rs:226
- Mechanism: After creating a new worktree with `git_worktree_add`, `WorktreeManager::create_worktree` invokes `self.list_worktrees()?` to find the newly created worktree and return its `Worktree` record. `list_worktrees()` already runs `git worktree list --porcelain` and canonicalizes each item's path. Then the subsequent loop in `create_worktree` executes `self.canonical_allowed_path(&wt.path)` a second time on every single worktree in the repository.
- Hot path: yes
- Suggested fix: Return the constructed `Worktree` struct directly from known inputs and the verified canonical path, or query only the single created worktree path rather than executing an extra `git worktree list` subprocess and quadratic canonicalization passes.
- Write scope: src-tauri/src/worktree/manager.rs
- RED proof:
```rust
// src-tauri/src/worktree/manager.rs
let target_canonical = self.canonical_allowed_path(&options.path)?;
for wt in self.list_worktrees()? {
    if let Ok(wt_canonical) = self.canonical_allowed_path(&wt.path) {
        if wt_canonical == target_canonical {
            return Ok(wt);
        }
    }
}
```
Why it is slow: Worktree creation issues an unnecessary `git worktree list` process spawn and performs double-canonicalization loops (`2 * N` `fs::canonicalize` syscalls) across all existing worktrees.

---

### F-worktree-03
- Severity: Medium
- File: src-tauri/src/worktree/manager.rs:309
- Mechanism: `WorktreeManager::branch_is_merged` checks branch merge status by executing `git branch --merged HEAD --format=%(refname:short)`. This enumerates and formats every single branch merged into `HEAD` across the entire repository, returning an unbounded multi-line string that is parsed and searched linearly in Rust. In large repositories with hundreds or thousands of branches, this incurs substantial memory allocation, string formatting, and process stdout overhead.
- Hot path: yes
- Suggested fix: Replace the unbounded branch dump with `git merge-base --is-ancestor <branch> HEAD` (or against the target base branch), which runs an efficient commit graph reachability check and returns exit code 0 or 1 without generating branch lists or string allocations.
- Write scope: src-tauri/src/worktree/manager.rs, src-tauri/src/worktree/git.rs
- RED proof:
```rust
// src-tauri/src/worktree/manager.rs
fn branch_is_merged(&self, branch: &str) -> Result<bool, WorktreeError> {
    let merged = run_git(
        &self.repo_root,
        &["branch", "--merged", "HEAD", "--format=%(refname:short)"],
    )?;
    Ok(merged.lines().any(|line| line.trim() == branch))
}
```
Why it is slow: Spawns a full-repo branch list query and linear string comparison instead of a constant-output Git ancestor check (`git merge-base --is-ancestor`).

---

### F-worktree-04
- Severity: Medium
- File: src-tauri/src/worktree/manager.rs:360
- Mechanism: `WorktreeManager::delete_worktree_and_branch_inner` acquires `self.delete_lock: Arc<Mutex<()>>` and holds it continuously across a synchronous sequence of 6 to 8 external `git` subprocess executions: `find_worktree` (`git worktree list`), `run_git` (`rev-parse`), `branch_is_merged` (`git branch --merged`), `rev-parse @{upstream}`, `rev-list --left-right`, `check_dirty` (`git status --porcelain`), `git worktree remove`, `git worktree prune`, and `git branch -d`. Because `acquire_writer` and `acquire_writer_lease` also synchronize on `self.delete_lock`, writer lease operations and deletions on entirely independent worktrees in the same repository are forced to block waiting on child process I/O.
- Hot path: yes
- Suggested fix: Narrow the lock scope or use path-keyed lease/deletion synchronization so that worktree deletions and lease acquisitions for distinct paths execute concurrently without global manager-level lock contention.
- Write scope: src-tauri/src/worktree/manager.rs
- RED proof:
```rust
// src-tauri/src/worktree/manager.rs
fn delete_worktree_and_branch_inner(
    &self,
    worktree_path: &Path,
    delete_branch: bool,
    destructive: bool,
) -> Result<bool, WorktreeError> {
    let _delete_guard = self.delete_lock.lock();
    self.ensure_no_writer(worktree_path)?;

    let branch = if delete_branch {
        let preview = self.branch_deletion_preview(worktree_path)?;
        if !destructive && !preview.merged {
            return Err(WorktreeError::UnmergedBranch {
                branch: preview.branch,
                head: preview.head,
            });
        }
        Some(preview.branch)
    } else {
        None
    };

    let pruned = self.remove_worktree_locked(worktree_path, false)?;
    if let Some(branch) = branch {
        git_branch_delete(&self.repo_root, &branch, destructive)?;
    }
    Ok(pruned)
}
```
Why it is slow: A coarse synchronous Mutex is held across up to 8 serial subprocess spawns, blocking all writer lease requests and unrelated worktree operations across the workspace.

---

### F-worktree-05
- Severity: Low
- File: src-tauri/src/worktree/git.rs:51
- Mechanism: Before executing `git worktree add`, `git_worktree_add` calls `validate_base_ref`, which executes `git rev-parse --verify --quiet <base_ref>^{commit}` as a separate subprocess. When creating a worktree with a base reference, this adds an extra sequential process spawn and fork/exec overhead before `git worktree add`, which itself validates the commit atomically during worktree creation.
- Hot path: yes
- Suggested fix: Validate ref format in-memory with pure Rust string checks and allow `git worktree add` to validate commit resolution directly, eliminating the redundant `git rev-parse` process spawn.
- Write scope: src-tauri/src/worktree/git.rs
- RED proof:
```rust
// src-tauri/src/worktree/git.rs
fn validate_base_ref(repo_root: &Path, base_ref: &str) -> Result<(), WorktreeError> {
    validate_git_value(base_ref, "Base ref")?;
    let commit_ref = format!("{base_ref}^{{commit}}");
    run_git(
        repo_root,
        &["rev-parse", "--verify", "--quiet", commit_ref.as_str()],
    )?;
    Ok(())
}
```
Why it is slow: Spawns an extra blocking Git subprocess per worktree creation when `base_ref` is supplied.

## Non-findings / accepted
- **Tokio runtime offloading**: All IPC commands in `src-tauri/src/ipc/worktree.rs` wrap blocking Git invocations and manager calls in `run_blocking(move || ...)`, ensuring that heavy subprocess I/O is dispatched to Tokio's dedicated blocking threadpool rather than stalling async reactor workers.
- **In-memory workspace lookup**: `WorkspaceRegistry` uses a `parking_lot::RwLock` around the workspace hash map, allowing fast read-side manager lookups without holding locks during downstream Git commands.
- **Dirty state change deduplication**: `WorktreeManager::observe_dirty_state` records previous dirty statuses in `dirty_snapshots: Arc<Mutex<HashMap<PathBuf, bool>>>`, emitting frontend `worktree_changed` IPC events only when dirty state actually changes.
- **In-memory ref and path validation**: `validate_git_value` and `validate_git_path_argument` perform string and control character checks in Rust memory before command dispatch, preventing malicious arguments without invoking child processes.

## Scan coverage
- `src-tauri/src/worktree/git.rs`: Subprocess execution helpers, porcelain parsers, worktree and branch Git commands.
- `src-tauri/src/worktree/manager.rs`: `WorktreeManager` lifecycle, writer lease registry, branch name formatting/parsing, path canonicalization, dirty state detection, deletion safety.
- `src-tauri/src/worktree/registry.rs`: `WorkspaceRegistry` workspace mapping, path and identity resolution, terminal target resolution.
- `src-tauri/src/worktree/mod.rs`: Worktree module exports and unit test suite.
- `src-tauri/src/worktree/model.rs`: Data models (`Worktree`, `DirtyState`, `CreateWorktreeOptions`, `WorktreeError`).
- `src-tauri/src/ipc/worktree.rs`: Tauri IPC command endpoints and event emitters.
- `src-tauri/tests/worktree_safety.rs`: Integration tests for worktree concurrency, writer leases, and deletion safety.
- `src-tauri/src/ipc/blocking_contract_tests.rs`: Tests verifying blocking IPC contract execution on separate threadpool.
- `src-tauri/Cargo.toml`: Backend crate dependencies and configuration.
