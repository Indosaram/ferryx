# Review: Git worktree management + ferryx scope layer (Rust)

Scope reviewed: `src-tauri/src/worktree/`, `src-tauri/src/ferryx_scope/`, plus the two call sites that
drive them (`src-tauri/src/ipc/worktree.rs`, `src-tauri/src/remote/server.rs`). Path-jail logic in
`worktree/manager.rs` (`validate_path_components`, `ensure_canonical_inside_root`,
`canonical_allowed_path`) and in `ferryx_scope/history/mod.rs` (`load`, per-component symlink
rejection) was examined specifically for `..`, symlink, and absolute-input escapes; no escape was
confirmed, so no P0 is reported.

### [P1] `crop_png` indexes the decoded buffer with an unclamped `right`/`bottom`, panicking on fractional device scales
- Location: src-tauri/src/ferryx_scope/design/mod.rs:58
- Observed: `left`/`top` are clamped to the image (`(x.max(0.) * sx).floor().min(f64::from(info.width)) as u32`, line 56), but the far edges are not:
  `let right = ((x + width).min(viewport.width) * sx).ceil().max(0.) as u32;` (line 58) and the same shape for `bottom` (line 59).
  Those values then drive raw slicing: `cropped.extend_from_slice(&decoded[start..start + (right-left) as usize * channels]);` (line 72),
  with `start = (row as usize * frame.width as usize + left as usize) * channels` (line 71) over `for row in top..bottom` (line 70).
- Why it is wrong: `sx = f64::from(info.width) / viewport.width` (line 49), so a full-width selection computes `viewport.width * (info.width / viewport.width)`, which in binary floating point is frequently a hair above `info.width` (e.g. image 1812 px / viewport 1610.1054458029384 -> 1812.0000000000002), and `ceil()` then yields `info.width + 1`. The geometry gate on line 55 does not catch this: it only checks `|sx - dpr*zoom| <= 1/viewport.width`, and an adapter reporting the true effective scale passes it exactly. I sampled realistic fractional scales (1.25/1.5/1.75/2.0/2.4 with integer image widths) and ~2.4% of gate-passing combinations produce `right > info.width`. The result is an out-of-range slice index on line 72 (or a `start` past the end of `decoded` when `bottom > info.height`), i.e. a Rust panic on a full-viewport crop instead of an error. Note: `rg` over `src-tauri` and `ui` finds no production caller of `crop_png` today (only `src-tauri/tests/scoped_design.rs`), so this is a latent crash in the scope API rather than a live shipped path — hence P1, not P0.
- Minimal fix: clamp the far edges the same way the near edges are clamped: `let right = ((x + width).min(viewport.width) * sx).ceil().min(f64::from(info.width)).max(0.) as u32;` and the analogous `.min(f64::from(info.height))` for `bottom`.

### [P2] Remote gateway runs blocking `git` subprocesses directly on the async runtime
- Location: src-tauri/src/remote/server.rs:1153
- Observed: the axum handler `async fn delete_worktree(...)` (line 1129) calls
  `mgr.delete_worktree_and_branch(&worktree.path, payload.delete_branch.unwrap_or(false))` inline.
  The same pattern appears at `.create_worktree(options)` (line 1119) and `worktrees: mgr.list_worktrees().unwrap_or_default()` (line 535).
  All of these fan out to `run_git` in src-tauri/src/worktree/git.rs:105, which calls `.output()` — a synchronous `std::process::Command` wait.
  `run_blocking` appears exactly once in the whole file (line 2052, for `resolve_dist_dir`), while the Tauri IPC twin of the same operation is correctly wrapped: src-tauri/src/ipc/worktree.rs:158 `let pruned = run_blocking(move || { ... delete_worktree_and_branch_with_prune_status(...) })`, and `run_blocking` is `tokio::task::spawn_blocking` (src-tauri/src/ipc/mod.rs:34).
- Why it is wrong: `git worktree add`/`remove`/`list` on a large repository or a slow/network mount takes seconds. Each such request parks a tokio worker thread inside the HTTP handler, so concurrent remote clients (terminal streams, polling status endpoints served by the same runtime) stall for the duration. It also means `delete_worktree_and_branch` holds `delete_lock` — a blocking `parking_lot::Mutex` (manager.rs:780) — from an async context, so a second remote delete blocks a worker thread outright rather than yielding.
- Minimal fix: wrap the three call sites in `crate::ipc::run_blocking(move || ...).await` (or `tokio::task::spawn_blocking`), exactly as `src-tauri/src/ipc/worktree.rs` already does.

### [P2] Rescan's "bounded" workspace scan does not actually bound anything; it leaks blocking threads
- Location: src-tauri/src/worktree/rescan.rs:550
- Observed: `let scan = tokio::time::timeout(WORKSPACE_SCAN_TIMEOUT, run_blocking(move || { Ok(sweep_workspace_with_probe(...)) })).await;`
  The doc comment above promises "Each workspace is scanned in its own bounded `run_blocking` job" and that
  "a hung scan for one workspace delays (bounded by the timeout) but never blocks the others" (lines 528-532; `WORKSPACE_SCAN_TIMEOUT` is 30s, line 526).
- Why it is wrong: `run_blocking` is `spawn_blocking` (src-tauri/src/ipc/mod.rs:34). Dropping the `JoinHandle` on timeout cancels nothing — the blocking closure keeps running and the `git` child process it spawned via `run_git` keeps running. On a wedged mount, every 30s tick (`WORKTREE_RESCAN_INTERVAL`) starts another permanently-stuck blocking task for the same workspace. Those threads accumulate against tokio's blocking-pool cap (512 by default), after which every other `run_blocking` in the app — every worktree IPC command — queues behind them and the UI's git operations stop responding. The timeout gives the appearance of containment while the resource actually leaks without bound.
- Minimal fix: make the scan skippable rather than re-entrant — keep a per-workspace "scan in flight" flag (or an `Arc<AtomicBool>`/`tokio::sync::Semaphore` permit held by the closure) and skip that workspace's tick while a previous sweep is still running, so at most one blocking thread per workspace can ever be outstanding.

### [P2] `parse_status_porcelain` stores git's C-quoted/escaped paths verbatim and mis-splits rename entries
- Location: src-tauri/src/worktree/git.rs:288
- Observed: the v1 fallback branch does `let path = line[3..].trim().to_string();` (line 288, guarded by `if line.len() >= 3`, line 286) and the v2 branches do `path: parts[7..].join(" ")` (line 246) and similar. The command is `run_git(worktree_path, &["status", "--porcelain"])` (line 400) — no `-z`, and no `-c core.quotePath=false`.
- Why it is wrong: with git's default `core.quotePath=true`, a path containing non-ASCII or control bytes is emitted C-quoted and octal-escaped. I verified against real git: a file named `a\n€b.txt` yields the literal porcelain line `?? "a\n\342\202\254b.txt"`. That raw string is what lands in `DirtyFile.path`, is carried into `WorktreeError::DirtyWorktree { files }` (manager.rs:665-669), and is what the user is shown when a delete is refused — so a user with any non-ASCII filename sees mojibake instead of the file blocking their delete. The same fallback also parses rename lines (`R  old -> new`) into a single bogus path `old -> new`.
- Minimal fix: pass `-c core.quotePath=false` (and prefer `--porcelain=v2`) in `git_status_porcelain`, and split the v1 rename form on `" -> "`, keeping the destination path.

Summary: P0=0, P1=1, P2=3, P3=0
