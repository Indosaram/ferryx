# Ferryx Terminal Scale Implementation Result

## Outcome

Ferryx now budgets terminal **renderers** independently from live backend PTYs. It supports the required five workspaces with twenty already-running sessions each in deterministic lifecycle and navigation coverage, while retaining bounded output and the existing remote/mobile source contract.

## Delivered behavior

- Visible terminal panes are ref-count pinned; inactive xterm/WebGL renderers are retained only in a bounded LRU warm cache.
- LRU eviction disposes frontend resources only. It does not spawn or close a backend terminal, so an evicted pane reconnects to the same continuing PTY.
- Per-renderer output is ordered and animation-frame batched. A 128 KiB queued-output cap flushes synchronously rather than allowing an unbounded throttled-frame queue.
- Inactive sessions stop writing to xterm but retain bounded replay and OSC title/activity state through the frontend event bus.
- Async renderer creation is invalidated on session destruction. Late completions dispose themselves, cannot repopulate the cache, and cannot erase a newer create for the same session.
- Selecting an existing tab or parked workspace remains a state-only path: the five-workspace/100-session test asserts no terminal spawn, close, wait, event-start, or CWD operation.
- The native output hub remains the session/remote authority. Its remote attach path provides retained history first, followed by live broadcast.

## Code organization

`ui/src/lib/terminalHostManager.ts` owns renderer lifetime, visibility, cache eviction, and create/destroy invalidation. `terminalInstanceFactory.ts` owns xterm/WebGL construction and disposal. `terminalOutputScheduler.ts` owns output batching. Their measured nonblank/noncomment line counts are 237, 164, and 52 respectively.

Lifecycle tests are divided into focused core, output, LRU, and scale suites, with a shared deterministic fixture. The measured test/helper file counts are 180, 112, 141, 83, and 104 pure lines.

## Verification

| Check | Result |
| --- | --- |
| `bun run --cwd ui test -- terminalHostManager TerminalPane terminalEvents workspaceStore` | PASS — 13 files, 58 tests |
| `bun run --cwd ui build` | PASS — `tsc && vite build` |
| `bun run --cwd ui test` | PASS — 70 files, 493 tests |
| `cargo test --manifest-path src-tauri/Cargo.toml output_hub` | PASS — 2 output-hub tests |
| `git diff --check` | PASS — clean |

The Rust command emitted three pre-existing dead-code warnings for `WriterLeaseGuard` in `src-tauri/src/worktree/manager.rs`; this terminal-scale work did not change that code.

## Reactivation viewport alignment follow-up

The renderer now uses an at-bottom-aware `fitTerminal` operation. Each existing fit path preserves an explicit user scrollback position, but re-aligns a terminal that was following its live output after xterm's fit/reflow. This prevents a fit that changes terminal rows from leaving the viewport exactly one text row above the latest output.

The deterministic regression proof is in `ui/src/components/TerminalPane.test.tsx`: one test simulates a post-mount fit shifting a bottom-following viewport by one row and verifies recovery; the companion test verifies a deliberately scrolled-up viewport is unchanged. See [TERMINAL_VIEWPORT_OFFSET_FIX.md](TERMINAL_VIEWPORT_OFFSET_FIX.md) for the full red/green record.

## Desktop acceptance remains user-run

Desktop input automation was intentionally not used because the workspace owner prohibits it. Run the focused Scenario D in [MANUAL_DESKTOP_QA.md](MANUAL_DESKTOP_QA.md), then return a screenshot or short recording and PASS/FAIL. The five-workspace Scenario C can be recorded as `DEFERRED` while its setup is unavailable. These manual outcomes are not represented as automated passes.

## Scope hygiene

No commit was created. The repository already contained extensive unrelated changes, which were left intact.
