# Agent Activity State Management — Orca-Style Rebuild (2026-08-27)

The spinner never spun and finished sessions were never marked as needing attention, while the unit
suite was fully green. Two independent defects caused that, in different layers. Both are fixed and
each fix carries a mutation proof showing its test can actually fail. Because the measured titles then
showed that title inference alone cannot cover every agent, Ferryx also ships an extension that lets
an agent report its own state.

## Result

| Gate | Command | Result |
| :--- | :--- | :--- |
| Frontend suite | `cd ui && npx vitest run` | 99 files / 858 tests passed, 0 failed, 0 unhandled errors |
| Typecheck | `cd ui && npx tsc --noEmit` | exit 0 |
| Frontend build | `bun run --cwd ui build` | exit 0 |
| Rust lib | `cargo test --manifest-path src-tauri/Cargo.toml --lib --features native-terminal` | 306 passed, 0 failed |
| Rust lib under load | same, with 12 saturating processes | 306 passed, 0 failed |
| Real surface | agent-browser against the rendered harness | 5/5 scenarios pass, screenshots captured |

## Why the green suite lied

The pre-existing tests fed only *classifiable* titles: a working title, then a done title. Real
terminals do not behave that way. An agent rewrites its title constantly with progress detail, and a
shell repaints it with the cwd. The old tests never exercised that interleaving, so they passed while
the shipped behavior was broken.

## Defect 1 — the store treated status as the newest title, not as state

`ui/src/state/workspaceStore.ts`, case `SESSION_TITLE_ACTIVITY`, deleted the session's activity entry
whenever `classifyTerminalTitleActivity()` returned null:

```ts
if (!classified) {
  // ...
  delete activityBySessionId[action.sessionId];
```

That classifier returns null for any title without a status word. So `codex: src/lib/activity.ts`
during active work, or a shell prompt repainting `~/code/project/orca-lite`, **erased** an in-flight
`working` state. The spinner appeared and vanished on the next title write.

Captured RED, before any production change
(`ui/src/state/activityStatePersistence.test.ts`):

```
FAIL keeps the working state when the agent writes a title carrying no status word
  expected "working", received undefined
FAIL keeps the working state when a shell prompt repaints the title
  expected "working", received undefined
```

`received undefined` is the tell: the entry was removed, not mis-stated.

**Fix.** Status is now a persisted per-session state machine, which is the Orca model the request
asked for. Deletion is gated on a new `isBareAgentTitle()` in `ui/src/lib/agentTitle.ts` — true only
for a title that is exactly a known agent name with no task text and no spinner, which is the one
explicit idle signal. Any other non-classifying title carries the previous state forward:

```ts
state: classified ?? previous!.state,
```

A session with no prior activity and a non-classifying title still records nothing, and the existing
referential-stability short-circuit is preserved so `memo` on `SortableTab` keeps working.

## Defect 2 — Rust swallowed titles present in replayed history

`src-tauri/src/native_terminal/surface_host.rs` called `terminal.feed()` at four sites but
`take_native_terminal_events()` at only two. Lines 361 and 372, both inside
`attach_daemon_attachment`, fed the replayed daemon history and never drained the events. Ghostty
parsed the OSC title in that history and set `title_updated`, and nothing read the flag — so a
session whose agent had already set its title before the webview attached showed no state at all
after app start, session restore, or re-attach.

**Fix.** Both attach branches now fall through to a single drain, and emission happens after the
sessions mutex guard is released, matching the pump's existing discipline:

```rust
let events = take_native_terminal_events(session, session_id);
(update_sender, render_coordinator, events)
};

for event in events {
    emit_native_terminal_event(app.as_ref(), &self.event_sink, event);
}
```

`attach_replayed_history_emits_title_for_new_and_existing_sessions` asserts both branches emit, using
`set_event_sink()` as the observation point so no Tauri `AppHandle` is needed.

## What was already working

Two layers were cleared by execution, not assumption, which is what localized the defects:

- **Ghostty FFI and the title callback.** `native_terminal_ffi_probe_osc_2_reports_title_change_and_value`
  prints `title_changed=true title="some-agent-title"`. The FFI was never at fault.
- **store → selector → TabBar → SortableTab → StatusDot.** `ui/src/state/activityRenderChain.test.tsx`
  drives the real store and real components with nothing mocked, and passes.

## Mutation proofs

A passing test is only evidence if it can fail. Each fix was re-broken and the asserting test
observed failing, then restored and confirmed byte-identical.

