# Client gate fixes

Both requested blockers are fixed in `src-tauri/src/paired_host/client.rs`.

## Changes

- HTTP error envelopes are projected into a known-code allowlist, a fixed locally owned message, and typed details. Unknown codes become `PAIRED_HOST_REMOTE_ERROR`. No remote message, arbitrary detail key, nested object, or string detail is forwarded.
- The detail allowlist is limited to boolean `worktreeRemoved`, `branchDeleted`, and `pruned` for the two partial-worktree failure codes. Other errors have empty details. The structured machine error contract remains intact; retryability stays boolean and request IDs use the existing UUID-constrained decoder.
- Completed journal error outcomes use the same projection, closing the alternate path for the same remote error DTO.
- Attachment requires both `terminalCreateV1` and `terminalStreamV1` from the same authenticated capability response, before session lookup and socket connection. Missing stream support returns structured `PAIRED_HOST_CAPABILITY_UNAVAILABLE`.
- The successful socket fixture now advertises both capabilities.
- Added three regressions: malicious HTTP error text/details and unknown code; journal error projection; create-only peer with an atomic socket-attempt counter that must remain zero. Tests use real loopback HTTP/client paths, no sleeps or polling. Existing successful socket/native actor tests exercise real WebSocket attachment and input/output without a local PTY.

## Failing-first evidence

`GATEFIX-client-RED.log` is verbatim output captured before implementation changes, including exit status 101. Both new blocker regressions failed:

```text
assertion `left == right` failed: create-only peer received a socket request
  left: 1
 right: 0
```

The other failure prints the native serialized error containing `Bearer fixture-secret` and secret-bearing details. The token is a test fixture, not a real credential. Result: 0 passed, 2 failed.

## Green verification

`GATEFIX-client-GREEN.log` contains the complete output and independently captured exit status for each command:

1. `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --test-threads=1`: **36 passed, 0 failed**, exit 0.
2. `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: exit 0.
3. `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit 0.

All ran with the required PATH. Cargo warnings are retained in full, not suppressed. LSP diagnostics on all three changed Rust files returned no diagnostics (one intermediate test-file diagnostics request timed out; the final request succeeded).

The production `pairedDaemonProxyV1` remains false at `daemon/client.rs:492`; the false assertions remain at `paired_host/process_tests.rs:121` and `native_operation_tests.rs:162`. Those files were not edited.

## Scope and assumptions

Only this assigned worktree's client, client tests, proxy fixture, and these evidence files were written. No commits, release builds, desktop automation, or operations against the canonical daemon, GUI, or running PTYs were performed. Existing build inputs were not deleted.

The implementation assumes the existing protocol's UUID request-ID and boolean retryability types remain the intended native contract. Free-form branch/path diagnostics are intentionally omitted; the machine error code and partial-operation boolean flags provide machine-consumed recovery information while the human-readable message is local.

Git status initially failed on the existing vendor submodule symlink. A scoped status with submodules ignored succeeded and showed the three Rust files as untracked in this worktree; accordingly, ordinary git diff does not represent their content changes. No git state was modified.
