# A08 remaining wire seams - resume receipt

Date: 2026-09-13. Task: `st_01a09810`. Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`. Platform: Darwin arm64, headless library tests.

## Outcome and boundary

All three requested Q2 wire seams passed through one actual private `DaemonServer` owner:

1. Fault-free **first HTTP 201** for explicit `baseRef: HEAD~1`; response HEAD and actual checkout `git rev-parse HEAD` equal the base commit and differ from the repository's original HEAD.
2. Actual `DaemonClient` over private Unix socket returns `worktreeError / WORKTREE_REMOVED_PRUNE_FAILED`, preserving the nested `GIT_ERROR`, command `git worktree prune`, exit 128 and real Git stderr. Checkout is removed; the explicitly requested branch deletion does not run after prune failure, and the branch remains at its original commit.
3. Actual HTTP DELETE with a mirror-Control token returns **409**, with redacted partial details (`worktreeRemoved: true`, `pruned: false`, `branchDeleted: false`), no path or nested cause. Checkout is removed and branch remains at its original commit.

Requirements read: approved plan section 4.4 and A08; `WAVE1-resume-acceptance-gaps.md` Q2, section 7, and explicit non-HEAD/Local/legacy missing-proof rows. The approved plan was read as the explicitly named reference from the original checkout; code inspection and all source edits were confined to this assigned worktree.

This is proof-only coverage. It does not repeat transaction, manager, journal, or error-projection implementation. It does not claim original-owner restart recovery, storage-fault TCP behavior, forced relay, Linux/Windows, native Tauri, desktop, deployment, or full A08 acceptance. Parent owns composed batch verification after concurrent writers settle.

## Owned files and mechanism

- New `src-tauri/src/remote/workspace_api/worktree_wire_proof_tests.rs`.
- Only added the `cfg(all(test, unix))` path/module declaration near the top of `worktrees.rs`; no changes to its production body or existing authority tests.
- This receipt.

The test starts an exact-selected child of the library test executable, with private runtime/data/session/home/config paths. Its HTTP router and UDS connection handler use the same owning `DaemonServer` and shared workspace service. A real version-3 UDS handshake asserts the server PID equals the child PID. No adapter-return fixture or mocked HTTP response is used.

The existing `transaction_probe` observes `workspaceGateRequested` on the owning blocking worker for both Local and legacy paths. It installs the existing thread-local `manager::PRUNE_PROBE`; this changes private `.git/config` repositoryformatversion from 0 to 999 only around actual Git prune, then restores the original bytes before catalog observation/publication. No production fault hook was added. Every scenario checks restored format version and absent backup, actual checkout absence, and the retained branch's Git object ID. Publication receivers are subscribed before deletion and awaited with a bounded timeout. There are no sleeps or polling barriers. Sync filesystem/Git setup and inspection in async code use blocking workers (or Tokio filesystem wrappers).

The ordinary failure path clears the service probe, joins the UDS listener and connection tasks, gracefully joins HTTP, restores any remaining config backup, removes the socket, asserts TCP/UDS connection refusal, and drops the owner. The private runtime is dropped even after a caught scenario panic. `waitpid(-1, WNOHANG)` must report `ECHILD` in the isolated process. The parent waits/reaps that exact owner, removes the root through a blocking worker, and asserts its absence before checking success. No PTYs are created. The 180-second parent watchdog was not triggered in either recorded run; watchdog/process-kill behavior is not a newly proven acceptance claim.

## Commands and diagnostics

Initial status used `git status --short`; Git rejected the inherited `src-tauri/vendor/ghostty` symlink as a submodule. Status/diff inspection then used `--ignore-submodules=all`; no submodule changes were made. Status/diff and declaration lines were re-read immediately before edits. The workspace already contained concurrent, unrelated changes.

LSP before cargo:

- `lsp_diagnostics(worktree_wire_proof_tests.rs, all)` initially reported `unlinked-file` while the module tree caught up; this was an IDE limitation, not successful compilation.
- `lsp_diagnostics(worktrees.rs, all)`: no diagnostics.
- After the fixture-only registration correction, `lsp_diagnostics(worktree_wire_proof_tests.rs, all)`: no diagnostics.

Both monitored cargo attempts used this exact command from the assigned worktree:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
CARGO_BUILD_JOBS=3 RUSTC_WRAPPER= CARGO_TARGET_DIR=$PWD/src-tauri/target \
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features \
  --lib remote::workspace_api::worktrees::wire_proof_tests -- --nocapture
```

