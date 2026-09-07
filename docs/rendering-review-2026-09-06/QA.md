# Rendering repair evidence checklist

Status: incomplete. This document is a checklist, not a passing verdict.

## C1: Source review

- Atlas/frame coherence: `atlas.md` must cite current
  `renderer/atlas.rs`, `row_cache.rs`, `renderer.rs`, `instances.rs` and their
  actual render callers. Diagnosis worker is running; no accepted verdict yet.
- Native scheduling/ownership/presentation: `surface.md` must cite
  `surface_host.rs`, `render_pass.rs`, `child_surface.rs`, lifecycle/composition
  guards and relevant platform implementations. Diagnosis worker is running.
- Frontend ownership/geometry/visibility: `frontend.md` must cite
  `NativeTerminalPane.tsx`, `TerminalSplitView.tsx`, native lifecycle/visibility
  helpers and direct callers. Diagnosis worker is running.
- Independent synthesis: `review.md` must check every reported finding against
  current source and existing guards/tests. This dependency has not completed.
- Every accepted defect needs exact source lines, a reachable triggering
  sequence, a literal regression command and a real-surface scenario before
  production edits. Source plausibility alone does not establish runtime cause.

## C2: Rendering fixes

- No production code has been changed by this run.
- Foreign uncommitted atlas and row-cache changes remain a read-only baseline.
  Their earlier tests do not supply this run's missing RED/GREEN evidence.
- Each accepted defect must have a right-reason failing test or scenario
  recorded before its fix, then the identical proof passing afterwards.
- Dense Korean/ASCII rendering must be captured on the actual debug app through
  repaint and 1x/2x display-density transitions.
- PASS requires readable before/after screenshots without wrong glyphs, missing
  glyphs or stale frames. Process presence, source tests and simulated frontend
  density events cannot substitute for these screenshots.
- The actual app/build identity must be recorded for the screenshots. Existing
  installed and debug processes are not owned by this run and must not be killed.

## C3: Adjacent regression

For each scenario below, refine concrete controls/coordinates from fresh debug
app state before action. Record the exact action log and screenshots:

- Split: two panes retain distinct content and their own backend sessions.
- Resize: both panes repaint at their current bounds without persistent blanks.
- Pane-to-tab move: the moved pane retains content and PTY; the old native
  surface does not remain in its former location.
- Tab switch: only the selected tab's panes are visible with correct content.
- Overlay open/close: native content yields to the overlay and returns only
  for the current visible owner.
- Density transition: a fixed logical layout updates native geometry and text
  at both densities; no wrong-character or stale-frame result.

None of these native screenshot scenarios has passed yet.

## Supporting checks

The independent baseline worker owns one execution of each command below.
Results will be recorded in `baseline.md`; pending runs are not passing evidence.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal
bun run --cwd ui test -- src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/lib/nativeTerminalLifecycle.test.ts
cargo check --manifest-path src-tauri/Cargo.toml
bun run --cwd ui build
```

Changed-code diagnostics, post-fix affected tests and final build evidence are
still required when implementation changes their inputs. Do not rerun unchanged
passing checks merely to populate this checklist.

## Native QA access evidence

These were read-only preflight calls. No desktop input or window activation was
performed.

```sh
orca status --json
orca computer capabilities --json
orca computer list-windows --app pid:60274 --json
```

Observed: `orca status` reported `app.running=false` and
`runtime.state=not_running`. The computer calls returned `runtime_unavailable`
because `/Users/indo/Library/Application Support/orca/orca-runtime.json` was
absent. No alternate Orca executable was selected.

Read-only native fallback:

```sh
osascript -e 'tell application "System Events" to tell (first application process whose unix id is 60274) to get {name, name of every window}'
```

Observed exit code 1:

```text
106:110: execution error: System Events got an error: osascript is not allowed assistive access. (-1728)
```

An earlier AppleScript collection query had a syntax/collection error; it is not
evidence about accessibility or rendering. The corrected query above establishes
the access limitation. Native QA remains unresolved, not successful.

## Cleanup and delivery

- The source-review and baseline workers may not stop foreign processes.
- Lead's atlas modification watcher `mon_XV5PZ1ERHS51FPGB` / `watch_1` delivered
  `timed_out`; no source-modification event was delivered before expiry.
- Goal-generation monitor `mon_RXPWAMP8JCV774C9` and previous-session retrieval
  monitor `mon_AMZZB12MKAF9C7YJ` completed with exit code 0.
- No debug app, PTY or browser context has been created by the lead.
- Baseline worker cleanup receipts remain pending.
- No implementation commit or integration has been made by this run.
- Final report, confirmed-defect closure, native screenshots, post-fix checks and
  any required integration approval remain pending.

Durable ledger:
`.omo/ulw-loop/rendering-review-20260906/ledger.jsonl`.

Current criterion artifacts:
`.omo/evidence/ulw/rendering-review-20260906/G001-review-and-resolve-ferryx-intermitte/a1`.
