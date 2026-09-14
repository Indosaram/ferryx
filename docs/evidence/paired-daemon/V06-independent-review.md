# V06 independent review

Reviewer: st_01a093d4. Snapshot before test cleanup refinement.

**Approve the scoped V06 fix.** No blocking correctness or authorization findings in the reviewed change. This is not approval of A04-A24 or Windows/native behavior.

### Findings, ordered by severity

1. **Low - setup failures lack guaranteed asynchronous PTY cleanup.**
   `resize_security_tests.rs:32-70` spawns the PTY and starts the fixture before entering `catch_unwind`. A panic during pairing/server startup can bypass explicit close/reap. `PtySession::drop` closes handles and aborts its reader but does not explicitly kill/reap the child. This limitation is accurately disclosed in the evidence request; it does not invalidate the observed assertion-path RED. The test-body cleanup catches assertion/timeout panics, stops the listener, awaits PTY close and output-pump completion, removes the hub, then resumes the original panic.

2. **Low - Close/EOF is a fixture-specific barrier, not a general processing acknowledgment.**
   `tests.rs:1665-1674` maps header EOF to `Close`; therefore `resize_security_tests.rs:7-13` cannot independently distinguish ordered receive completion from unrelated connection termination. **I found no active competing termination cause in this fixture:** selection remains fixed, neither device is revoked, local recovery stays pending, the hub retains its broadcast sender, and the client stays connected through the barrier. Both receive loops process Resize inline before reading Close; production `TerminalService` resize is synchronous before its returned future completes. Grid Control additionally waits for the exact resized full frame. These facts and the actual RED make the barrier adequate for this scoped regression, rather than a timing-based absence assertion. Unexpected transport/mirror termination remains outside what EOF alone proves.

### Authorization and regression assessment

- `server.rs:1334-1336` and `1707-1709` reject non-Control Resize before any backend mutation; the grid guard also precedes mirror resize/enqueue. Existing Control geometry validation, initial Control-only viewport policy, SSH generation admission, Scroll, and selection policy remain unchanged.
- Tests exercise actual authenticated HTTP/WebSocket connections and production `TerminalService`/PTY, not a mocked resize boundary. Saved View geometry is collected after the barrier and before Control changes it.
- Control backend and child `stty size` assertions execute **before** saved View denial assertions. Blanket resize denial therefore cannot pass.
- Child output subscription precedes its trigger; bounds detect missing events rather than establish absence. No test sleeps or polling were added.
- The grid test is not native-feature-gated, but its ancestor is **`#[cfg(all(test, unix))]` at `remote/mod.rs:21`**. These tests do not execute on Windows.
- Scope is exactly six production lines, three registration lines, and the new test module. The new module is currently untracked; unrelated worktree changes were not reviewed.

### Evidence boundaries

**Directly inspected:** the scoped diff, complete new test module, registration, surrounding authorization/receive/cancellation paths, socket helpers, production resize/cleanup dependencies, and both requested evidence files. Current test-module and registration SHA-256 hashes match the request manifest.

**Directly observed in `V06-parent-red.log`:** both named tests failed at the saved View backend assertion with actual `(51, 17)` versus expected `(80, 24)`; **0 passed, 2 failed, exit 101**. Their assertion location establishes that preceding Control backend/child checks completed. The later View child assertion was not reached on RED.

**Parent-reported, not independently executed or log-inspected here:** the identical focused command subsequently passed **2 tests, 0 failures, 0.85s, exit 0** (`mon_G74NZ6CXVXDJP5XG`, `bash187`).

I ran no Cargo commands, launched no desktop/daemon, and wrote no files. Full headless remote regression remains pending in the supplied status; default-feature/native, Windows, and broader plan approval remain unverified.
