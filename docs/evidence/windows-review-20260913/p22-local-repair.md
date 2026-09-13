# P22 local transport repair - 2026-09-13

Task st_01a098ff. Only `script/qa/win-daemon-e2e.mjs` and this report were edited. Local Darwin arm64/Bun evidence, not native Windows acceptance.

## Contract and repair

Read the parent loop's original `.omo/ulw-loop/01a0983f-c995-753d-afa9-593f6d118788/brief.md`, P22 in repair-packets.md, the entire original validator, `src-tauri/src/daemon/protocol.rs`, and server attach/response dispatch. Rust currently declares `DAEMON_PROTOCOL_VERSION: u32 = 3`. Responses are newline JSON with no response IDs. Attach sends attachOk/history then changes the connection into a stream; stream types are output, gap, replayGap, agentState, remoteStatus and exit. Desktop remote events have a separate event/payload envelope.

The helper now separates those events from FIFO responses, isolates and reports subscriber exceptions, and poisons the entire connection on a response deadline instead of allowing ambiguous late replies to satisfy later requests. One idempotent terminal path handles socket error, EOF, close and explicit close; it rejects pending requests, clears deadlines and notifies the stream wrapper, which rejects and removes every PTY waiter. Opening handshake/attach failure closes the acquired client before propagating rejection. Both actual handshake call sites use protocol 3. The control handshake is factored into the same function exercised by self-test; normal main still owns its control client in its existing finally block.

## Failing-first procedure

Before any behavioral fix, added all 13 transport scenarios to the existing self-test. Only preparatory helper changes were default-preserving clock/socket injection and extraction of the unchanged version-2 control request. Both injections still create actual loopback TCP sockets; no request method or JSON parser is mocked. Then executed exactly:

```text
bun script/qa/win-daemon-e2e.mjs --self-test
```

Exit 1, 12 intended assertion failures and one existing passing control. No barrier timeout or missing prerequisite was used as RED. The following are the actual failing output values (line breaks condensed):

| Scenario | Pre-fix receipt | Identical post-fix assertion |
| --- | --- | --- |
| event-response-fifo | A resolves only its response despite output/listener throw: actual 'output', expected 'writeOk' | GREEN, cases=1 receipts=2 timers=0 |
| listener-isolation | Same A assertion: actual undefined, expected 'writeOk' | GREEN, cases=1 receipts=2 timers=0 |
| timeout-poisons-late-reply | timeout settles queued request before late reply: 0 !== 1 | GREEN, cases=1 receipts=2 timers=0 |
| requests-close | all requests settle once on terminal event: actual [0,0,0,0], expected [1,1,0,0] | GREEN, cases=1 receipts=2 timers=0 |
| pty-waiters-close | all PTY waiters settle once on terminal event: actual [0,0,0,0], expected [1,1,0,0] | GREEN, cases=1 receipts=2 timers=0 |
| requests-eof | all requests settle once on terminal event: actual [0,0,0,0], expected [1,1,0,0] | GREEN, cases=1 receipts=2 timers=0 |
| pty-waiters-eof | all PTY waiters settle once on terminal event: actual [0,0,0,0], expected [1,1,0,0] | GREEN, cases=1 receipts=2 timers=0 |
| requests-error | Already GREEN before repair; cases=1 receipts=2 timers=0 | GREEN, cases=1 receipts=2 timers=0 |
| pty-waiters-error | all PTY waiters settle once on terminal event: actual [0,0,0,0], expected [1,1,0,0] | GREEN, cases=1 receipts=2 timers=0 |
| failed-handshake-cleanup | failed opening closes acquired socket before rejection: false !== true | GREEN, cases=1 receipts=1 timers=0 |
| failed-attach-cleanup | failed opening closes acquired socket before rejection: false !== true | GREEN, cases=1 receipts=2 timers=0 |
| protocol-control | actual handshake matches Rust protocol constant: 2 !== 3 | GREEN, cases=1 receipts=1 timers=0 |
| protocol-stream | actual handshake matches Rust protocol constant: 2 !== 3 | GREEN, cases=1 receipts=2 timers=0 |

RED ended with `E2E FAIL: transport scenarios failed: event-response-fifo, listener-isolation, timeout-poisons-late-reply, requests-close, pty-waiters-close, requests-eof, pty-waiters-eof, pty-waiters-error, failed-handshake-cleanup, failed-attach-cleanup, protocol-control, protocol-stream` and `12 !== 0`.

Every RED fixture printed `sockets=0 listenerClosed=true`. Residual virtual deadlines before fixture disposal: timeout=2, requests-close=2, requests-eof=2, pty-eof=2, pty-error=2, protocol-control=1, protocol-stream=1; all other cases=0. These leaks were reported, then the owned injected scheduler was disposed, not hidden as passing cleanup. The explicit-close PTY defect had zero timers but two unsettled promises.

## GREEN and cleanup receipts

