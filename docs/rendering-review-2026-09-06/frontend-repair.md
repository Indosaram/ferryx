# D2 frontend ownership repair - 2026-09-06

## Outcome and scope

D2 is repaired and reproduced RED/GREEN at the real React component/lifecycle
boundary. All 167 focused UI tests and the UI build pass. Three isolated browser
scenarios also pass with controlled IPC. **Native compositor pixels, desktop
z-order, and the reported macOS incident remain unverified.**

Commit: `bce59b45d6d2d43f46cc63e35a5fb9b1161ca145`
(`fix(terminal): invalidate obsolete input recovery owners`).
Worktree: `/Users/indo/code/project/orca-lite-rendering-20260906`.
Branch: `fix/rendering-review-20260906`; no upstream configured, no push/merge.
Base: `b8f82d707f0cb99907e3d79c0c9cdc75053ef931`.

Only these three application files are in the commit:

- `ui/src/components/NativeTerminalPane.tsx`
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`
- `ui/src/lib/nativeTerminalLifecycle.ts`

No App/onboarding, Rust, vendor, dependency or main-tree source edits. A foreign
`src-tauri/tests/native_terminal_wayland_subsurface_contract.rs` edit appeared
during execution; it remains uncommitted and untouched by this task. The named
report and QA artifacts are deliberately outside the UI commit, in the main repo.

## Mechanism and minimal repair

The traced production caller is active-tab `TerminalSplitView` -> `TerminalPane`
-> `NativeTerminalPane.sendInput` -> `performAttach` -> real native lifecycle.
The rejected input previously force-attached captured A after its owner departed,
cancelling the A detachment held under B's presentation.

The pane now creates a committed owner identity in a layout effect. Target change,
visibility change and unmount invalidate it before passive teardown/queued IPC.
Object identity, not just session ID, prevents A -> B -> A or hide/show from
reviving obsolete input. Checks protect:

1. Input execution and recovery ownership mutation.
2. A queued attach callback when it actually executes.
3. Post-recovery focus restoration/retry.
4. Input/retry receipt publication and recovery/retry error publication.

The lifecycle retains its shared readiness promise and legitimate detachment
cancellation. If an attachment is still queued, a real returning owner replaces
that queued callback with its own lifetime and geometry. Already-started IPC
remains shared and serialized; it is not cancelled or treated as unsent. This
small lifecycle addition prevents the new queue guard from starving a returning
owner. No daemon PTY close or native detach policy was changed.

## Deterministic RED/GREEN

Evidence directory, abbreviated `D2/` below:
`/Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/D2/`.

Each required command ran from the repair worktree using
`bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t '<name>'`.
Each RED executed exactly one failing test, exit 1; each matching GREEN executed
exactly one passing test, exit 0. Filtered-out tests are not the RED evidence.

| Exact test name | Right-reason RED | Logs |
|---|---|---|
| does not reclaim the outgoing surface when input fails after tab replacement | A attached twice, expected once | `red-replacement.log`, `green-replacement.log` |
| does not recover input when its owner becomes hidden | A reattached after actual detach | `red-hidden.log`, `green-hidden.log` |
| does not retry input after its owner leaves during recovery | Two input invocations, expected one | `red-recovery.log`, `green-recovery.log` |

The replacement case uses different A/B IDs and rectangles, holds B's bounds
receipt, rejects A input, then releases B presentation and observes A detaching
exactly once while B remains attached. The hidden case uses the real visibility
provider and awaits A's detach before rejecting input. The recovery case departs
while recovery IPC is pending and rejects obsolete retry/IME publication.

Seven additional executed cases cover queued recovery with unmount and A -> B -> A
return, first-input/retry stale receipts, recovery/retry stale errors, and one
shared recovery for two concurrent inputs from a live owner. `red-additional.log`
records six right-reason failures and the positive shared-recovery pass before
production edits. All are GREEN in the combined run. Existing StrictMode,
same-session reparenting, held-detach cancellation, readiness and retry-limit
contracts also remain GREEN.

Tests use the real component/lifecycle, mock IPC/layout only, reset both singleton
modules, and settle subscribed deferred IPC using React `act`. No new sleep,
polling or `waitFor` assertions were added; test-runner timeouts bound failures.

## Verification receipts

- `focused-ui.log`: one execution of
  `bun run --cwd ui test -- src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/lib/nativeTerminalLifecycle.test.ts`;
  **167 passed across three files, exit 0**. REPAIR_PHASE names this combined gate;
  its exact command is supplied by `baseline.md`.
- Expected stderr from the existing injected mount-attach retry test remains
  visible in that log; no errors/warnings/tests were suppressed.
- `diagnostics.md`: LSP severity=all on every changed file, all three clean.
- `build.log`: `bun run --cwd ui build` (**tsc + Vite**, exit 0).
- `provenance-before-commit.log`, `provenance-after-commit.log`, `commit.log`:
  source hashes, base/commit tree IDs, working status, history and scoped commit.
- `ui-increment.patch` equals the staged patch byte-for-byte; SHA-256:
  `cc043653a09cfb65039b07362583743dc096e822af279414f4b58618751f7bea`.
  Scoped main source comparison against the base was empty. Diff checks passed.

## Real browser entry proof and cleanup

Run: `bun .omo/evidence/ulw/rendering-review-20260906/D2/browser-qa.mjs`
from the main repository. This QA-only bundle imports the actual worktree pane,
lifecycle, visibility provider and Tauri JS wrappers. Bun.WebView uses isolated
system WebKit; the boundary is `__TAURI_INTERNALS__`, not a mocked lifecycle.
The ephemeral loopback server serves the real built UI CSS. No dependencies added.

The script registers IPC signals before actions, clicks the real viewport, presses
`x` through the real input handlers, and drives harness replacement/visibility/
IPC-settlement controls with awaited React `act`. A/B rectangles differ.
`browser-run.log` records all three scenarios passing, exit 0:

| Scenario | Capture and IPC log | Observed contract |
|---|---|---|
| Replacement, then reject A input | `browser-replacement.png/.json` | A attach=1, input=1, detach=1; B detach=0 |
| Hide, await A detach, then reject | `browser-hidden.png/.json` | No recovery attach/retry after hiding |
| Recover A, replace with B, finish recovery | `browser-recovery.png/.json` | A attach=2 (recovery began while live), input=1, detach=1; B detach=0 |

No stale alert appeared in any scenario. All screenshots are valid 1280x900 RGBA
PNG files (file/sips checked). The evidence page explicitly labels its empty
terminal region as **not native terminal pixels**. Image viewing is unsupported
by this child model and no delegation tool was exposed; pixel-fidelity and
independent visual review are not claimed. The phase verifier has the captures.

Browser setup failures are retained in `browser-attempt1..8.log` with cleanup
receipts, rather than presented as product failures or silently discarded. They
involved the QA bundler environment, hidden-sink actionability/InsertText focus,
and unawaited harness `act`, all repaired only in QA artifacts. The final complete
run is the browser receipt, not any partial attempt.

`browser-cleanup.json` confirms all three components unmounted with zero IPC
callbacks left, three WebViews closed and server stopped. The WebKit host was
also closed via `Bun.WebView.closeAll`; final port 49872 had no listener. Earlier
attempts closed their owned view/server in finally and their CLI processes exited.
No desktop app, daemon, existing browser profile or user session was controlled.

## Exact remaining native QA (not completed)

Native desktop execution is unavailable by the task's established boundary:
Orca runtime absent and osascript Accessibility denied. Do not infer a native
pixel fix from these JS/IPC receipts. An authorized native QA owner still needs:

1. On a real macOS desktop build containing this commit, present distinct A/B
   content at distinct bounds. Hold A's input response, switch to B with first
   presentation held, reject A input, then release B. Capture IPC/host teardown
   and desktop screenshots showing A absent after B presents, without stale
   overlay/z-order pixels or a stranded host.
2. Cover A with real Settings/search/provider suppression, await native detach,
   reject pending input and capture the overlay with no recovered A surface or
   retry. Confirm hiding does not close the daemon PTY.
3. Start legitimate A recovery, leave while native attach is pending, settle it
   after B presents, and verify no A input retry, focus theft, stale IME anchor or
   stale banner. Include queued departure and A -> B -> A returning-owner order.
4. Positive native checks: same-owner shared recovery retries each input once;
   same-session reparent/return remains attached and presents at current bounds.

Other rendering criteria/platform repairs are outside D2. Aggregate native
rendering acceptance and independent phase verification remain with the parent.
