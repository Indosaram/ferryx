# P07 local repair - 2026-09-13

## Outcome and scope

RF-07 / C002 repaired locally in exactly the two allocated source/test files:

- `ui/src/lib/nativeTerminalVisibility.tsx`
- `ui/src/lib/nativeTerminalVisibility.test.tsx`

The shared yielding-surface selector now includes Sonner's actual `[data-sonner-toast]` items. It does not match empty toaster containers or arbitrary live regions. Existing closest-ancestor opt-out handling and platform/owner calculations are unchanged. Toast items keep presentation yielded through their exit animation until DOM removal.

No other source files were changed by this task. No install, build, GUI, daemon, SSH, worktree, commit, or release operation was performed. Execution was local Darwin arm64 using the existing UI dependencies (Vitest 3.2.7).

## Mechanism traced

`App.tsx` mounts the UI Sonner wrapper globally and emits error toasts. The wrapper renders the real Sonner Toaster at bottom-right with a close button. Sonner emits a persistent `section[aria-live="polite"]`, adds `[data-sonner-toast]` items through a scheduled React flush, and removes dismissed items after its exit animation. Its `ol[data-sonner-toaster]` is absent while empty.

The visibility hook previously recognized only dialog/search roles, so toast insertion never changed either returned visibility flag. `NativeTerminalPane` consumes the hook's presentation flag in its surface-attachment effect and its interactive flag for input; cleanup detaches native presentation. `BrowserPane` also consumes the interactive helper. The repair is at their shared overlay-classification seam, not individual toast callers or session code.

## Faithful regression

Four parameterized cases mount the real `Toaster` from `sonner`, under the real visibility provider, with a real hook consumer:

1. Non-Mac: empty toaster leaves terminal visible/interactive; persistent error toast yields both; actual close-button click restores both.
2. Mac: toast leaves presentation visible but disables interaction; dismissal restores interaction.
3. Explicitly hidden owner: stays hidden before, during, and after toast.
4. Opted-out toaster ancestor: toast never hides or disables the owner.

The hook installs its real MutationObserver before toast creation. Async React `act` completion flushes observer delivery and the resulting state commits. Controlled timers complete Sonner's deferred mount, requestAnimationFrame dismissal dispatch, and exit-animation removal; no fixed-duration advancement, sleeps, polling, or waitFor is used. The toast has `duration: Infinity`, so it cannot auto-expire before assertions. Dismissal is driven through Sonner's real close button, not a replacement Toaster or handwritten toast DOM.

A mount-scoped effect verifies the same hook consumer survives both presentation transitions and is released exactly once on explicit unmount. This is local React ownership evidence, NOT a claim that an actual native terminal or PTY was mounted. Native session destruction is outside this hook's responsibilities and unchanged by the selector repair. Same-PTY survival remains pending Windows runtime evidence.

Cleanup dismisses only the fixture's owned toast ID, drains its scheduled removal work (including on assertion failure), unmounts React roots, drains remaining owned scheduler callbacks, asserts zero pending fake timers, and restores real timers. Existing suite cleanup removes fixture DOM; the existing observer-unmount test remains passing. Only the platform classifier is mocked in the added regression; Sonner, React, DOM, and MutationObserver are real. The mount-release spy is an observation callback, not a substituted session implementation.

## Exact command and receipts

Both behavioral RED and GREEN used the identical command and identical final test code:

```sh
bun run --cwd ui test src/lib/nativeTerminalVisibility.test.tsx
```

### Fixture calibration (not behavioral RED)

- 13:18:48: exit 1; 18 tests, 4 failed / 14 passed. Incorrect fixture assumption: empty Sonner has no `[data-sonner-toaster]` list. Replaced that empty-state assertion with the actual persistent `section[aria-live="polite"]` container.
- 13:19:10: exit 1; 18 tests, 2 failed / 16 passed. On the intended failing paths, Sonner's removal effect queued one timer after the first `act` completed, so cleanup's zero-timer assertion masked the behavioral assertion. Added post-unmount scheduler drain before asserting zero timers. No product change had been made.

### Intended RED - 13:19:30

Exit 1; 1 failed file; **18 tests: 2 failed, 16 passed**; duration 486 ms.

Both failures were at the same mounted-toast visibility assertion (`nativeTerminalVisibility.test.tsx:65`):

```text
non-Mac:
  expected { visible: false, interactive: false }
  received { visible: true, interactive: true }
Mac:
  expected { visible: true, interactive: false }
  received { visible: true, interactive: true }
```

The real toast DOM assertion had already passed. Hidden-owner, opt-out, and the 14 existing cases passed. Cleanup completed without masking either failure.

### GREEN - 13:20:00

After only the production selector/comment edit: exit 0; 1 passed file; **18 tests passed**; duration 490 ms. The exact final regression passed on its first post-fix run, including real close-button dismissal, presentation restoration, retained hook ownership, and timer cleanup.

### Static verification

- LSP diagnostics on each changed TSX file: **No diagnostics found**.
- `git diff --check -- ui/src/lib/nativeTerminalVisibility.tsx ui/src/lib/nativeTerminalVisibility.test.tsx`: exit 0, no output.
- Reviewed scoped diff: one selector addition plus explanatory comment; four regression cases and imports, no unrelated production changes.
- Nonblank/non-line-comment counts: source **65**, test **232**. Test file is in the 200-250 warning band; if future edits add substantial coverage, split toast scenarios by responsibility under a separately authorized allocation. No unrelated split performed.

## Architectural review

- Responsibilities: source owns native-overlay visibility policy; tests own that policy's behavioral regression coverage.
- Boundary purity: existing typed platform/React inputs; no new untrusted data boundary.
- Variant discrimination: no new tagged-variant branching.
- Escape hatches: none added; DOM close-button lookup uses a typed generic and explicit missing-fixture failure, not an assertion cast.
- Defensive layers: none added to production.
- Helpers: no one-off production helper or abstraction introduced.
- Tests: final regression fails on the unfixed selector and passes unchanged after repair.
- Parameter bloat: none introduced.
- Redundant verification: no production destructive-operation re-query; DOM/timer checks are regression assertions.
- Negative naming: none introduced.
- Logging: unchanged.

## Pending / limits

Lead owns combined tests/build verification. Actual Windows error-toast visibility and clickability over the bottom-right native surface, restore of the same terminal PID/session and viewport, and native input delivery remain pending. jsdom policy/React-lifecycle evidence cannot establish HWND stacking, pointer routing, or same-PTY survival. No Windows runtime acceptance is claimed.
