# xterm.js → libghostty Migration Report

Desktop terminal surfaces migrated off xterm.js onto the native libghostty/wgpu path.
The in-terminal glyph display was explicitly out of scope. The remote/web browser client
keeps xterm.js, because a browser cannot host the native renderer.

## Result per criterion

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | Native for every pane | MET | `nativeTerminalDev.ts` deleted; `TerminalPane.tsx` renders `NativeTerminalPane` unconditionally (49 lines, no `isTauriRuntime()` branch, no `terminal-mount` path); `TerminalPane.nativeAlways.test.tsx` 4/4 |
| 2 | Input surfaces on libghostty | MET | `native_terminal_capability_contract` 6/6, `native_terminal_input_boundary_contract` 10/10; six commands defined and registered in `lib.rs` |
| 3 | Title and bell reach React | MET | backend `native_terminal_daemon_pump_pushes_title_and_bell_and_detach_stops_events`; frontend `NativeTerminalPane.titleBell.test.tsx` 3/3 |
| 4 | Theme drives the renderer | MET | `native_terminal_renderer_contract` 19/19 |
| 5 | Simultaneous split panes render natively | MET | `native_terminal_surface_host_contract` 8/8 |
| 6 | Desktop xterm legacy deleted, remote intact | MET | `rg -l "@xterm" ui/src` → `remote/RemoteTerminal.tsx` + `lib/terminalRenderer.ts` only; build exit 0 |
| 7 | Full gates green | MET for migration scope | see Gates; 3 pre-existing daemon failures reported separately |

Every backend criterion was mutation-proven: the assertion was broken deliberately and the
matching test had to fail before the criterion was accepted.

## Gates (each run once, exit codes captured)

```
cargo check --manifest-path src-tauri/Cargo.toml --all-targets   exit 0, 0 errors
cargo test  --manifest-path src-tauri/Cargo.toml                 all suites ok except 3 pre-existing daemon tests
bun run --cwd ui test                                            76 files, 611 tests passed
bun run --cwd ui build                                           exit 0, built in 4.32s
```

## What was deleted

Desktop-only xterm modules (12 files): `terminalHostManager.ts` and its five tests,
`terminalInstanceFactory.ts`, `terminalOutputScheduler.ts` and its metrics test,
`terminalHostManagerTestHelper.ts`, `terminalFit.test.ts`, `terminalRenderer.test.ts`.

`@xterm/addon-search` was removed from both `loadTerminalAssets` and `package.json` after
confirming its only consumer destructured just `Terminal` and `FitAddon`.

## What deliberately survives

`ui/src/remote/RemoteTerminal.tsx` keeps xterm.js. It imports `lib/terminalRenderer.ts`,
which loads `@xterm/addon-unicode11` — required for East-Asian wide-glyph width in the
browser terminal. Four packages remain in `package.json`, each with a real importer:
`@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-webgl`, `@xterm/addon-unicode11`.

The invariant to preserve in both directions: every package in `package.json` has at least
one importer under `ui/src`, and every `@xterm` import under `ui/src` has its package
declared. Two separate agents broke the remote path by deleting these shared modules; the
`bun run --cwd ui build` gate is what catches it.

## Test count delta: 641 → 611

Not a regression. Eight desktop xterm test files were deleted along with the modules they
covered, and `TerminalPane.test.tsx` went from 5 tests to 1 because its five tests mocked
the now-deleted `terminalHostManager`. Added: 4 (`nativeAlways`) + 3 (`titleBell`).
Equivalent behavior is covered by `NativeTerminalPane.test.tsx` (30 tests).

## Pre-existing failures, not caused by this work

Three tests in `src-tauri/tests/daemon_persistence_contract.rs` fail:
`test_daemon_output_sequence_contiguity_and_replay_gap`,
`test_daemon_gui_process_non_ownership_and_process_tree`,
`test_daemon_terminal_persistence_reconnect_replay_and_isolation`.

All three die in `daemon.connect_client()` with `Os { code: 2, kind: NotFound }` — the UDS
socket is missing. Proven pre-existing by running the same suite in a clean worktree at
HEAD `93dcaab`, where the same three fail. The daemon changes in the working tree are a
Windows/TCP portability refactor with zero references to native terminal, title, or bell.

Separately, `ui/` carries 16 typecheck errors from an unrelated, incomplete
`RegisteredProject.gitRoot` refactor. `lib/types.ts` is clean against HEAD and never
defined that type. Untouched and unsuppressed by this migration.

## Remaining work

On-screen GUI verification is not automated and was never claimed as verified. Run
`docs/NATIVE_TERMINAL_MANUAL_E2E_CHECKLIST.md` (sections A–K, 48 items) against a desktop
build to confirm scroll, selection, copy, paste, mouse reporting, search, title, bell, and
theme behave correctly on real panes.
