# Native pane frontend rendering audit - 2026-09-06

## Verdict and evidence boundary

One HIGH source-proven lifecycle defect remains: a delayed input failure can
force an obsolete pane owner to attach again, cancelling its legitimate teardown.
Its native pixel manifestation has **not** been reproduced in this review.
No additional concrete caller-backed geometry or density defect was established.

This is a read-only diagnosis, not a fix or runtime approval. No tests, builds,
app launches, desktop input, or browser automation were run. Proposed RED tests
below are **not implemented and not run**. The September 5 audit's passing tests
and headless WebKit evidence are historical evidence, not this session's results.

Baseline: `HEAD b8f82d707f0cb99907e3d79c0c9cdc75053ef931`. Source references below
refer to the current working tree, with one-based line numbers.

- Read `PLAN.md`, `docs/audits/TAURI_FRONTEND_RENDERING_REVIEW_2026-09-05.md`,
  applicable repository instructions, scoped components/lifecycle/visibility,
  relevant existing tests, and direct state/IPC callers.
- `git diff --` for the three scoped components, lifecycle, visibility, and
  `ui/src/lib/tauri.ts` was empty. These observations are of committed code, not
  claims against an older HEAD while overlooking a current frontend patch.
- `ui/src/lib/nativeTerminalBridge.ts` does not exist in this tree. Native pane
  lifecycle/geometry use direct Tauri `invoke`; event/overlay wrappers are in
  `ui/src/lib/tauri.ts:443-496`.
- The direct `App.tsx` caller has foreign onboarding changes. Its diff was read
  and left untouched; its existing terminal props/selection path are unchanged.
  Foreign atlas, row-cache, manifest, vendor, runner, and onboarding work was not
  modified or evaluated as an owned fix.
- History checked includes `8ed71b3` (pending attachment readiness), `076e429`
  (density events), `2aef606` (masking), and recent scoped-path history.
- LSP symbols and lifecycle/provider references worked. One provider reference
  query at the preceding line failed with TypeScript's `Debug Failure`; the
  corrected declaration-position query succeeded. AST search found exactly two
  `performAttach(...)` call sites in one file: recovery at line 765 and ordinary
  attachment at line 1703. No production code was edited.

## Findings, severity ordered

### F1 - HIGH: delayed input recovery can reclaim a departed native surface owner

**Classification:** Source-proven ownership defect under the explicit rejection
ordering below. Actual frequency, compositor stacking, and screenshots remain
unproven. This is not the already-fixed warm-attachment readiness race.

**Defective branch and direct guards**

- `ui/src/components/NativeTerminalPane.tsx:729-774`: `sendInput` captures
  `currentSessionId`, awaits `cmd_native_terminal_send_input`, and on *any* first
  rejection calls `performAttach(currentSessionId, true)`. It has no live owner,
  visibility, or generation check after the await. The initial guard at
  `:718-728` only protects the original invocation. After recovery it restores
  focus and retries through `executeInput(true)`, bypassing even that guard.
- `NativeTerminalPane.tsx:680-708`: `performAttach` measures the component's
  **current** viewport ref but attaches the **captured** target ID. A tab switch
  that reuses the component can therefore supply the replacement's rectangle
  to the departed session.
- `ui/src/lib/nativeTerminalLifecycle.ts:254-259`: forced reattach deletes the
  attached marker and calls ordinary attach. At `:196-214`, that attach cancels
  both a pending same-session detach and a detach held under a replacement's
  presentation. It then establishes a new generation at `:227-233`. These are
  correct operations for a real returning owner, but recovery has not proved
  that such an owner exists.
- Contrast `NativeTerminalPane.tsx:1701-1742,1782-1823`: ordinary mount completion
  checks `isSubscribed`, and cleanup clears it and schedules detach. That local
  flag is not available to `sendInput` and cannot invalidate its continuation.
  `sessionInputRecoveries` deduplicates concurrent recovery by backend ID; it
  does not establish React ownership.

**Caller chain and reachable trigger**

1. A visible terminal A sends an input from the textarea at
   `NativeTerminalPane.tsx:2097-2108` (`onInput`), or the key path. Hold that IPC's
   result. A and B are two live terminal tabs with distinct backend IDs.
2. Select B through `App.tsx:1242-1263` ->
   `workspaceStore.ts:1008` (`activateTab`) -> `layout.ts:194-210`.
   `TerminalSplitView.tsx:716-718,793-817` renders only the group's active tab;
   its leaf path passes the selected session through `TerminalPane.tsx:133-140`.
   A's effect cleans up and B attaches. Keep B's first bounds receipt pending.
3. `nativeTerminalLifecycle.ts:196-210,129-142` holds A's outgoing teardown under
   B until B presents. A's native host is intentionally still present here.
4. Reject A's earlier input request, then allow recovery attach/retry to settle.
   `sendInput` still uses A. Forced attach cancels the held teardown of A.
