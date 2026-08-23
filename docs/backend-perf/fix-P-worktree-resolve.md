# Fix: P-worktree-resolve (F-worktree-01, F-worktree-03)

Lead-implemented after the DAG node stayed queued.

## Change
- `git.rs`: `inspect_worktree` (rev-parse + symbolic-ref on one path); `git_branch_is_ancestor_of_head` (`merge-base --is-ancestor`)
- `manager.rs`: `find_worktree_by_slug` builds `.orca-worktrees/<ws>/<slug>` and inspects that path instead of `git worktree list`; `branch_is_merged` uses ancestor check

## GREEN
```
cargo test --manifest-path src-tauri/Cargo.toml --lib worktree::
```
13 passed including `git_merge_base_is_ancestor_and_branch_merge_detection` and `resolve_worktree_by_path_and_identity`.
