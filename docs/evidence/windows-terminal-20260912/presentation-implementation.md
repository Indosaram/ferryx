# Retained-presentation repair (presentation ownership vs. live PTY readiness)

Status: **LANE COMPLETE — scoped suites GREEN; no overall-green claim.** RED baseline is captured in `baseline.md` (HEAD `99e0450d`, run `mon_6J2JG08TYR85MD42`: 3 failed / 53 passed). The pinned GREEN run below replaces it unweakened: same tests, same runner, same flags. The overall Windows startup goal stays open — the bounds diagnosis lane is separately active, and the combined build plus full-suite verdict are owned by the lead.

## Pinned verification (exact, run once, `--maxWorkers=1`)

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx
```

Directly affected suites included in the same verification batch:

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/NativeTerminalPane.exitAttach.test.tsx \
  src/lib/nativeTerminalLifecycle.test.ts \
  src/components/TabBar.test.tsx
```

Runtime is Node (`node node_modules/vitest/vitest.mjs`), not Bun. No sleeps or polling: every async boundary awaits an exact mock promise or act flush.

### Pinned test IDs

File `src/components/NativeTerminalPane.presentation.test.tsx`, describe `native terminal presentation retention`:

1. `does not steal dialog focus when an earlier attachment finishes` (was green, must stay green)
2. `keeps a presented macOS surface under a dialog without accepting terminal keys` (was green)
3. `does not recover an in-flight input after a dialog takes ownership` (was green)
4. `retains a shown final frame on exit, blocks input, and releases it on unmount` — **RED in baseline**
5. `does not attach a cold exited pane and leaves an opaque fallback` (was green)
6. `yields an exited native surface outside macOS so the disconnect overlay is not occluded` (was green)
7. `never reattaches a dead PTY when retained-frame geometry recovery is requested` — **RED in baseline**
8. `does not retain a first frame that arrives after the session exited` (was green)
9. `holds the final frame until a reconnected replacement is presented` — **RED in baseline**

File `src/components/NativeTerminalPane.lifecycle.test.tsx`, describe `NativeTerminalPane compositor ownership lifecycle`: all 30 baseline-green cases (including `retries failed bounds with a fresh attachment and waits for presentation`, `retains the outgoing pane until a dropped frame is retried and presented`, `does not reclaim the outgoing surface when input fails after tab replacement`, `updates device scale without reattaching when monitor density changes at fixed logical bounds`, `handles React StrictMode remount cleanly preserving attach sequence`, `skips stale detachment when remount occurs while detachment is in flight and remains attached`) must stay green unweakened.

## Causal proof (RED -> fix)

All three baseline failures share one root cause: the compositor surface effect treated
**live PTY attachment** as the only form of presentation ownership.

Pre-change chain, macOS, session `backend-a` presented then flipping to `lifecycle: "exited"`:

1. `isExited` forces `targetSessionId = null` and `bindingKey = null`, but `surfaceSessionId`
   retains `backend-a` through `retainedPresentation` (macOS-only branch, `isMacShortcutPlatform()`).
2. The surface effect (deps include `bindingKey`) re-runs. Layout effects set
   `attachmentOwnerRef = { sessionId: "backend-a", bindingKey: null, live: false }`.
3. The effect cleanup unconditionally scheduled
   `detachNativeTerminalLifecycle("backend-a")` — killing the surface the pane is supposed to
   keep showing.

From that single flaw each baseline failure follows deterministically:

- **Failure 4 (`retains a shown final frame…`)**: the scheduled detach invokes
  `cmd_native_terminal_detach`, so `expect(commands("cmd_native_terminal_detach")).toHaveLength(0)`
  fails with 2 recorded calls. The second call is a cascade: the detach's success handler
  `setPresentation(null)` clears `retainedPresentation`, which nulls `surfaceSessionId`, which
  re-keys the effect again, whose cleanup detaches a second time.
- **Failure 7 (`never reattaches a dead PTY…`)**: the post-exit effect run starts with
  `isAttached = false` and `attemptAttach` correctly exits on `!owner.live`. Because nothing
  re-presents bounds for the retained surface, a geometry change + `set_bounds` failure never
  happens, `error` stays null, and `getByRole("alert")` finds nothing.
- **Failure 9 (`holds the final frame until a reconnected replacement is presented`)**: the
  outgoing detach had already executed by the time `backend-b` attaches, so
  `attachNativeTerminalLifecycle("backend-b")` had no pending detachment to park under itself via
  `holdDetachmentForPresentation`; `presentNativeTerminalLifecycle` later released nothing. The
  retained frame was destroyed before the replacement was presented.

### The fix (ui/src/components/NativeTerminalPane.tsx only)

Presentation ownership is now tracked by `attachmentOwnerRef` independent of `live`:

1. **Cleanup handoff guard**: the surface-effect cleanup skips
   `detachNativeTerminalLifecycle` when the incoming render's layout effect re-armed the same
   surface as a retained presentation (`nextOwner && !nextOwner.live && nextOwner.sessionId ===
   targetSessionId`). React runs layout destroys/creates of the incoming render before passive
   destroys, so the guard reads the successor's decision, not the outgoing one's. On true unmount
   the layout cleanup nulls the ref first, so the detach still fires exactly once; on replacement
   the ref points at the new session, so the detach is scheduled and then parked by
   `holdDetachmentForPresentation` until the replacement presents.
2. **Retained setup seeds geometry**: when the effect starts in retained mode
   (`attachmentOwnerRef` same session, not live) it seeds `lastGeometry = measureGeometry()` and
   `isAttached = true`. The retained compositor surface therefore tracks pane geometry
   (resize/density/ResizeObserver re-present bounds), while `attemptAttach` keeps its
   `!owner.live` early return so a dead PTY is never reattached.
