# A16: partial native routing and owned lifecycle

Status: PARTIAL. A16 acceptance and AC08 are not complete. `pairedDaemonProxyV1` remains false; neither deliberate false contract assertion was changed.

## Implemented

- Added `terminal/paired_runtime.rs`: bounded command mailbox and one socket-owning task per proxy, concurrent receive/control selection, five-second keepalive scheduling, generation-fenced write/resize/interrupt, explicit detach acknowledgement after proxy/socket/hub destruction, and task join on successful detach. Registry drop aborts owned tasks. Finished owners are excluded from live inventory and can be replaced on explicit reattach.
- TerminalService owns the paired registry, includes live proxy IDs in native inventory, and uses the existing output-hub attachment APIs. Generation operations route paired IDs to the actor. Unfenced write/resize/signal reject paired IDs rather than entering PTY operations.
- SessionRouter recognizes the paired namespace as locally routed, including missing IDs, and never forwards those IDs to legacy peers. Missing proxy attachment fails in TerminalService.
- Added daemon protocol/client/server reattach and detach operations. Reattach calls the A15 `MachineClient::attach_terminal` path through `Proxy::reattach`, preserving execute-based machine identity/capability/generation validation. It creates no remote session. Existing generation-bearing RemoteWrite/RemoteResize IPC requests route paired IDs to the actor. Ordinary Write/Resize reject them explicitly.
- Native Attach continues through the existing daemon output stream and local epoch/sequence framing; remote metadata is decoded by the unchanged A15 core before it reaches the hub.
- Generic paired Close is deliberately rejected with instructions to use the existing request-ID-bearing paired CloseSession operation. Detach does not pretend to close the remote PTY. No new remote mutation variant or mutation retry logic was introduced.

## Evidence

`A16-RED.log` records the failing-first missing `TerminalService::paired` compile errors, and a real deliberately broken assertion in the actor socket test (decoded `hello` versus `intentionally-broken`, exit 101). The broken assertion was reached after socket/server/temp-root cleanup, then restored.

`A16-GREEN.log` records:

- Required paired_host suite: final run 30 passed, 0 failed, exit 0 (A15 had 28).
- Required no-default-features library cargo check: exit 0.
- Required ferryx-cli/ferryx-relay cargo check: exit 0.
- Existing warnings preserved, not suppressed.
- The initial full suite had 29 passed / 1 failed: `real_machine_catalog_replay_checks_digest_and_preserves_metadata` failed `project.metadata.git_root.is_none()`. Its temp directory was inside the isolated Git worktree, so Git discovered the ancestor repository. Setting `GIT_CEILING_DIRECTORIES=$TMPDIR` corrected test environment isolation; no unrelated test was edited. Both results remain in the log.
- LSP checks on changed files returned no diagnostics in the completed directory/file wave. Some earlier concurrent requests were cancelled; directory waves also reported cancellations for unrelated files.
- A16 runtime/temp/HOME root removal and absence assertion recorded.

The real loopback HTTP/WebSocket fixture now runs both A15 direct transport and A16 native registry actor modes. Actor mode proves authorized reattach, native hub subscription, router ownership, unfenced and stale-controller rejection, delivered binary input/resize/interrupt, concurrent replay/live output, separate native sequence allocation, detach, rejection after detach, empty registry/PTy inventory, listener refusal, joined fixture server and removed temporary directory. Subscription and exact oneshot signals precede actions; no sleeps/polling are used. The second new test proves two hosts with the same raw session ID remain distinct and a transport-detached registered proxy cannot write. No fixture PTY is spawned.

## Not implemented or not proved

- No Tauri command registration or frontend spawn/reattach wiring; native renderer end-to-end, detached *surface* ownership (distinct from remote controller fencing), IME/paste/menu/search/selection and inactive-workspace lifecycle are unproved.
- New daemon reattach/detach and generation routing arms compile but were not exercised over real UDS in this packet. The proven real surface is TerminalService/SessionRouter plus HTTP/WebSocket, not desktop IPC end to end.
- No CWD/project/session metadata projection, remote lifecycle-to-frontend mapping, event-stream reconciliation or explicit remote exit-code mapping. Transport termination removes the hub but is not a complete lifecycle model.
- Keepalive scheduling is implemented but elapsed-time keepalive delivery is not asserted. Automatic recovery, controller replacement, credential revocation mid-IO, slow consumers and repeated reconnect remain unproved. This actor uses the A15 single-socket core; cancellation during receive-side Pong flushing deserves dedicated coverage before enablement.
- No durable descriptors/restoration policy, remote close integration, create-and-attach orchestration or real relay/remote PTY continuity proof. Remote close stays on existing MachineClient mutation handling; native generic Close does not silently detach.
- Registry Drop aborts tasks; explicit detach is the tested joined cleanup path. Finished task handles remain as bounded-by-created-session tombstones until detach/replacement/runtime drop; automated reclamation remains outstanding.

## Isolation and assumptions

All source/evidence edits are in the assigned worktree. The binding plan was read at the explicitly supplied canonical path only. No commits, destructive Git operations, release builds, deployment, desktop automation, canonical daemon interaction or running PTY interaction occurred. No UI or handover file was edited.

Execution caveat: the first RED compile was launched before setting an isolated HOME/CARGO_HOME and timed out during dependency compilation; it produced no test execution result. Subsequent commands used the A16 isolated HOME/TMP and the already-populated worktree-local A24 Cargo cache (not deleted because it is not A16-owned), with installed Rust toolchains referenced read-only. This initial command did not satisfy the requested real-HOME isolation; no claim is made that Cargo performed no cache access. All A16-created temporary roots were removed.

Assumed the existing generation-bearing native operations are the appropriate low-level input route, without treating SSH-helper wire semantics as relay semantics. Delivered the permitted partial routing/lifecycle foundation rather than enabling an unproved capability.