5. Resolve B's first bounds receipt with `presented: true`.
   `NativeTerminalPane.tsx:1603-1607` releases B's waiters, but A's waiter was
   removed. A is left attached without a corresponding current pane owner.

This error ordering is supported by a real asynchronous IPC path, not just an
invented rejecting function: `src-tauri/src/ipc/native_terminal.rs:791-830`
awaits a daemon write, then requests a receipt on the main thread, and propagates
errors from either stage. The defect is conditional on that request failing; this
audit did not cause a real daemon-write failure.

The native guard does not refute this sequence:
`surface_host.rs:1498-1517` removes a host only when detach actually executes;
the sequence above cancels it before that point. Native attach at
`ipc/native_terminal.rs:460-465` calls warm reattach, which marks
`surface_attached = true` at `surface_host.rs:1033` and updates layout/bounds at
`:1096-1099`. Scheduled rendering at `surface_host.rs:337-362` accepts that flag
and uses the retained host. `ensure_surface_attached` at `:830-837` protects a
genuinely detached session, not this falsely renewed one. Old content continuing
at retained/replacement bounds is thus a supported mechanism; whether it paints
above B on a particular OS still needs real-surface evidence.

There is a second trigger for the same missing lifetime check: insert Settings
(`App.tsx:2000-2010`) or search (`TerminalPane.tsx:141-146`) while input is pending,
let visibility cleanup detach A, then reject the old input. Recovery issues a new
attach despite the owner now being hidden. Do not assume that attach alone
recreates visible pixels after the host was already removed: scheduled rendering
uses an existing host, rather than constructing one (`surface_host.rs:357-359`).
The unauthorized reattachment/retry is source-proven; overlay pixel leakage in
that particular ordering is not.

**Existing-test coverage**

- `NativeTerminalPane.test.tsx:1388`: `self-heals detached session on send_input
  error by re-attaching and retrying input once` rejects immediately and leaves
  the owner unchanged. `:1427` covers repeated failure, not departure.
- `NativeTerminalPane.lifecycle.test.tsx:253-423` covers Settings/search/provider
  suppression and blocks newly dispatched input while hidden. It does not settle
  an already-running input request after hiding or switching.
- `NativeTerminalPane.test.tsx:986,1019` covers outgoing presentation handoff and
  rapid A/B/C replacement without input recovery in flight.
- `nativeTerminalLifecycle.test.ts:109` intentionally tests cancellation when a
  departed pane **really returns**. That guard must remain; it is not the bug.

**Cheapest faithful RED specifications (proposed, not yet executable tests)**

Add to `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`, retaining the
real component and real lifecycle module; fake only Tauri IPC and layout reads.
Reset lifecycle/pane singleton state per test. Use distinct A/B IDs and rectangles.

1. Test name: `does not reclaim the outgoing surface when input fails after tab replacement`.
   Given A presented, input result deferred, and B's first bounds result deferred.
   When the obsolete A input rejects after the A -> B rerender has registered
   B's bounds request, settle recovery and then B's valid presentation receipt.
   Then A has no second attach/no input retry, A detaches once, and B remains
   attached. Current source instead reattaches A and cancels its teardown.
2. Test name: `does not recover input when its owner becomes hidden`.
   Given A's input result deferred, rerender the real visibility provider to
   false and await the detach IPC completion. When the old input rejects, then
   no attach or retry is emitted. Current source emits both. A separate actual
   dialog/MutationObserver integration variant should exercise the same contract.

Literal RED commands, **after adding those named tests**:

```sh
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not reclaim the outgoing surface when input fails after tab replacement'
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'does not recover input when its owner becomes hidden'
```

Use explicit deferred `inputStarted`, `boundsStarted`, `detachCompleted`, and
controlled settlement promises registered before each action; await React `act`
and the relevant promise reactions. No sleeps, polling, or timeout-driven success.
An external bounded test timeout is only a failure bound. The current suite's
`waitFor` usage must not be copied into this regression's ordering mechanism.

**Recommended fix seam:** carry a live pane-ownership token through recovery,
invalidate it on target/visibility/unmount transitions, and recheck it before
forced attach, retry, and result publication. Preserve the legitimate same-session
warm reparent cancellation and shared attachment readiness. Do not fix this by
removing lifecycle's returning-owner guard or by closing the daemon PTY.

**Real-surface scenario for acceptance:** in an isolated debug app launched only
with `bun tauri dev`, create tabs A and B with different continuously identifiable
content. Use a journaled, event-gated IPC fault hook to hold A's input response
and B's first presentation, select B through the actual tab UI, fail A's held
response, and release B. Inspect native attached-host IDs, verify only B occupies
the selected pane, resize B, and confirm A's output does not repaint over it.
Repeat with a split-to-tab move so old/new rectangles differ, and with Settings
opened during the held input. Check that PTYs survive, hidden owners stay hidden,
and input is not replayed after departure. A visual pass without forcing/observing
the error ordering does not reproduce this finding. No part of this scenario ran
in this diagnosis lane; any fault hook must be removed by its execution owner.

