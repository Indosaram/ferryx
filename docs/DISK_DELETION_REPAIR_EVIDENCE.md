# Disk deletion transaction repair evidence

Captured 2026-09-13 in `/Users/indo/code/project/orca-lite-wt/sa-worktree-disk`.
No commits were created, including fixture commits. Git fixtures clone existing
repository objects with `--no-hardlinks --no-checkout`, then use existing HEAD and
HEAD~1 to construct unmerged branches. All temporary directories, build output,
and logs are inside this worktree. No daemon actions or GUI automation were used.
Shared dependency symlinks were not changed; Vitest ran with `--no-cache`.

## Contract and implementation

The existing preview response now requires `dirtyState` and `missing`:

```json
{"dirtyState":{"isDirty":true,"files":[{"statusCode":"??","path":"repair-untracked.txt"}]},"missing":false}
```

Existing branch metadata remains. File count is the current `files.length`.
The existing preview, safe-delete, and destructive-delete command routes and
service method signatures remain unchanged. Preview Git/status operations and
deletion run inside `run_blocking`.

Deletion-only record resolution validates the managed identity, exact expected
path, actual Git record, prunable status for missing directories, and nearest
existing ancestor. Ordinary worktree/terminal resolution still requires an
existing canonical directory. `git worktree remove -- <path>` removes only the
selected record; deletion no longer globally prunes unrelated records.

The delete dialog uses fresh preview file details, blocks destructive deletion
without a successful preview, and invalidates/refreshes preview when safe deletion
reports DIRTY_WORKTREE or UNMERGED_BRANCH. Cached dirty props cannot authorize loss.

Successful command deletion removes the path from both current and completed scan
snapshots under the publication mutex and cancels any in-flight worker. Its stale
completion cannot republish the deleted row. This does not emit a replacement
disk-scan snapshot. The lead owns the disk dialog's effect-local `current` update;
those foreign files were not edited by this repair worker.

## RED before production changes

From worktree root:

```sh
export TMPDIR="$PWD/target/deletion-repair/tmp"
export CARGO_TARGET_DIR="$PWD/src-tauri/target"
cargo test --manifest-path src-tauri/Cargo.toml --lib deletion_repair -- --test-threads=1
```

`target/deletion-repair/logs/backend-red.log`: exit 101, **0 passed; 3 failed**.

- Missing managed record preview: `Err(IpcError { code: WorktreeNotFound, ... })`
  at the assertion requiring successful preview of the actual prunable Git record.
- Current dirty preview: `left: Null`, `right: false` at the required current
  dirty-state assertion.
- Successful deletion/cache: `pre-delete worker must not resurrect deleted row`.

Before UI production changes:

```sh
cd ui
./node_modules/.bin/vitest run src/components/WorktreeDeleteDialog.test.tsx \
  -t 'gates destructive|uses fresh|invalidates preview' --maxWorkers=1 --no-cache
```

`target/deletion-repair/logs/ui-red.log`: exit 1, **3 failed; 8 deselected**.

- Preview rejection: destructive button was not disabled.
- Fresh loss preview: `dirty-file-preview` was absent despite current dirty files.
- Safe refusal refresh: expected preview spy called 2 times, received 1.

The new UI tests use awaited React `act` and a directly controlled preview promise,
not sleeps or polling. Existing delete-dialog tests were converted from polling
to awaited React work while retaining their behavioral assertions.

## GREEN and build

Backend regression command above, after fixes and again after Rust formatting:

```text
deletion_repair_missing_record_preview_and_targeted_cleanup ... ok
deletion_repair_preview_reports_current_dirty_and_unmerged_loss ... ok
deletion_repair_success_removes_cached_row_and_blocks_stale_worker ... ok
test result: ok. 3 passed; 0 failed
```

Logs: `backend-green.log`, `backend-final.log` in `target/deletion-repair/logs`.
The missing-record test verifies other in-root and outside-root prunable Git
records survive targeted cleanup. The preview test changes clean to dirty while
the branch is unmerged, asserting actual serialized current status/files. The cache
test performs real deletion through the existing command, then attempts stale
publication and checks both result lookup and cached reopen.

```sh
GIT_CEILING_DIRECTORIES="$TMPDIR" cargo test --manifest-path src-tauri/Cargo.toml \
  --lib ipc::worktree_disk_tests -- --test-threads=1
cargo check --manifest-path src-tauri/Cargo.toml
cd ui
./node_modules/.bin/vitest run src/components/WorktreeDeleteDialog.test.tsx \
  src/components/WorktreeDiskDialog.test.tsx \
  src/components/WorktreeDiskDialog.lifecycle.test.tsx --maxWorkers=1 --no-cache
./node_modules/.bin/tsc --noEmit
./node_modules/.bin/vite build --outDir ../target/deletion-repair/ui-dist
```

- Cache tests: **3 passed; 0 failed** (`cache-tests-green.log`). An initial run
  without Git's discovery ceiling failed `completed.rows.is_empty()` because its
  plain-directory fixture inherited this enclosing Git worktree. The rerun bounds
  Git discovery at the local temp root; no test or production workaround was added.
  Original failure retained in `cache-tests.log`.
- Cargo check: **exit 0**, 16 existing warnings, including unrelated unused imports,
  unnecessary unsafe blocks, and unused writer-lease code (`cargo-check-final.log`).
- UI: **31 passed across 3 files**, exit 0 (`ui-green.log`).
- TypeScript: **exit 0**, no output (`tsc.log`).
- Vite build: **exit 0**, built successfully; existing >500 kB chunk-size warning
  retained (`ui-build.log`). This is a UI asset build, not a release application build.
- Language-server diagnostics initially returned **No diagnostics found** for all
  eight owned changed Rust/TS/TSX files. After Rust-only formatting, repeat requests
  for the four formatted Rust files were cancelled/timed out by the server; the
  subsequent compiler check and regression tests passed. No diagnostic suppression.
- `git diff --check`: exit 0.

No full library suite was run: existing unrelated Git fixtures create commits.
The real command functions, Git removal, cache state, and rendered React dialog
interactions were exercised; desktop GUI/daemon behavior was not exercised.
