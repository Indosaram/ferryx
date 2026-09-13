# A08 missing-checkout rich deletion preview

Task `st_01a0980b`, 2026-09-12. Scoped implementation of approved section 4.4 / A08 and Q3 in `WAVE1-resume-acceptance-gaps.md`. This is not whole-A08, native-client, platform, or aggregate acceptance.

## Delivered contract

The authenticated machine GET `/api/v1/workspace/worktrees/status` now returns 200 metadata for a listed, identity-derived managed worktree whose checkout is missing, including a locked missing checkout that Git does **not** mark prunable. It reports the examined revision, Git locked/prunable values, actual live session IDs, and repository-ref branch deletion metadata when resolvable.

For A14's future typed client:

- Existing checkout success retains its existing response shape and dirty files/count semantics.
- Missing/prunable checkout: `dirtyInspection: "unavailable"`, `dirtyFiles: null`, `dirtyCount: null`. Null is **unknown**, never clean, zero dirty files, or permission to delete.
- `branchDeletion` is the existing branch-preview object when repository refs resolve. The commit is resolved from `refs/heads/<derived-managed-branch>^{commit}`; merge/upstream/ahead/behind reuse the existing manager mechanics.
- If branch metadata cannot resolve, `branchDeletion: null` and `branchDeletionError` contains a machine error code (the exercised deleted-ref case is `INVALID_WORKTREE`). There is no fabricated `merged: true`. A resolved object has `branchDeletionError: null`.
- Deadline, authorization and output-budget errors remain request failures. Unsafe or inaccessible jail components fail closed; an inspection failure on an existing checkout remains an error, not a successful clean scan. This change does not claim rich recovery through an inaccessible registered root.
- A 200 preview describes metadata, not deletion eligibility. DELETE still requires its existing canonical checkout resolver and all gated revision/locked/live/dirty/merge rechecks.

The preview validates identity and matches the listed row to the manager-derived path. It walks only normal components under the registered canonical root, checks symlink metadata before following each component, verifies canonical equality for existing directories, and stops at the first absent component. Inside-root aliases, outside-root links and dangling links are rejected. No arbitrary preview path field was added. No mutation implementation was changed.

## Owned source changes

- `src-tauri/src/remote/workspace_api/worktrees.rs`: preview read body only; missing-checkout jail walk and explicit unavailable projection. Preserve independently owned module declarations and all mutation code.
- `src-tauri/src/worktree/manager.rs`: extract repository-ref branch preview reuse; keep checkout validation in the existing public checkout-based entry point. Inherited dirty/prune/base-ref repairs remain untouched.
- `src-tauri/src/remote/workspace_api/worktree_authority_tests.rs`: extend only the old prunable-preview assertion to truthful 200/null semantics and actual branch commit/merge state. Preserve both catalog/journal storage-failure and replay assertions.
- `src-tauri/tests/machine_prunable_preview.rs`: new real loopback HTTP fixture, registered private Git root, normal and missing/locked worktrees, actual live private shell sessions, unmerged refs, deleted refs, identity mismatch, inside/outside/dangling symlinks and guarded DELETE.

## Failing-first HTTP evidence

`A08-prunable-resume-RED.log`, exit 101, was captured **before** the production preview change. The fixture registers the repository over HTTP, creates the worktree over HTTP, makes an actual unmerged Git commit, removes the checkout on disk, then performs the real HTTP GET.

```text
A08 HTTP missing locked=false status=422 body={"error":{"code":"WORKTREE_PREVIEW_UNSUPPORTED",...}}
assertion `left == right` failed: registered missing checkout must return truthful rich preview, not unsupported
  left: 422
 right: 200
A08 cleanup listener_joined=true connection_refused=true owner_dropped=true private_root_removed=true no_pty_spawned=true
test result: FAILED. 0 passed; 1 failed
```

The exact unabridged body/request ID is in the log. Initial fixture compile mistakes (private pairing API, reqwest without JSON feature) were corrected before this HTTP RED; these were not represented as contract evidence. A later additive live-session fixture compile mistake (`new(...).unwrap()`) was corrected to `try_new(...).unwrap()` before its GREEN. No failing assertion was skipped.

## Commands and results

All commands run from `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8` with:

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=3 RUSTC_WRAPPER=
export CARGO_TARGET_DIR=$PWD/src-tauri/target
```

| Command | Actual result / artifact |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_prunable_preview -- --nocapture` | RED: exit 101 as above. Final expanded fixture: exit 0, 1 passed; `A08-prunable-resume-live-GREEN.log`. |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test machine_prunable_preview --test machine_worktrees --test worktree_safety -- --nocapture` | Exit 0; initial preview fixture 1, existing HTTP 2, safety 9 passed; `A08-prunable-resume-GREEN.log`. Final additive preview coverage is in the separate live GREEN log. |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::workspace_api::worktrees::authority_tests -- --nocapture` | Exit 0; 10 passed; `A08-prunable-resume-authority-GREEN.log`. Both storage-failure paths now report `prunable_preview=200 dirty_inspection=unavailable`, retaining actual non-HEAD and replay checks. |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree:: -- --nocapture` | Exit 0; 44 passed; `A08-prunable-resume-manager-GREEN.log`. |
| `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay` | Exit 0; `A08-prunable-resume-build.log`. Existing compiler warnings remain visible, not suppressed. |
| `git -c diff.ignoreSubmodules=all diff --check` | Exit 0. The submodule override is required because inherited `vendor/ghostty` is a symlink. |

LSP diagnostics were attempted on all four owned Rust files before Cargo. Manager, old authority test and new integration test reported no errors; the concurrent worktrees request was canceled by the language server. Compiler/test/build results above are authoritative; LSP also missed fixture errors subsequently caught and fixed by Cargo. Build-lock contention was allowed to queue; no lock holder was killed.

## Final HTTP GREEN and cleanup

`A08-prunable-resume-live-GREEN.log` contains actual 200 response bodies for both cases:

- Prunable: `locked: null`, `prunable: "gitdir file points to non-existent location"`, actual unmerged commit, `merged: false`, null dirty fields, and live session `f9047e63-fc81-4f4f-b5c5-43835f3152e2`.
- Locked missing: `locked: "A08 lock"`, `prunable: null`, actual unmerged commit, `merged: false`, null dirty fields, and live session `6062249e-3c98-4954-aa9e-78f99af44ccd`.
- Both missing targets: DELETE 404. Three symlink preview replacements per target: 400; every corresponding DELETE remains unsuccessful. Wrong workspace identity: 400.
- Removing each actual branch ref yields 200 with `branchDeletion: null`, `branchDeletionError: "INVALID_WORKTREE"`, and dirty inspection still unavailable.

Sessions use private `/bin/sh` processes with an output readiness event subscribed before waiting and a blocking shell `read`, not sleeps or polling. Cleanup explicitly awaits each session close under a ten-second bound, verifies the session is absent, gracefully shuts down/joins the private TCP listener under a ten-second bound, verifies connection refusal, drops the owner and explicitly removes/verifies absence of the private root. The initial failing assertion also ran cleanup. `A08-prunable-resume-cleanup.log` extracts the actual cleanup receipts from RED and final GREEN.

No canonical daemon, developer PTY, desktop, deployment, release, or commit was used. All changes remain uncommitted in the shared worktree. Aggregate acceptance belongs to the parent verifier; this evidence does not replace Linux/Windows, real Tauri/native A14 handling, external-event/capability, or release-environment gates.