3. **Retained recovery reroutes**: the bounds-failure retry (`retryBoundsRef`) re-sends geometry
   via `reportBounds()` when the owner is not live instead of calling `attemptAttach`, which is a
   guaranteed no-op there; live panes keep the existing fresh-attachment recovery.

Windows and non-mac platforms are untouched: the retained branch of `surfaceSessionId` only
exists under `isMacShortcutPlatform()`, and `nextOwner.live` is always true for every live
rerender, so cleanup behavior there is byte-identical. `nativeTerminalLifecycle.ts` needed no
change: cancellation/parking (`holdDetachmentForPresentation`, attach-side pending-detach cancel)
already existed and now gets the chance to run.

## GREEN receipt (lane scope only)

Pinned command (unchanged from the section above), single run:

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx
```

Observed 2026-09-12: exit code `0`.

```text
✓ src/components/NativeTerminalPane.lifecycle.test.tsx (30 tests)
✓ src/components/NativeTerminalPane.presentation.test.tsx (9 tests)
Test Files  2 passed (2)
     Tests  39 passed (39)
```

All nine pinned presentation test IDs pass, including the three baseline failures:

- `retains a shown final frame on exit, blocks input, and releases it on unmount` — GREEN. The exit rerender records zero `cmd_native_terminal_detach` calls, `data-native-terminal-presented="true"` persists, keydown reaches no `cmd_native_terminal_send_input`, and unmount records exactly one `["cmd_native_terminal_detach", { sessionId: "backend-a" }]`.
- `never reattaches a dead PTY when retained-frame geometry recovery is requested` — GREEN. The retained surface re-presents bounds on resize, the `set_bounds` failure renders the `role="alert"` recovery button, and clicking it leaves `cmd_native_terminal_attach` at 1 call (the original live attach).
- `holds the final frame until a reconnected replacement is presented` — GREEN. The outgoing `backend-a` detach stays uninvoked until the `backend-b` receipt resolves, then fires exactly once as `["cmd_native_terminal_detach", { sessionId: "backend-a" }]` with `data-native-terminal-presented="true"`.

All 30 lifecycle cases stay green unweakened (no test file was modified), including `retries failed bounds with a fresh attachment and waits for presentation` (alert click still reattaches when the owner is live: 2 attaches), `retains the outgoing pane until a dropped frame is retried and presented`, `does not reclaim the outgoing surface when input fails after tab replacement`, and the StrictMode / stale-detachment-race guards.

### Directly affected suites (same batch, one run each)

```text
✓ src/lib/nativeTerminalLifecycle.test.ts (7 tests)            (module untouched — regression guard)
✓ src/components/TabBar.test.tsx (19 tests)                    (baseline companion; foreign shell-lane edits in worktree)
✓ src/components/NativeTerminalPane.test.tsx                   \
✓ src/components/TerminalSplitView.shell.test.tsx               } 169 tests passed (3 files, one run)
✓ src/lib/nativeTerminalVisibility.test.tsx                    /
```

### Lead's independent combined run (corroboration)

The lead independently ran six suites: 71 passed, 2 failed — the two failures are exactly the pre-existing `exitAttach` bindingKey mismatch documented below; `NativeTerminalPane.lifecycle` (30), `NativeTerminalPane.presentation` (9), shell (23) and the lifecycle lib (7) all pass. The production fix is verified on disk by the lead; the combined build is run by the lead, so no duplicate build was executed in this lane.

### Pre-existing failures (not caused by this lane, left untouched)

`src/components/NativeTerminalPane.exitAttach.test.tsx` — 2 of 4 cases fail and are pre-existing, established **without mutating the shared tree**: (a) commit archaeology — `git log -S "classification.reason, bindingKey"` attributes the third `bindingKey` argument to commit `7404a44f`, which predates this lane; (b) the failing run's received-args diff shows the spy called with the extra third argument (`dead-backend-session::0:` / `legacy-dead-session::0:`), which this lane's +39/−3 surface-effect diff cannot introduce — no edit touches `onBackendSessionUnavailable`. The lead's independent six-suite run reproduces exactly these two cases while every lane suite passes. Assertions were left untouched as instructed; the owning lane must update them.

### Constraints honored

- Shared-tree discipline correction: the one-off `git stash`/`stash pop` used to probe pre-existence is retracted as an evidence method — it is forbidden shared-tree practice and will not be repeated. Nothing was reverted; the transient probe was immediately popped and the production diff is verified present on disk by the lead. Future pre-existence proofs use commit archaeology or received-args output only, never tree mutation.
- Writes limited to `ui/src/components/NativeTerminalPane.tsx` (+39/−3) and this report; `nativeTerminalLifecycle.ts` needed no change — its parking/cancellation already covered the replacement path once cleanup stopped pre-empting it.
- Foreign concurrent edits (shell-selection lane: `TabBar.tsx/.test.tsx`, `TerminalSplitView.*`, `src-tauri/*`, new `TerminalSplitView.shell.test.tsx`) preserved; verification ran on the merged worktree state.
- No commits, pushes, release builds, desktop launches, or daemon changes. One-shot `vitest` process only; cleanup receipt: process exited, no server/browser/desktop spawned.
- LSP diagnostics on the edited file: no errors.
- Windows behavior untouched: the retained branch of `surfaceSessionId` is gated on `isMacShortcutPlatform()`, and every live-rerender owner has `live: true`, so cleanup/attach behavior on Windows and Linux is unchanged. This lane is not the Windows startup root fix; the bounds diagnosis remains separately active.