After repair, the exact registered command exited 0 with all 13 scenarios passing on its first execution. After removing the parser-only success banner, exposing expected fixture socket errors, and replacing forced process.exit with process.exitCode, the same command again exited 0 naturally. Assertions were unchanged between RED and GREEN.

Final output included all GREEN rows above, and for each of the 13 names:

```text
P22 CLEAN <scenario>: sockets=0 listenerClosed=true residualTimers=0 disposedTimers=0
SELF-TEST PASS: transport scenarios=13; pure parser retained; owned resources closed.
```

There were 24 parsed peer request receipts in total. Listener-isolation reports `Daemon QA listener failed: owned-listener-sentinel` three times while both responses and the later listener succeed. Socket error controls report `owned-socket-error` and `owned-stream-error`; the deliberately late write after timeout reported `read ECONNRESET` on the final run. These expected errors are visible, not silently suppressed or treated as standalone success.

Tests subscribe to exact incoming request and endpoint close signals before triggering replies/termination. Multiple outstanding promises carry resolve/reject counters. The timeout fires an injected scheduler callback only after the peer has parsed both queued requests, writes late A through the real peer, and asserts queued/new requests reject without misassignment. PTY lifetime cases establish a real handshake/attach before allocating two waiters, then verify two rejections, no resolutions, post-close rejection and zero timers. Strict protocol peers compare actual outgoing values against the constant parsed from the Rust source via an import-relative URL and reject stale versions. The final count must equal 13, so parser-only/zero-case success is impossible.

Owned peers bind only `127.0.0.1:0`; no port file is read on this branch. Each scenario destroys only its tracked client/accepted sockets in finally, awaits both close events, closes its listener, and checks zero owned sockets/deadlines. One-second real deadlines only fail missing event barriers and are cleared in finally; there are no sleeps, polling or timing-based success checks. The process now exits naturally so forced success cannot conceal open handles.

Diagnostics on the behavioral repair returned `No diagnostics found`. The final freshness request after the reporting/process-exit edits timed out at 3000ms; Markdown has no configured LSP server. Final script execution still passed. Scoped `git diff --check` passed, and the actual script diff was inspected. No build is required for this interpreted single-domain script; the registered command executes the affected real entry point and transport surface.

## Limits

## Lead verification and remaining cleanup repair

Lead read the complete helper, this report, the actual diff and Rust response/
stream enums. Independently ran the exact registered self-test:
mon_GH5DRMVVFYEQSGVK / bash_20, exit 0, 13 scenarios and 24 real peer receipts.
Every fixture reported zero sockets and residual deadlines. Full output read.

Registered independent original-boundary mutation:
`bun script/qa/p22-mutation.mjs --self-test`, mon_RBGNK9AV9GRZHF0Y / bash_21.
Exact-match in-memory edits retained all final assertions. Exit 1 with
12 intended failures and requests-error passing; all 13 fixture listeners/
sockets closed. Leaked virtual deadlines were reported and disposed, not
counted as clean success. Full output read. The runner was deleted after exit,
its absence checked and helper bytes stayed unchanged across that mutation.

Lead found the normal main finally block still swallowed session-close failure.
Registered three more real TCP scenarios before changing that behavior.
Extracted the unchanged cleanup into closeOwnedSession, used by main and tests.
The registered self-test then failed at session-cleanup-error and
session-cleanup-eof (rejected count 0 instead of 1), with the other 14 passing:
mon_3ZFWCWQE9GNGSXCR / bash_22, exit 1. All sockets and timers cleaned.

Minimal repair now requires closeOk, propagates failure, and always closes
both control/stream clients in finally. Identical assertions and command:
mon_RCWWWPF41NYS5D9V / bash_23, exit 0, 16 scenarios, 33 peer request receipts,
16 cleanup rows with zero residual sockets/deadlines. Complete output read.
Original parser and all 13 transport cases remain intact.

Final script SHA256:
`19f0a0d4910adfea38aca3374c444baee8c265ed1a7f972e1e30866aa74cc84e`.
Initial lead LSP was clean; final freshness checks timed out at 3000ms and
are not clean diagnostics. The complete interpreted entry point executed
successfully. git diff --check exited 0. Same-scope binary diff excluding
this helper and the active P31 owner's Sidebar/WorktreeList was identical
before/after (43,362 bytes); no foreign changes were reverted.

The earlier limits paragraph is historical regarding lead inspection/rerun;
native Windows, real session cleanup/PID proof and aggregate acceptance
remain unverified. A closeOk acknowledgement alone is not a PTY exit receipt.

No daemon executable, user daemon, default port-file invocation, desktop, SSH, ConPTY, install, release, branch/worktree, commit or push was invoked. Other owners' changes were untouched. This is local transport-validator evidence only. Native Windows ConPTY input/output, actual shell cwd, sequence advancement, owned readiness/PID identity and sibling/user PID survival remain pending, as do the lead's independent artifact inspection and rerun. The source contract assumes current uncorrelated FIFO replies; adding protocol IDs/authentication requires coordinated adaptation rather than silently negotiating with a stale installed daemon.
