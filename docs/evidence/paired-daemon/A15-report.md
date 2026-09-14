# A15: partial transport and lifecycle core

Status: PARTIAL. This does not unblock AC08. `pairedDaemonProxyV1` remains false.

## Changes

- Added `src-tauri/src/terminal/paired_daemon.rs` and exported the module.
  - Credential-free serializable host/generation/remote target/cursor descriptor.
  - Host-qualified backend IDs use the existing `proxy_backend_id` contract.
  - Explicit reattach performs no CreateSession request. No shell, SSH process, PTY, or background task is created by this module.
  - Bounded WebSocket frames, bounded IO deadlines, credential-lease cancellation, attached-target validation, controller-generation admission.
  - Input, resize, interrupt, ping, detach, binary output decoding, and JSON lifecycle return to the caller.
  - Existing terminal wire parser strips remote metadata; only terminal bytes enter the bounded native output hub. Remote cursors remain separate from native sequence allocation. Explicit gaps clear hub replay via publish_gap.
  - Proxy drop removes its hub entry and drops its owned socket.
- Added `MachineClient::attach_terminal`. It calls `MachineClient::execute(Operation::Session)` for existing host identity, permission, capability, target and generation checks, then captures the same generation's credential lease and opens the host-qualified socket. Authorization is a header, never a URL parameter or terminal payload. Transport errors are mapped to fixed codes, not logged with credential-bearing requests.
- Added `paired_host/proxy_tests.rs` and its test module registration.
- No changes to daemon IPC mutation variants, 35-second timeout, 32 KiB request cap, capability advertisement, UI, handover, deployment or release configuration.

## Evidence

`A15-RED.log` contains the pre-implementation compile failure on the missing paired_daemon module. It also contains the required real assertion failure after implementation: decoded bytes were `hello`, while a deliberately broken assertion expected `intentionally-broken`. The latter assertion ran after explicit fixture cleanup. The correct assertion was restored.

An intermediate development run failed because the new fixture used `state` rather than the actual SessionDetail discriminator `status`; `A15-development.log` retains that failure. Corrected the fixture to the inspected wire contract.

`A15-GREEN.log` records:

- `RUST_TEST_NOCAPTURE=1 cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib paired_host:: -- --test-threads=1`: 28 passed, 0 failed, exit 0.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: exit 0.
- `cargo check --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay`: exit 0.
- Existing compiler warnings remain visible in the logs; none were suppressed.
- LSP diagnostics on all five changed Rust files reported no diagnostics.

The new real-socket fixture verifies header authentication and exact non-secret socket query; machine-session validation before attachment; controller 6 rejection while controller 7 is attached; binary input and parsed resize/interrupt controls; framed replay decoding into the existing hub; independently allocated local output sequence (including a resize marker); remote cursor advancement through live output; explicit detach; hub removal on drop; graceful fixture server completion; listener refusal after teardown; temporary root removal. Output subscription and input oneshot channel are established before triggering actions. There are no sleeps or polling loops. The fixture creates no PTY.

## Not implemented or not proved

- No proxy registry/actor is installed in TerminalService, SessionRouter, daemon request handling, or native terminal IPC. The core is callable Rust code, not a native client operation exposed over IPC. Missing-proxy non-fallthrough routing still needs implementation.
- Create/close remain existing MachineClient operations; there is no combined proxy Create API or remote close integration. No new mutation operation bypasses the existing reconciliation logic.
- The owner must drive receive and ping; there is no concurrent socket actor, scheduled keepalive, automatic reconnect, or event-stream reconciliation. Holding a mutable Proxy serializes operations and prevents stale callback adoption, but does not provide the final concurrent native runtime.
- Descriptor serialization exists, but persistence/restart recovery does not. Restoring a remote cursor into a fresh empty local hub requires a full replay policy before this can be advertised.
- Reattach and cursor query implementation exists, but repeated connection recovery, controller replacement, credential revocation mid-IO, remote/local epoch changes, gap recovery, duplicate/out-of-order frames, slow consumers and bounded replay behavior are not covered by this new test.
- No real relay server/data-channel path or remote daemon PTY was exercised by the new fixture. No native renderer surface was exercised. Process identity continuity, no duplicate remote shell, remote close, and full AC08 remain unproved.
- Full A15 acceptance and A16 routing are still required. Do not turn on pairedDaemonProxyV1 based on this evidence.

## Isolation and assumptions

All edits and commands used the isolated worktree. The binding plan was read from the explicitly supplied canonical plan path because it is absent in the worktree; no canonical file was written. No commits or destructive git commands, deployment, release builds, desktop automation, or user PTY interaction occurred. `git status` could not run because this worktree's vendored ghostty submodule path is a symbolic link; no attempt was made to modify that setup.

Assumed the existing gateway machine terminal wire and `terminalCreateV1` session capability are authoritative. Kept SSH semantics entirely separate. Delivered the explicitly permitted partial transport core rather than advertising an unimplemented native runtime.