Parent archived both complete outputs and exit receipts in this directory:
`A08-wire-seams-resume-attempt1.log` / `.exit` (101) and
`A08-wire-seams-resume-GREEN.log` / `.exit` (0). Original temporary paths were
`/tmp/st_01a09810-wire.log`, `/tmp/st_01a09810-wire.exit`,
`/tmp/st_01a09810-wire-green.log`, and `/tmp/st_01a09810-wire-green.exit`.
Both initially waited on the shared build-directory lock; no lock holder was killed. No shared compile blocker remained when these tests compiled. The successful command built and executed `src-tauri/target/debug/deps/ferryx_lib-82e1988715e4a304`; it was not source-only evidence.

Cargo emitted 16 pre-existing warnings in `macos_file_drop.rs`, `remote/auth.rs`, `lib.rs`, `ipc/notifications.rs`, `native_terminal/sys/constants.rs`, `remote/server.rs`, and `worktree/manager.rs` (unnecessary unsafe, unused variables/dead code). They were not suppressed or repaired by this task. No test-module warnings appeared.

## Attempt 1: fixture mistake, not manufactured production RED

The first fixture registered a machine-only workspace. Non-HEAD and Local prune already passed, but legacy correctly returned 403 `MACHINE_ACCESS_REQUIRED` rather than the expected partial 409. Section 4.4 explicitly prohibits mirror access to machine-only roots. The only correction was using the existing Local `register` service entry point for a mirror-exposed workspace. No production mutation was used to manufacture RED.

Actual failure/cleanup excerpt:

```text
A08_WIRE_NON_HEAD first_http=201 base=dd1df0b29e54cd4e1bd06005b84e2f9dd7c36d5b original_head=a344b567db544af05069b13a8a7fdc2e39699ea5 actual_checkout_equal=true
A08_WIRE_PRUNE legacy=false removed=true branch_preserved=true config_restored=true publication_received=true
A08_WIRE_HTTP_RESPONSE status=403 body={"error":{"code":"MACHINE_ACCESS_REQUIRED","details":{},"message":"MACHINE_ACCESS_REQUIRED","requestId":"6834e0db-0d96-4569-9daa-0707e2fbf578","retryable":false}}
assertion `left == right` failed
  left: 403
 right: 409
A08_WIRE_LISTENERS joined=true clients_joined=true tcp_refused=true uds_refused=true socket_removed=true failed=true
A08_WIRE_OWNER pid=93318 runtime_joined=true git_children=ECHILD failed=true
A08_WIRE_PARENT owner_pid=93318 reaped=true status=exit status: 101 timeout=false root=/tmp/a08-wire-ERHs2r absent=true
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 953 filtered out; finished in 1.33s
```

## Successful wire and cleanup log

Exact runtime log excerpt from attempt 2 (authentication tokens are never printed):

