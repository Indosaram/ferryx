# D5 direct dropped-frame retry repair

## Delivered increment

Commit: `829fbe8e01e06418f329fa7022141b2fbab3e6e9`
(`fix(native-terminal): rearm dropped direct frames after guard release (D5)`).
Branch: `fix/rendering-retry-20260906`.
Worktree: `/Users/indo/code/project/orca-lite-render-retry-20260906`.
Parent: verified atlas `17fb10608ad31e0e60ae565e8dc686afdc4735f9`.
Only committed file: `src-tauri/src/native_terminal/surface_host.rs`, including
its internal tests. No main merge or push; active geometry/receipt worktrees
were not modified. These reports live in the parent checkout, outside that commit.

**Host execution/completion is repaired and verified. Native screenshot/pixel
acceptance remains pending.** The injected frame target is not a native GPU pass.

## Cause and behavior change

The macOS scroll callback in `src/lib.rs` updates VT scroll state through
`scroll_attached_native_terminal`, then invokes public `state.render` once.
Portable scroll IPC also follows this path; focus invokes `set_focus` directly.
Both direct host methods formerly returned a dropped receipt without reaching
scheduled completion's retry handling. The scheduled path already rearmed
Timeout receipts and deferred its follow-up dispatch off-thread.

Both direct methods now retain the original receipt, release the host guard,
then rearm a still-attached session's existing coordinator only for an actual
non-presented host frame. Session snapshot guards have already ended. The helper
also releases its short session lookup guard before `schedule_render`/dispatch.
Only the idle-to-scheduled winner submits work; existing pending work coalesces.
The same off-thread `defer_scheduled_render` boundary serves direct and scheduled
completion, so Wry's inline main-thread dispatch cannot recursively retry.

The returned receipt is unchanged (`presented: false` still means the original
attempt dropped). `?` still returns fatal errors without invoking rearm. A focus
receipt without an existing host still takes the original non-rendering branch.
Deferred work uses the original scheduled execution's attached-session lookup,
host lookup, snapshot capture, begin/finish transitions, and host ownership lock;
it does not create hosts. Detach consumes pending work and removes the host;
close removes the session/host, so an already-queued task cannot reveal it.

## Neutral seam and faithful RED chronology

1. Before extraction or behavior edits, the unmodified host test filter passed
   **27 tests**, including ownership guard, detach, coordinator, receipt, retained
   session and layout characterizations. See `pre-extraction-characterization.log`.
2. Behavior-neutral extraction split native frame resources from the host's
   layout/ownership container. Native field destruction remains
   `surface -> target -> renderer`. The private test-only enum alternative
   substitutes the native target; production has only the native alternative.
   `acquire_surface_frame` preserves the original acquire, one Lost/Outdated
   reconfigure, reacquire, Timeout-drop/fatal-error sequence. Native rendering
   calls this helper with real WGPU acquisition/configuration closures.
   Main-thread task submission gained a test-only queue boundary; production
   still calls `window.run_on_main_thread`.
3. With that extraction and **without rearm**, direct success, real scroll/focus,
   receipt, and detached-entry characterization passed one exact test. A first
   compilation attempt caught a missing `'static` lifetime on the fixture's
   daemon message sender; it was corrected before behavioral RED. Both logs are
   retained, not classified as behavioral failures.
4. The mandated exact `one_shot_render_requeues_when_frame_is_dropped` command
   executed **one test, one failure, exit 101** at the assertion:
   `dropped direct frame must rearm the attached session coordinator`.
   Before that assertion, real public scroll/render had run on an attached
   daemon-backed session and existing injected host, acquiring Timeout and
   returning the original dropped receipt. `neutral-seam-and-red-test.patch`
   records the complete pre-rearm source delta. No helper was manually invoked
   to supply the missing retry.
5. Only after that RED, direct rearm was added. The existing scheduled off-thread
   spawn was factored into the shared deferred helper without changing its
   scheduling semantics. The exact regression passed, as did the final gates.

The seam does not emulate a second coordinator or replace public direct execution.
It substitutes the unavailable native frame target and main-thread queue only:
configuration/viewport side effects and presentation are simulated, not measured.
The real attached session, VT, host map, lock lifetime, receipt completion, and
scheduled execution remain in the test. This is host proof, not drawable/pixel proof.

## Deterministic contracts

Eight added tests cover direct success/focus characterization; one Timeout to
success; Lost/reconfigure to Timeout to success; another scheduled Timeout to
success; dropped focus receipt; actual detach and close before executing queued
retry work; fatal OOM from direct render/focus and from a scheduled retry; and
coalescing with a real preedit request already pending.

The fixture attaches real daemon history without launching a daemon, installs an
existing host with injected frame primitives, and subscribes to the task channel
before the trigger. The final fixture also asserts that scroll changes VT offset.
At frame execution it asserts host ownership is locked and the snapshot session
guard is released. At queued execution it checks both guards are available.
Retry submission must run on a different thread from the direct caller. Deferred
submission JoinHandles are awaited before checking for extra queued work, so
absence/coalescing assertions cannot pass because a worker happened to be slow.
Tests execute only tasks submitted by production completion; they never schedule
the missing retry. Deadlines fail stuck exact events/tasks; there are no added
sleeps or polling loops. Legacy unrelated yield-based tests were not rewritten.

## Verification and limits

See [direct-retry-verification.md](direct-retry-verification.md) for commands and
the raw evidence index. Observed results: exact final regression **1 passed**,
host filter **35 passed**, existing host contract **18 passed**, broader
native-terminal library filter **127 passed**, normal-config Cargo check/build
**exit 0**, and changed-file LSP **No diagnostics found**. Existing compiler
warnings outside this file remain visible and were not suppressed.

The standalone host-test executable produced the same entry trace with the real
scroll/render/coordinator path and injected frame target. Its first invocation
failed before entry because direct execution lacked Cargo's Ghostty dylib search
path. Supplying the observed build artifact directory via `DYLD_LIBRARY_PATH`
made that same binary execute one passing test; both logs and the owned loader
crash report are retained. This setup failure is neither D5 RED nor a native app
fault-injection result.

No desktop app, installed app, daemon, or user-input automation was launched.
The native one-scroll fault-injection screenshot remains **pending**: this run
does not establish real Metal Timeout frequency, native reveal behavior, or final
pixels. Host integration with the active receipt/geometry branch remains a
separate parent-owned merge and combined verification, not performed here.

## Cleanup

All owned Cargo/Bun/test commands exited. Fixture teardown aborts its owned
stream/pump tasks and removes hosts/sessions; deferred submission tasks are
joined in the regressions. The standalone loader crash report was relocated
from `~/Library/Logs/DiagnosticReports` into D5 evidence; no `/cores/core.14088`
artifact existed. No persistent child process remained in the cleanup inspection.
No cache clean, foreign process kill, stash/reset, main merge, or push was used.

The isolated worktree is clean after the commit. Ignored frozen-install
`node_modules`, `ui/node_modules`, real `ui/dist`, and generated ignored Cargo
lockfile are retained as ordinary reproducible build prerequisites, not committed
dependency changes. Shared Cargo caches and the prepared pinned Ghostty clone
were reused non-destructively and retained. See `owned-cleanup.log`.