| Proof | Mutation | Observed failure |
| :--- | :--- | :--- |
| MUT-L | delete the attach-path emission loop in `surface_host.rs` | `left: []` vs the two expected `NativeTerminalTitlePayload` values |
| MUT-M | replace `classified ?? previous!.state` with `classified!` | `activityStatePersistence.test.ts` — 2 failed, 1 passed |

## Real-surface evidence

The spinner and the attention marker are visual, so they were verified in a browser, not asserted
from a store dump. `ui/src/devtools/ActivitySurfaceHarness.tsx` (served at
`/activity-qa.html`) mounts the **real** `TabBar`, the **real** `WorktreeList`, and the **real**
`workspaceReducer` driven by the **real** `SESSION_TITLE_ACTIVITY` payload, and `agent-browser`
drives it. No desktop automation and no OS input injection were used.

Probed DOM for the background tab (`tab-bg`, deliberately not the active tab):

| Scenario | Rendered result |
| :--- | :--- |
| working | `data-status-state="working"`, class contains `animate-spin`, `tab-working-indicator`; worktree row `working` |
| non-status title after working | still `working` — the defect-1 erasure is gone |
| shell prompt repaint | still `working` — the defect-1 erasure is gone |
| done | `unread`, `tab-unread-dot`, `bg-primary`; worktree row `unread` |
| needs input | `waiting`, `tab-waiting-indicator`, `bg-status-warning`; worktree row `waiting` |

Screenshots: `docs/evidence/agent-activity/surface-green/GREEN-*.png` (5 files, 1440x900 PNG).
`GREEN-working-background.png` was opened and visually confirmed to show the spinner and the OMO
agent icon on the background tab.

There is deliberately **no** browser RED artifact. Reverting the fixed modules makes the harness fail
to compile, so the page renders nothing — and a blank page is not evidence that an indicator is
missing. Those misleading captures were deleted. The honest failing-first proof for defect 1 is the
reducer-level RED above, reinforced by MUT-M.

## Incidental fix, called out separately

`ui/src/components/SettingsDialog.tsx` used `<Download />` with no matching import, breaking `tsc`
and `bun run build` and failing 8 `SettingsDialog` tests. This was **pre-existing and unrelated** to
agent activity: verified by running `tsc` in a pristine `git worktree` at HEAD, where the identical
`TS2552: Cannot find name 'Download'` appears. Since it blocked the build gate, the missing import was
added. That is the entire change to that file.

## What the agents actually emit (measured, not assumed)

The whole design infers status from OSC titles, so the titles were captured from a real pty
(`pty.fork`, `TERM=xterm-256color`) and fed through the shipped classifier:

| Agent | Captured title | Classifier verdict |
| :--- | :--- | :--- |
| omo | `ESC]0;OmO - orca-lite BEL` | null, `isAgent=true`, type `omo` |
| codex | `ESC]0;orca-lite BEL` | null, `isAgent=false` |
| codex | `ESC]0;⠸ orca-lite BEL` | **working** (braille spinner), type `generic` |
| opencode | `ESC]0;OpenCode BEL` | null, `isAgent=true`, type `opencode` |
| gemini | `ESC]0;◇  Ready (orca-lite) BEL` | **done** (leading-diamond glyph rule) |

`claude` produced zero bytes under a bare pty, so it is inconclusive rather than proven silent.
No `~/.zshrc` or starship title configuration competes with these titles.

This is what made the bug so total. Most real titles carry **no status word**, and under the old
reducer every one of them erased the activity entry. Codex does emit a spinner title that classifies
as working — and then its very next plain `orca-lite` title deleted that state immediately. So even
the one agent signalling correctly produced a spinner that never survived to be seen.

After the fix, codex's working state survives the following plain title, which is the behavior
captured on the real surface above.

**The limitation that forced a second signal.** For omo and opencode, a bare agent name is the only
title they emit. Ferryx can show the agent icon, but no title-only design can infer
working-versus-done from that. Gemini's `◇ Ready` classifies as done via the leading-glyph rule,
which happens to be reasonable but is glyph luck rather than a contract. This is why status is no
longer inferred from titles alone — see the next section.

## The agent reports its own state

Titles are a guess. An agent that runs a lifecycle extension can state its status directly, so Ferryx
ships one and installs it into every agent directory that exists on the host.

`src-tauri/resources/agent-extensions/ferryx-agent-state.ts` subscribes to the agent's lifecycle
events and writes one JSON line per transition — `{"type":"agentState","sessionId","state","agent"}`
— to a Unix socket. States are exactly `working`, `blocked`, and `idle`.

