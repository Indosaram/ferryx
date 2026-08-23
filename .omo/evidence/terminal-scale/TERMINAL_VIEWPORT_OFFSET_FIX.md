# Terminal Reactivation Viewport Offset Fix

## Reported behavior

After a terminal tab was reactivated, the terminal viewport could appear exactly one text row above its newest output.

## Cause

Warm renderer reactivation replays terminal output while the pane's delayed font fit and its resize observer can still call xterm's `FitAddon.fit()`. When the viewport is following live output, that reflow can change the row geometry and leave xterm's viewport one row short of its active buffer base.

## Fix

`fitTerminal` records whether `viewportY === baseY` before a fit. It executes the fit and calls xterm's public `scrollToBottom()` only when the terminal was already following live output. A deliberately scrolled-up viewport is not moved.

The helper is used by all existing terminal fit paths:

- Resize-observer fit in `ui/src/lib/terminalInstanceFactory.ts`
- Font-ready post-mount fit in `ui/src/components/TerminalPane.tsx`
- Terminal settings fits in `ui/src/lib/terminalHostManager.ts`

No PTY, output subscription, LRU cache, event-bus replay, Rust remote service, or mobile protocol behavior changed.

## Failing-first proof

Command before the fix:

```text
bun run --cwd ui test -- TerminalPane --reporter=verbose
FAIL src/components/TerminalPane.test.tsx > TerminalPane mounted sizing > realigns a bottom-following terminal after the post-mount font fit shifts it by one row
AssertionError: expected "spy" to be called once, but got 0 times
```

The test models an xterm viewport at `baseY = viewportY = 12`; its fit simulation moves `viewportY` to `baseY - 1`. It fails without the post-fit alignment because `scrollToBottom()` is never called.

## Green verification

```text
bun run --cwd ui test -- TerminalPane terminalHostManager --reporter=verbose
PASS: 5 files, 19 tests
```

The focused run now covers 6 files / 22 tests, including the `TerminalPane` activation assertions and direct `terminalFit` helper checks:

1. A bottom-following terminal returns to `viewportY === baseY` after a fit moves it one row.
2. A deliberately scrolled-up terminal does not call `scrollToBottom()` after fit.

Final regression evidence:

```text
bun run --cwd ui test
PASS: 71 files, 498 tests

bun run --cwd ui build
PASS: tsc && vite build

cargo test --manifest-path src-tauri/Cargo.toml output_hub
PASS: 2 tests
```

The native command emitted only the pre-existing `WriterLeaseGuard` dead-code warnings in `src-tauri/src/worktree/manager.rs`.

Desktop surface confirmation remains user-run under Scenario D in [MANUAL_DESKTOP_QA.md](MANUAL_DESKTOP_QA.md), because agent desktop input automation is prohibited. The 5×20 workspace-scale Scenario C is explicitly `DEFERRED` at the owner's request until that setup is available.
