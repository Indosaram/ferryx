# Terminal Scale Runtime Implementation Plan

## Goal

Make Ferryx practical for at least five workspaces with 20 or more concurrently running terminal agents each, while preserving live PTY processes, bounded replay, mobile remote control, and immediate navigation among already-known workspaces and tabs.

## Constraints

- Keep `portable-pty` and the Rust output hub as the session-lifetime authority.
- Keep xterm.js as the browser and mobile-compatible renderer for this increment; do not migrate to libghostty.
- Render only visible terminal panes, retain only a bounded least-recently-used warm renderer cache, and never close a PTY merely because its renderer is evicted.
- Deliver output to active renderers in bounded animation-frame batches without reordering a session's stream.
- Preserve bounded frontend and Rust replay buffers plus activity/title tracking for inactive sessions.
- Switching to a known workspace or tab must select state without spawning terminals, performing network work, or waiting for a PTY.
- Preserve existing user and agent changes outside this implementation's owned paths. No commit unless explicitly requested.

## Baseline Findings

- `src-tauri/src/terminal/output_hub.rs` retains bounded session output and broadcasts live chunks.
- `ui/src/lib/terminalEvents.ts` retains 512 KiB per session and currently forwards every decoded chunk immediately.
- `ui/src/lib/terminalHostManager.ts` keeps created xterm/WebGL instances in a process-lifetime map until explicit destruction, and each instance retains a live output subscription.
- `ui/src/components/TerminalSplitView.tsx` renders only active tabs in visible tab groups, but switching previously visited tabs can revive retained renderer instances rather than applying an explicit resource budget.
- `ui/src/state/workspaceStore.ts` preserves parked workspace layouts in `worktreeLayouts`; an existing parked layout is the fast navigation seam.

## Delivery Topology

1. **Architecture audit DAG wave** — independent read-only lanes: lifecycle/cache seam; output batching/replay seam; workspace switching/state seam. A synthesis node validates their report artifacts and selects the narrow implementation boundary. This is a DAG because the synthesis depends on all three reports.
2. **Implementation DAG wave** — serialized integration to prevent shared TypeScript file conflicts:
   - renderer lifecycle/cache and its test owns `ui/src/lib/terminalHostManager.ts` and its tests;
   - output batching/replay and its test owns `ui/src/lib/terminalEvents.ts` and its tests;
   - workspace navigation tests and minimal state changes own `ui/src/state/workspaceStore.ts` and its tests;
   - integration node connects consumers only after prior nodes complete;
   - verification node depends on every producer and runs targeted tests, UI build, and native regression tests.
3. **Manual QA** — use the closest available Ferryx UI surface to switch a retained and an evicted terminal. If desktop automation is unavailable, record the exact manual test required rather than claiming a desktop pass.

## Required RED → GREEN Proofs

1. Renderer-budget test: currently fails because renderer instances remain subscribed and retained without an LRU cap; after implementation it proves visible panes plus the configured warm-cache limit remain instantiated and eviction preserves the backend session.
2. Output test: currently fails because every event writes directly to xterm; after implementation it proves ordered frame batching, bounded queue behavior, and inactive-session replay/activity continuity.
3. Navigation test: currently fails if known parked workspace selection calls spawn or waits; after implementation it proves state-only activation for five workspaces and 100 existing sessions.

## Verification

- Targeted Vitest files for renderer lifecycle, terminal events, and workspace navigation must pass after their own RED captures.
- `bun run --cwd ui build` must exit zero.
- Targeted Rust output-hub/session tests must pass to prove the mobile remote source stream remains compatible.
- A real Ferryx surface must demonstrate tab switching and evicted-terminal reattachment, with evidence and cleanup receipt; otherwise the needed user-performed desktop check is recorded exactly.

## Completion Record

- Implemented a visible renderer ref-count plus bounded inactive LRU warm cache; eviction disposes only xterm/WebGL frontend resources and preserves backend PTYs.
- Added a 128 KiB per-renderer animation-frame output scheduler with ordered synchronous overflow flush and teardown cancellation; the frontend event bus retains a 512 KiB reconnectable suffix with title/activity continuity.
- Preserved known workspace/tab state-only activation and added five-workspace by twenty-session regression proof with zero spawn, close, wait, event-start, or CWD calls.
- Closed asynchronous create/destroy races so a late renderer cannot resurrect after close or erase a newer pending create; generation bookkeeping is released once active creations settle.
- Refactored terminal lifecycle production and test responsibilities into focused modules, each below 250 nonblank/noncomment lines, without changing their external lifecycle API.
- Final automated evidence: targeted terminal suite 13 files / 58 tests; `bun run --cwd ui build`; full `bun run --cwd ui test` 70 files / 493 tests; `cargo test --manifest-path src-tauri/Cargo.toml output_hub` 2 tests; `git diff --check` clean.
- Desktop acceptance is intentionally user-run because agent desktop input automation is prohibited. The complete Scenario A-C procedure is `.omo/evidence/terminal-scale/MANUAL_DESKTOP_QA.md`.
