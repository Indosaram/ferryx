# REGRESSION2 - remaining three lib regressions

Task: st_01a099da. Work confined to `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`; no commits, destructive Git operations, release builds, desktop automation, or intentional interaction with canonical daemon/GUI/existing PTYs.

## Delivered

1. `daemon/server.rs`: the Unix profile-default path test saves, removes, and restores `FERRYX_RUNTIME_DIR` with a panic-safe Drop guard. Unix `get_runtime_dir` reads only this override; `is_dev_runtime` is compile-time and socket/lock paths delegate to runtime-dir. LOCALAPPDATA/TEMP occur only in the non-Unix implementation and are not read by this Unix-only test. All original expected path assertions remain unchanged.
2. `ipc/tests.rs`: the identity-contract fixture now manages a real socket-bound DaemonClient using the existing in-test DaemonServer accept-loop helper, registers the same temporary repository with the owner, and aborts/joins its listener afterward. Original assertions and distinct registry key `workspace-test` / branch namespace `ws-ipc` remain unchanged.
3. `daemon/server.rs`: removed implicit persistent Relay configuration from pairing-code creation. The actual chain was RemoteCreatePairingCode -> Off-to-Relay config mutation -> handle_remote_configure -> configure_gateway -> persist_config; the next constructor reloaded Relay, and production run could restore its listener. Pairing now mints credentials without changing network configuration. An explicitly started relay still supplies its pairing coordinator. Existing explicitly configured listener restoration is unchanged; this fix does not force every persisted enabled configuration Off.

## Additional cause exposed behind failure 2

Both cmd_worktree_create and cmd_worktree_delete resolve managed Arc<DaemonClient>, send CreateWorktree/DeleteWorktree, and require successful real daemon responses. The daemon delegates to workspace_api::worktrees::owner_mutation, which gates mutations, validates catalog membership, runs bounded Git, checks path containment/live sessions/locks, and publishes catalog revisions. A bogus socket could not satisfy the success assertions.

That local owner path also incorrectly imposed machine HTTP's equality between registry key and worktree branch namespace. The fixture alone would therefore only reveal a second error. `remote/workspace_api/worktrees.rs` now preserves independent local identities: catalog lookup and mutation gate use the registry key; validated branch namespace and path derivation use ws_id. Delete keeps the canonical path jail and selects the actual porcelain-listed path for lock/prunable metadata. Machine HTTP identity validation is untouched. RequestContext checks, CancelWork, deadlines, Git budgets/output bounds, and subprocess reaping are unchanged.

The first implementation used HEAD inspection for deletion metadata; validation caught WORKTREE_LOCKED becoming GIT_ERROR. This was corrected to porcelain listing, not by changing any assertion. The failed first run is retained.

## Verification

All Rust tests used `--test-threads=1`. Runner `REGRESSION2-run.py` gives tests private HOME, runtime, data, session, XDG, agent, and temp paths, and the required PATH. Cargo/Rustup use existing toolchain caches; target is the worktree target. No existing test was removed, ignored, renamed, or weakened.

Final run (PID 8371):

| Command | Result | Log |
| --- | --- | --- |
| cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --test-threads=1 | **1034 passed; 0 failed; 1 ignored**, exit 0 | REGRESSION2-full-8371.log |
| cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --test-threads=1 | **36 passed; 0 failed**, exit 0 | REGRESSION2-paired-host-8371.log |
| cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib | exit 0 | REGRESSION2-check-lib-8371.log |
| cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay | exit 0 | REGRESSION2-check-bins-8371.log |
| cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-relay | debug build, exit 0 | REGRESSION2-build-entrypoint-8371.log |

LSP diagnostics on all three changed Rust files: no diagnostics found; repeated on worktrees.rs after its correction. `git diff --check`: exit 0. `pairedDaemonProxyV1` remains false at daemon/client.rs:492.

Real surfaces exercised by the suite include Tauri command functions over a real private Unix socket, daemon worktree mutation, and pairing/restart over two private daemon listeners. No desktop manual QA was performed. The standalone relay `--help` invocation was also executed but is unsupported (exit 2, `unrecognized argument: --help`), preserved in REGRESSION2-relay-entrypoint-82016.log; it is not claimed as successful functional verification or included in GREEN. No standalone relay was started by that invocation.

`REGRESSION2-RED.log` explicitly preserves the parent baseline: 1031 passed, 3 failed, 1 ignored. First development run `REGRESSION2-full-82016.log`: 1033 passed, 1 failed, 1 ignored (lock metadata issue caused and fixed here). Its results remain in REGRESSION2-results-first-82016.json. `REGRESSION2-GREEN.log` contains only the final successful command logs. Final command metadata and isolated QA root are in REGRESSION2-results.json.

## Assumptions and boundaries

The plan is absent from this worktree; per the task, the parent's verified statement that it mandates no new starts-in-Relay behavior was trusted rather than reading the canonical checkout. The binding distinction is credential creation versus explicit listener configuration. Removal of implicit activation applies to the shared mirror/machine pairing branch; callers wanting remotely redeemable relay credentials must explicitly enable Relay first. No rollout gate was changed.