The wiring, end to end:

| Step | Location |
| :--- | :--- |
| Install into `~/.omo`, `~/.pi`, `~/.omp` at daemon start | `src-tauri/src/daemon/agent_extension.rs` |
| Pass session identity + socket path to the child | `src-tauri/src/terminal/pty.rs` (`FERRYX_SESSION_ID`, `FERRYX_AGENT_STATE_SOCKET`) |
| Accept reports on `/tmp/rorca-{uid}/agent-state.sock` | `spawn_agent_state_listener` in `daemon/server.rs` |
| Fan out to the session's stream as `AgentState` | `pump_sequenced_stream_with_agent_state` |
| Emit to the webview, suppressing screen inference | `native_terminal/surface_host.rs` |

Two properties matter. The env vars are injected where the session id first exists and the child is
not yet spawned, so a report can always be attributed to the right pane. And once a session reports
its own state, `agent_reports_own_state` permanently disables screen-scraping for it — the agent's
self-report is authoritative and a working-looking screen cannot contradict it.

Installation is idempotent: an unchanged file is never rewritten, a stale one is replaced atomically
(temp + rename), and a missing agent directory is skipped rather than created.

`extension_reported_state_wins_over_screen_inference` covers the precedence rule, and deleting the
guard makes it fail with a second, contradicting state (`left: 2, right: 1`).

## Test nondeterminism found while verifying this

Four `ipc::tests` awaits bounded real-shell output at 5 seconds. They passed normally and failed
whenever the machine was busy — a cold login shell can take longer than that to reach its first echo.
The awaits themselves are correct (they wait on the expected output, not on a sleep), so only the
budget changed: a named `REAL_SHELL_OUTPUT_DEADLINE` of 30s. Verified green with 12 saturating
processes running, which previously reproduced the failure.

## Not verified here

The harness proves the shipped render path and the state machine. It does not prove the end-to-end
desktop path — real agent process to daemon PTY to ghostty to Tauri emit to webview — which needs the
packaged app and a human at the keyboard. Manual steps live in
`docs/AGENT_ACTIVITY_NOTIFICATION_MANUAL_E2E.md`, section I.

One open question remains: whether the daemon stream pump stays attached for parked worktree layouts
in a non-active project. If it detaches, cross-project background notifications still will not fire,
and that is a separate Rust-side fix. Checklist item I3 is the test for it.

The extension path is proven by unit tests at each hop, not end to end with a live agent process. It
also only covers agents exposing this lifecycle API; anything else still relies on title and screen
inference.

## Three defects found by verification, not by reading

Verification of this design found three real defects. Each was captured failing first, then fixed,
then re-broken by mutation to prove the test is load-bearing.

1. **`TERM=dumb`** (`src-tauri/src/terminal/pty.rs`). Nothing set `TERM`, so PTYs inherited `dumb`
   from the GUI-launched daemon. Agent TUIs then run non-interactive and the extension's
   `mode === "tui"` gate returns early, so no state was ever reported. This was the blocker behind
   "none of the agent features work". Covered by
   `terminal::tests::spawned_pty_advertises_a_real_terminal_type`, which drives a real `/bin/sh` in a
   real PTY and reads `$TERM` off the stream.

2. **Stale restored state** (`ui/src/lib/sessionPersistence.ts`). Restore copied `activity.state`
   verbatim, so a persisted `working` claim survived a restart and a tab showed a spinner for an
   agent that no longer existed. `working`/`waiting` are claims about a live process, so on restore
   they settle to `done`; `agentType`/`isAgent` are preserved so the tab keeps its brand icon.

3. **Nameless notifications** (`ui/src/state/workspaceStore.ts`). `selectActivityNotificationTargets`
   derived `agentLabel` only from the terminal title. An extension-reported state carries `agentType`
   but omo's title is a bare name, so a completed omo turn would notify with no agent name. The
   selector now falls back to `agentDisplayNameForType(agentType)`.

The daemon transport was **not** a defect. `NativeTerminalPane` -> `cmd_native_terminal_attach` ->
`daemon_client.attach()` reaches the single production caller of
`pump_sequenced_stream_with_agent_state`, which is agent-state-wired. An earlier suspicion based on a
low `endSequence` was invalid: that value is a chunk counter over a bounded ring buffer.

The socket-to-stream hop had zero coverage and now has two tests plus mutation proofs. The
extension-report-to-render path also had zero coverage and now has one, asserting the spinner and
then the attention dot on a **non-active** tab.
