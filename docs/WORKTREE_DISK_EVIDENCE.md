# Worktree Disk Management — verification evidence

Captured output for the work on this branch. Commands were run in this worktree with its own
`CARGO_TARGET_DIR`; the live daemon was never signalled, restarted, or killed.

## Commands and results

```
cargo check --manifest-path src-tauri/Cargo.toml                     exit 0
cargo test  --manifest-path src-tauri/Cargo.toml --lib disk          ok. 11 passed; 0 failed
bun run --cwd ui test src/components/WorktreeDiskDialog.test.tsx     8 passed
bun run --cwd ui build          (tsc && vite build)                  exit 0, 0 TS errors
```

Note `bun test --cwd ui` is **not** the runner for this repo — it fails with `vi.hoisted`
errors. The suite is vitest, invoked as `bun run --cwd ui test`. Type-checking is not part of
vitest; `bun run --cwd ui build` is what runs `tsc`.

Per-module breakdown:

```
worktree::disk_tests          ok.  7 passed; 0 failed; 1038 filtered out
ipc::worktree_disk_tests      ok.  3 passed; 0 failed; 1042 filtered out
```

## Mutation proof (failing-first substitute)

The implementation predates this evidence document, so a true pre-change RED could not be
recovered. The guards were instead proven by forcing the exact regression.

Cancellation is enforced by one function, `src-tauri/src/worktree/disk.rs:43`, called between
every filesystem operation:

```rust
pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), IpcError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(IpcError::new(IpcErrorCode::ScanCancelled, "Disk scan cancelled"));
    }
    Ok(())
}
```

Mutated so cancellation is never detected:

```diff
-  if cancelled.load(Ordering::Acquire) {
+  if false && cancelled.load(Ordering::Acquire) {
```

RED with the mutation present (exit 101):

```
test worktree::disk_tests::disk_scan_cancellation_stops_at_the_next_entry ... FAILED
    panicked at src/worktree/disk_tests.rs:37:6
test ipc::worktree_disk_tests::disk_scan_cancel_command_stops_a_live_walk_without_caching_partial_bytes ... FAILED
    panicked at src/ipc/worktree_disk_tests.rs:208:10
test result: FAILED. 9 passed; 2 failed; 1034 filtered out
```

GREEN after reverting (exit 0):

```
test result: ok. 11 passed; 0 failed; 1034 filtered out
```

### What the surviving test reveals

`disk_scan_cancelled_and_superseded_workers_cannot_publish_partial_cache` **still passed**
under the mutation. That is not a weak test — it guards a different layer.

Two independent defences stop a cancelled scan from publishing partial sizes:

1. **Detection** — `check_cancelled` aborts traversal at the next entry. The two tests that
   failed above cover this layer.
2. **Publication** — a superseded or cancelled worker is refused at the cache-write path
   regardless of how far it walked. The surviving test covers this layer.

Removing detection alone therefore does not produce a wrong cached size; the publication
guard still holds. The suite distinguishes the two, which is why only two of the three
cancellation-related tests move under this mutation.

The source file was restored from an in-memory copy and verified byte-identical; no
`git restore` / `git checkout --` was used, and the tracked tree was clean afterwards.

## Scope constraints, verified at source

- **No `du`.** `disk.rs` spawns no subprocess. Measurement is `fs::symlink_metadata` plus
  `checked_add`, so it is portable and cannot inherit a shell.
- **`run_blocking`.** Traversal is dispatched through `crate::ipc::run_blocking`, keeping
  synchronous disk I/O off async runtime threads.
- **Cancellable with progress.** Cancellation is checked between every entry
  (`disk.rs:90/104/112/115/131/199/205`). Individual OS calls cannot be interrupted, which the
  module header states plainly rather than implying finer granularity than exists.
- **Symlinks.** Never followed; a Windows reparse-point branch exists at `disk.rs:62`.
  Verified to compile for `x86_64-pc-windows-gnu` (exit 0), so the cross-platform premise is
  proven by compilation rather than assumed.
- **Cleanup reuses the existing flow.** No new delete command was added. The dialog imports
  the pre-existing `WorktreeDeleteDialog` (`WorktreeDiskDialog.tsx:36`, rendered `:664`),
  which drives `cmd_worktree_delete_preview` → `cmd_worktree_delete_destructive`. That
  component gained an `initialDirty` prop so the disk dialog can pre-seed known-dirty state
  instead of paying a redundant preview round-trip; the dirty branch is gated on the
  structured `DIRTY_WORKTREE` code, not string matching.

## Wording that matters

The size caption reads, verbatim (`WorktreeDiskDialog.tsx:370`):

> Apparent size is estimated from file sizes. Reclaimed disk space may vary due to filesystem
> deduplication and hard links.

This is deliberate. Apparent size double-counts APFS clones and hard links, so promising an
exact amount of reclaimable space would be false. The cleanup badge is advisory only and
pre-selects nothing.

## Not verified here

Desktop GUI end-to-end behaviour needs a real Tauri window, WGPU child surfaces and live
PTYs. It is not claimed. Manual steps are written up separately.

One caveat worth knowing before running the touched files: `ui/src/lib/tauri.test.ts` is red
on its own with 3 pre-existing failures, unrelated to this work. A tree that touched no UI at
all produces the same three failures by name. This branch adds one test to that file and it
passes (31 tests at base → 32 here, 28 passing → 29).

## Mutation proof: cancellation is PROMPT, not eventual

The scope headline requires a cancellable scanner. A test asserting only "a cancelled scan
returns ScanCancelled" would pass even if cancellation were checked once at the end - which
is precisely the failure that freezes the UI on a large tree. So the question is not whether
the test passes but whether its assertion bites.

`disk_scan_cancellation_stops_at_the_next_entry` writes 20 files, flips the cancel flag
inside the FIRST progress callback, and asserts `calls == 1`. It then re-scans uncancelled
and asserts 80 bytes / 20 files, so an early stop caused by a broken walker cannot be
mistaken for working cancellation.

Mutation in `src-tauri/src/worktree/disk.rs::scan_path` - removed the two per-entry
cancellation checks, leaving only the directory-branch one, making cancellation eventual
for file entries:

    - loop {
    -     check_cancelled(cancelled)?;
    -     ...
    -     check_cancelled(cancelled)?;
          if excluded_roots...

GREEN before: `test result: ok. 7 passed; 0 failed`
RED with mutation (exit 101):

    worktree::disk_tests::disk_scan_cancellation_stops_at_the_next_entry ... FAILED
    assertion `left == right` failed
      left: 21
     right: 1

21 progress callbacks means the walk continued through every entry after cancellation was
requested. Reverted; source restored byte-identical; 7 passed again.
