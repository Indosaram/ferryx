# Terminal Session Persistence Remediation

Date: 2026-08-24

Authoritative review: `docs/TERMINAL_SESSION_PERSISTENCE_CODE_REVIEW.md`

Governing design: `markdown/TERMINAL_SESSION_PERSISTENCE_FUNDAMENTAL_SOLUTION.md`

## Scope and preservation

This remediation addresses M-08 and H-01 through H-07 without resetting or replacing the existing uncommitted terminal-session-persistence implementation. Several reviewed fixes were already present in the working tree when this remediation pass began; those paths were preserved and regression-verified rather than rewritten. The remaining retry/trust defects were completed in `src-tauri/src/daemon/client.rs`, and verification exposed one recovered-close edge in `src-tauri/src/terminal/pty.rs` that was corrected without changing the bounded-close contract.

The deliberately exported-but-unwired `ensureSessionBackends` / `REBIND_SESSION_BACKEND` explicit-respawn utility remains untouched. No cleanup, reset, checkout, restore, commit, branch, or worktree operation was performed.

## M-08 — last-tab replacement ordering

### Fix

Closing the last terminal tab now follows the required lifecycle ordering: resolve the closing session's worktree, await `closeBackendSessionAndWait`, destroy terminal hosts, create a replacement with `createSpawnedTab(worktree, closingTab.label)`, then dispatch `CLOSE_TAB` with that replacement. Closing the sole browser tab resolves `closingTab.worktreePath`, creates a terminal replacement, awaits browser close, and dispatches `CLOSE_TAB` with the replacement. `createSpawnedTab` remains in the callback dependency list.

### Implementation and regression files

- `ui/src/state/workspaceStore.ts`
- `ui/src/state/workspaceStore.test.tsx`
- `ui/src/state/workspaceStore.browserLifecycle.test.tsx`

### Regression coverage

- `spawns a last-tab replacement only after lifecycle-confirmed writer release`
- `creates a replacement terminal tab and closes browser when closing the sole browser tab`
- sole-browser lifecycle regression in `workspaceStore.browserLifecycle.test.tsx`

## H-01 — replay-gap semantics survive Tauri and UI transport

### Fix

The Tauri output contract carries a distinct replay-gap boundary rather than flattening lag into ordinary output. The replay-gap payload carries the requested/available sequence boundary, replay start/end sequence, daemon epoch, and base64 history. UI event handling treats that frame as a reset boundary: pending output/decoder sequence state is cleared, xterm is reset, replay history is decoded and written, and only then does sequenced live output resume.

### Implementation and regression files

- `src-tauri/src/ipc/terminal.rs`
- `ui/src/lib/terminalEvents.ts`
- `ui/src/lib/terminalOutputScheduler.ts`
- `ui/src/lib/terminalFit.test.ts`

### Regression coverage

- `resets and replays history across a forced runtime replay gap before resuming sequenced live output`

## H-02 — ambiguous mutating requests are not transparently resent

### Fix

`DaemonClient` now tracks whether a failed request attempt may already have been delivered. Only retry-safe requests are transparently reconnected/retried: handshake, ping, list, describe, load, plus `Spawn`, whose `clientRequestId` makes it idempotent. Mutating requests such as write/signal/resize/close/save/clear/shutdown/workspace registration and remote mutation are not resent after an ambiguous post-write transport failure.

When delivery may have occurred, the client returns an explicit structured IPC error with `details.type = "ambiguousDelivery"` and `details.requestType`, retaining the underlying cause instead of silently executing the mutation again.

### Implementation and regression files

- `src-tauri/src/daemon/client.rs`

### Regression coverage

- `test_mutating_request_is_not_retried_after_ambiguous_delivery`: the server receives `Write` and drops the response; the client reports ambiguous delivery and no second connection receives another `Write`.
- `test_retry_safe_read_is_retried_after_ambiguous_delivery`: a lost `Ping` response causes one reconnect and a successful retry.

## H-03 — restore list failure is not interpreted as an empty live set

### Fix

Workspace restore no longer converts daemon/list failure into `[]`. The failure propagates to the restore coordinator, which enters `failed`; no replacement sessions are spawned and persisted backend mappings are not rewritten as exited based on an unauthoritative empty set.

### Implementation and regression files

- `ui/src/state/workspaceRestore.ts`
- `ui/src/state/workspaceRestore.test.tsx`

### Regression coverage