## Disproven hypotheses and bounded source checks

The following are source-observation PASS only, **not runtime PASS**:

| Hypothesis | Checked source/caller or existing coverage | Result |
| --- | --- | --- |
| Warm reuse resolves before native attach is ready | `nativeTerminalLifecycle.ts:219-224`; `8ed71b3`; readiness/failure/already-started-detach tests at `nativeTerminalLifecycle.test.ts:25-84` | Pre-existing fix present; do not re-report. |
| Same-turn split reparent tears down a returning sibling | Generation checks at lifecycle `:31-59`, same-session cancellation `:196-214`; component cleanup `:1802-1807`; lifecycle sibling/held-detach tests and component reparent tests `NativeTerminalPane.lifecycle.test.tsx:226`, `NativeTerminalPane.test.tsx:4656` | Existing guards cover legitimate returning owners; F1 bypasses their caller precondition. |
| Fixed CSS bounds prevent a density update | `NativeTerminalPane.tsx:1769-1779` rearms the resolution query and subscribes to window resize; cleanup `:1784-1785`; lifecycle density test `:425` | September 5 fix present, no new frontend density defect found. |
| The latest resize is discarded while IPC is pending | `NativeTerminalPane.tsx:1681-1694,1645-1654`; split divider `TerminalSplitView.tsx:1275-1327` -> store ratio actions `workspaceStore.ts:1029-1041` -> `layout.ts:537-553` | Latest measured geometry is retained; coalescing test at `NativeTerminalPane.test.tsx:705` covers the positive path. |
| Zero-area or failed bounds are permanently deduplicated | Geometry gate `NativeTerminalPane.tsx:671-675`; failure/cache handling `:1620-1633`; retry tests at `NativeTerminalPane.test.tsx:843,916` | No cache-poisoning finding. Generic bounds errors need another measurement or banner retry; no automatic retry guarantee claimed. |
| An old normal bounds completion publishes after departure | `NativeTerminalPane.tsx:1595,1647,1796`; backend attached-host guard `surface_host.rs:842-845,1720` | Normal stale completion is suppressed; this does not cancel already-issued native IPC or prove all same-session native scheduling orders. |
| Dropped presentation immediately releases the outgoing pane | `NativeTerminalPane.tsx:1596-1602`; lifecycle tests `:130,161` | `presented:false` retries through rAF, and unmount cancels retry. |
| Frontend ID is used despite a provided backend ID | `NativeTerminalPane.tsx:536`; `TerminalPane.tsx:133-140`; differing-ID/null-binding tests `NativeTerminalPane.test.tsx:4406,4498` | Backend identity guard present. |
| Normal modal masking or opt-out remains broken | `nativeTerminalVisibility.tsx:20-25,54-68`; actual search/settings mounts above; full visibility-hook test file read | September 5 opt-out repair present; real dialog/search mounts trigger observation. |
| Pane-to-tab conversion respawns rather than moves ownership | `TerminalSplitView.tsx:429-432` -> `workspaceStore.ts:889-906` -> `layout.ts:400-477` | Same session mapping is moved. Pane-to-tab-row tests mock TerminalPane, so their name does not imply native pixel coverage. |
| Every terminal must disappear for a pane drag | `TerminalSplitView.tsx:326-333,1012-1020`; `TerminalSplitView.dragFeedbackVisibility.test.tsx:161-219` | Current contract deliberately masks the feedback target only; browser panes separately hide for the entire drag. No redesign proposed. |

## Limits and unresolved hypotheses

- Position-only bounds drift remains **unproven**, not a finding. The component
  observes size plus window/density events, not arbitrary ancestor translation.
  The checked tab/swap/reparent callers change ownership/effect or size. No
  concrete incumbent action was established that silently translates the same
  owner at identical size without those signals. Do not add continuous polling
  based only on ResizeObserver's known limitation.
- Visibility observes child-list mutations, not attribute changes, and nested
  providers replace rather than combine context. The only production provider
  caller found was `TerminalSplitView`; no nested-provider or dynamic opt-out
  caller was found. Those API-level possibilities are not product defects here.
- Outgoing-surface retention when a replacement never presents is a remaining
  error-path hypothesis. Generic bounds errors, zero-area deferral, and failed
  attach have different behavior. No permanent blank/stale surface scenario,
  including F1's pixel manifestation, was reproduced in this lane.
- This lane did not audit renderer atlas/row-cache internals or claim that F1
  explains glyph corruption on display switching. The native-host reads above
  only test the frontend finding against its direct IPC guards.
- Existing tests include polling-style waits and mocked/undefined IPC receipts;
  reading their assertions is not proof of deterministic runtime coverage.
  No source test was edited or skipped. No all-clear is inferred from file names.
- Only this report was written. Source review is complete at the bounded scope;
  F1 still requires captured right-reason RED and real native-surface validation
  before any repair or aggregate rendering acceptance can be claimed.
