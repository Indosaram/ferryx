# Blocker 3: completed paired runtime owners

## Delivered

The native actor now owns a drop guard that removes its exact owner-map entry on termination, including EOF, remote exit, keepalive failure, cancellation and unwinding. The guard holds only a weak map reference, avoiding a runtime/task ownership cycle. A per-install identity prevents a retired actor from deleting a replacement for the same target. Installation and removal use the existing map mutex. The proxy and command receiver are dropped before reaping; removal releases the stored sender and task handle.

No automatic reinstallation or local PTY fallback is introduced. A command future that captured the old sender fails with `RemoteFailure { kind: Disconnected, message: PAIRED_PROXY_UNAVAILABLE }`; a lookup after reaping returns `PtyError::Other(PAIRED_PROXY_MISSING)`.

Changed by this packet:
- `src-tauri/src/terminal/paired_runtime.rs`
- `src-tauri/src/paired_host/proxy_tests.rs` (spontaneous termination fixture/test additions)
- This report and the RED/GREEN logs.

The parallel packet's `terminalStreamV1` fixture capability is present in the composed fixture and was not reverted. This packet did not edit `paired_host/client.rs` or `remote/machine_protocol.rs`.

## Failing-first evidence

`GATEFIX-owner-RED.log` preserves the full cargo output before production reaping was added. Both actual WebSocket EOF and remote lifecycle exit failed at the raw map assertion:

```text
completed owner retained in map
...
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1028 filtered out; finished in 0.07s
error: test failed, to rerun pass `--lib`
```

Command exit: 101. The original probe awaited the actual actor JoinHandle before asserting map removal, so failure did not depend on scheduling or delayed cleanup.

## Green evidence

`GATEFIX-owner-GREEN.log` contains full output and independently captured exit codes for:

1. `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --test-threads=1`: **36 passed, 0 failed**, exit 0.
2. `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: exit 0.
3. `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit 0.

Language-server diagnostics: no diagnostics on both changed Rust files (the first runtime request was cancelled; the subsequent request succeeded).

Existing unrelated compiler warnings remain visible in the logs; none were suppressed. Cargo test builds and executes the real native actor through a private loopback HTTP/WebSocket fixture, without launching a PTY or the user's daemon/GUI. Both spontaneous tests subscribe to cleanup before signaling termination through a oneshot. A bounded watch wait observes cleanup after map removal; assertions inspect the raw map and weak sender strong count (zero), then verify socket/hub removal, no PTY sessions, and private server shutdown. The captured pre-termination command fails closed before its sender-release assertion.

EOF and remote exit are exercised directly. Keepalive failure shares the same unconditional drop guard but is not independently forced by these tests. No sleeps, polling loops, or timing-based success conditions were added.

## Constraints and assumptions

All project reads/writes and cargo execution were confined to `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`; unique `/tmp/gatefix-owner-*` logs were used as explicitly authorized. No commits, destructive git operations, release builds, desktop automation, real HOME changes, or interactions with existing PTYs/daemon/GUI were performed. Required Tauri inputs and build caches were preserved.

The deliberately disabled `pairedDaemonProxyV1` remains false at `daemon/client.rs:492`; the false assertions remain at `paired_host/process_tests.rs:121` and `native_operation_tests.rs:162`. Assumption: explicit installation of a new proxy is allowed; only a stale actor must be prevented from removing that replacement, and stale sends must never target it.