- `propagates daemon list failure instead of treating it as an empty live-session set`

## H-04 — remote WebSocket output is sequence/replay aware

### Fix

The daemon-owned remote gateway attaches through `attach_with_sequence`, emits sequence metadata on terminal frames, and records the last emitted sequence. If the broadcast receiver lags, it reattaches/replays from that boundary and emits an explicit replay/reset gap frame when history eviction requires a reset. Lag is never silently continued past.

### Implementation and regression files

- `src-tauri/src/remote/server.rs`
- `src-tauri/src/remote/tests.rs`

### Regression coverage

- `test_remote_terminal_forced_lag_replays_with_explicit_gap_and_sequence_metadata`

## H-05 — terminal close remains bounded for TERM-resistant processes

### Fix

Close uses a bounded TERM grace period independent of PTY-reader shutdown, escalates to process-group KILL, performs a second bounded reap, then performs bounded reader cleanup. A best-effort TERM group-signal error no longer turns a successfully escalated and reaped close into a false failure; this matters after job-control transitions such as VINTR/SIGINT where the original process-group target may have changed. Sibling terminal sessions remain untouched.

### Implementation and regression files

- `src-tauri/src/terminal/pty.rs`

### Regression coverage

- `close_escalates_term_ignoring_process_group_and_reaps_bounded_without_touching_sibling`: `/bin/sh -c 'trap '' TERM; sleep 30'` is closed within the bound, reaped, and a sibling session stays alive.
- `close_after_interrupt_still_succeeds_via_escalation_and_reap`: an interrupted shell still closes successfully and is reaped through the fallback path.

## H-06 — production daemon socket is validated before connect

### Fix

The server's shared runtime-node validator enforces the section-9 filesystem trust contract: runtime directory ownership/type/mode, socket symlink/type/UID checks, and secure node modes. `DaemonClient` now invokes that validator before any connect to the fixed production daemon endpoint and validates the post-readiness node again before the final connect. The client does not perform stale-socket deletion; stale removal remains server-owned and lock-gated.

`new_with_socket` is the repository's dependency-injection/test harness path for arbitrary temporary UDS endpoints, so the fixed `/tmp/rorca-{uid}` runtime-directory policy is applied to `get_socket_path()` rather than imposing production directory-mode semantics on unrelated injected test sockets.

### Implementation and regression files

- `src-tauri/src/daemon/server.rs`
- `src-tauri/src/daemon/client.rs`

### Regression coverage

- `test_client_rejects_symlinked_socket_before_connecting`
- `test_client_rejects_wrong_uid_runtime_dir_before_connecting`

## H-07 — spawn idempotency key is owned by the logical UI action

### Fix

`SpawnTerminalRequest` accepts an optional `clientRequestId`. The UI logical spawn boundary generates one stable ID and reuses it across renderer/Tauri retries for that same logical action. Rust uses the supplied key when present, with the daemon TTL dedupe remaining the final idempotency guard; a new Tauri invocation therefore does not automatically mint a second shell identity.

### Implementation and regression files

- `src-tauri/src/ipc/terminal.rs`
- `ui/src/lib/tauri.ts`
- `ui/src/state/workspaceStore.ts`
- `ui/src/lib/tauri.test.ts`
- `ui/src/state/workspaceStore.browserLifecycle.test.tsx`

### Regression coverage

- `reuses the same clientRequestId when one logical terminal spawn retries after an ambiguous renderer transport failure`
- `spawns terminals with caller-stable clientRequestId, workspace/worktree identity, and an optional cwd slot`

## Verification matrix

The complete required verification pass immediately preceding this report succeeded. The same five commands are run once more after adding this report so completion evidence is tied to the final workspace revision.

| Command | Exit code | Result |
| --- | ---: | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 0 | 204 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml --tests -- --test-threads=1` | 0 | all unit/integration suites passed (lib 204 plus main/integration suites) |
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | 0 | check completed successfully |
| `bun run --cwd ui test` | 0 | 79 test files passed; 576 tests passed |
| `bun run --cwd ui build` | 0 | `tsc && vite build` completed successfully; 1721 modules transformed |

The current UI count is higher than the 561-test baseline cited in the review task because the working tree already contains additional remediation/regression coverage.

Known dead-code warnings in adjacent terminal/worktree support code are pre-existing and outside this remediation scope; they do not affect the exit status or the persistence invariants above.