```text
    Finished `test` profile [unoptimized] target(s) in 42.73s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-82e1988715e4a304)

running 2 tests
test remote::workspace_api::worktrees::wire_proof_tests::private_wire_owner ... ok

running 1 test
A08_WIRE_HANDSHAKE owner_pid=95639 exact=true socket=/tmp/a08-wire-2ND1zo/runtime/daemon.sock http=127.0.0.1:65385
A08_WIRE_HTTP_REQUEST method=POST endpoint=http://127.0.0.1:65385/api/v1/workspace/worktrees body={"baseRef":"HEAD~1","requestId":"8b8e109e-8baf-4160-bfd6-fcb5c0ad29d2","workspaceId":"wire-proof","worktree":{"slug":"non-head","wsId":"wire-proof"}}
A08_WIRE_HTTP_RESPONSE status=201 body={"bare":false,"branch":"refs/heads/orca/wire-proof/non-head","detached":false,"head":"9519d93718d017c8ef6f316e2674edff70c274cc","identity":{"slug":"non-head","wsId":"wire-proof"},"locked":null,"managed":true,"path":"/private/tmp/a08-wire-2ND1zo/repo/.orca-worktrees/wire-proof/non-head","prunable":null,"workspaceId":"wire-proof"}
A08_WIRE_NON_HEAD first_http=201 base=9519d93718d017c8ef6f316e2674edff70c274cc original_head=7b75b42b9d46da69176b76def075f75e499fd1a2 actual_checkout_equal=true
A08_WIRE_UDS_REQUEST {"baseRef":null,"type":"createWorktree","workspaceId":"wire-proof","worktree":{"slug":"local-prune","wsId":"wire-proof"}}
A08_WIRE_UDS_RESPONSE {"type":"createWorktreeOk","worktree":{"bare":false,"branch":"refs/heads/orca/wire-proof/local-prune","detached":false,"head":"7b75b42b9d46da69176b76def075f75e499fd1a2","locked":null,"path":"/private/tmp/a08-wire-2ND1zo/repo/.orca-worktrees/wire-proof/local-prune","prunable":null}}
A08_WIRE_UDS_REQUEST {"deleteBranch":true,"destructive":false,"type":"deleteWorktree","workspaceId":"wire-proof","worktree":{"slug":"local-prune","wsId":"wire-proof"}}
A08_WIRE_UDS_RESPONSE {"error":{"code":"WORKTREE_REMOVED_PRUNE_FAILED","details":{"branchDeleted":false,"cause":{"code":"GIT_ERROR","details":{"command":"git worktree prune","exitCode":128,"stderr":"fatal: Expected git repo version <= 1, found 999"},"message":"Git command failed (git worktree prune): fatal: Expected git repo version <= 1, found 999 (exit code: Some(128))"},"path":"/private/tmp/a08-wire-2ND1zo/repo/.orca-worktrees/wire-proof/local-prune","pruned":false,"worktreeRemoved":true},"message":"Worktree '/private/tmp/a08-wire-2ND1zo/repo/.orca-worktrees/wire-proof/local-prune' was removed but prune failed: Git command failed (git worktree prune): fatal: Expected git repo version <= 1, found 999 (exit code: Some(128))"},"type":"worktreeError"}
A08_WIRE_PRUNE legacy=false removed=true branch_preserved=true config_restored=true publication_received=true
A08_WIRE_UDS_REQUEST {"baseRef":null,"type":"createWorktree","workspaceId":"wire-proof","worktree":{"slug":"legacy-prune","wsId":"wire-proof"}}
A08_WIRE_UDS_RESPONSE {"type":"createWorktreeOk","worktree":{"bare":false,"branch":"refs/heads/orca/wire-proof/legacy-prune","detached":false,"head":"7b75b42b9d46da69176b76def075f75e499fd1a2","locked":null,"path":"/private/tmp/a08-wire-2ND1zo/repo/.orca-worktrees/wire-proof/legacy-prune","prunable":null}}
A08_WIRE_HTTP_REQUEST method=DELETE endpoint=http://127.0.0.1:65385/api/v1/workspace/worktrees body={"deleteBranch":true,"workspaceId":"wire-proof","worktree":{"slug":"legacy-prune","wsId":"wire-proof"}}
A08_WIRE_HTTP_RESPONSE status=409 body={"error":{"code":"WORKTREE_REMOVED_PRUNE_FAILED","details":{"branchDeleted":false,"pruned":false,"worktreeRemoved":true},"message":"WORKTREE_REMOVED_PRUNE_FAILED","requestId":"cc1da27b-ede0-4f5c-8857-ef1e844531c6","retryable":false}}
A08_WIRE_PRUNE legacy=true removed=true branch_preserved=true config_restored=true publication_received=true
A08_WIRE_LISTENERS joined=true clients_joined=true tcp_refused=true uds_refused=true socket_removed=true failed=false
A08_WIRE_OWNER pid=95639 runtime_joined=true git_children=ECHILD failed=false
test remote::workspace_api::worktrees::wire_proof_tests::private_wire_owner ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 956 filtered out; finished in 1.53s

A08_WIRE_PARENT owner_pid=95639 reaped=true status=exit status: 0 timeout=false root=/tmp/a08-wire-2ND1zo absent=true
test remote::workspace_api::worktrees::wire_proof_tests::non_head_and_partial_prune_wires ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 955 filtered out; finished in 1.55s
```

The outer module filter runs the private-child entry once without its environment (a no-op) and the real parent once. The parent invokes that exact private-child entry with its isolated environment, where all three wire seams execute. Therefore this is **one owning scenario covering three seams**, not three separately executed child scenarios; the counts above are not inflated into stronger acceptance. The different filtered-test counts between attempts reflect concurrent additions elsewhere in the shared worktree.
