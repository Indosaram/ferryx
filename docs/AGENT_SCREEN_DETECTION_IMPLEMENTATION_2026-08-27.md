# Screen-rule agent detection: implementation record

Implemented 2026-08-27. Interface spec: `AGENT_SCREEN_DETECTION_CONTRACT.md` (written first, then
built against). Supersedes the title-only activity model described in
`AGENT_ACTIVITY_STATE_MODEL_2026-08-27.md`, which remains accurate about the two defects it fixed.

Agent state in Ferryx is now derived from the live terminal screen, with the OSC title demoted to a
lowest-priority fallback. This is the architecture herdr 0.8.2 ships, validated by reading its 128
embedded rules across 20 agent manifests.

## Why the previous model could not work

Measured from a real pty, then fed through the shipped title classifier:

| Agent | Real title | Title-only verdict |
| --- | --- | --- |
| omo | `OmO - orca-lite` | none |
| codex | `orca-lite` | none |
| codex | `⠸ orca-lite` | working |
| opencode | `OpenCode` | none |
| gemini | `◇  Ready (orca-lite)` | done (glyph luck) |

Four of five carry no status word. The status is on the SCREEN (`esc to interrupt`,
`Do you want to proceed?`), which no title can express. Only codex worked, and only while its
spinner frame happened to be in the title.

RED baseline captured before implementing (5/5 assertions passed, confirming the gap): the shipped
classifier returns `null` for `esc to interrupt` and for `Do you want to proceed?` — the exact
strings that carry the state.

## What was built

### Rust engine — `src-tauri/src/agent_detect/` (924 LOC + manifests)

| File | LOC | Role |
| --- | --- | --- |
| `mod.rs` | 433 | public re-exports, contract test suite |
| `matcher.rs` | 137 | matcher tree: `contains`/`regex`/`line_regex`/`any`/`all`/`not` |
| `manifest.rs` | 132 | TOML schema, compile + validate |
| `engine.rs` | 113 | priority resolution, hold semantics, embedded manifests |
| `region.rs` | 113 | region parsing and resolution |

Not feature-gated: it consumes plain text, so it compiles and tests without
`--features native-terminal`. Manifests are embedded with `include_str!`, so there is no runtime
file IO and nothing to package.

Regions implemented: `whole_recent`, `osc_title`, `bottom_non_empty_lines(N)`,
`top_non_empty_lines(N)`. Deliberately omitted: `after_last_prompt_marker`,
`after_last_horizontal_rule`, `prompt_box_body`, `osc_progress` — they need OSC 133 semantic prompt
tracking Ferryx lacks. Only 2 of herdr's 128 rules depend on prompt markers; affected rules were
retargeted to `whole_recent` or dropped.

Resolution: highest `priority` wins, ties broken by declaration order. Then the rule that matters
most — **hold**. If the winner sets `skip_state_update`, or its state is `unknown`, or NO rule
matches, the previous state is returned unchanged. Absence of evidence is never evidence of idle.
This is precisely the discipline the old reducer lacked when it deleted state on an unclassifiable
title.

### Manifests — 5 agents, 23 rules

omo (4), opencode (3), codex (6), claude (7), gemini (3). Adapted from herdr's, with three forced
changes: Rust's `regex` crate has no lookahead, so backtracking patterns were rewritten or expressed
as `not` nodes; rules using unimplemented regions were retargeted or dropped; and glyph literals
lost in the `strings` extraction were replaced with unicode escapes (`\x{2800}-\x{28FF}`) rather
than copied broken.

### Deliberate deviation: global manifest evaluation

herdr tracks each pane's foreground process and evaluates only that agent's manifest. Ferryx cannot:
agents are launched by typing `codex` into a shell, so the session command is `zsh` and codex's title
carries no agent name. Ferryx therefore evaluates ALL manifests and takes the global
highest-priority match.

This is sound because rules identify a STATE, not an identity: `esc to interrupt` on screen means
something is working regardless of which agent drew it. Agent attribution for the tab icon still
comes from the title parser. The cost is a false-positive risk, which is why rules avoid
single-common-word matches — and why the probes below specifically test ordinary shell output.

### Driver — `surface_host.rs`

Detection runs inside the existing `take_native_terminal_events` drain, which is already called from
every `feed` site. It builds `ScreenInput` from `render_snapshot()` + `row_text(row)` plus the
current title, and emits `native_terminal_agent_state` through the existing
`emit_native_terminal_event` (preserving the `set_event_sink` test seam). Emission is
edge-triggered on `session.last_agent_activity`, so an unchanged state emits nothing. No polling
loop, no timer, no new PTY read path.

### Store — `workspaceStore.ts`

`SESSION_SCREEN_ACTIVITY` maps working→working, blocked→waiting, idle→done. Two guards carry the
correctness:

- `idle` is ignored unless the previous state was `working` or `waiting`. Without this, every freshly
  attached shell would instantly mark its own tab unread.
- Once a session has `source: "screen"`, `SESSION_TITLE_ACTIVITY` may still refresh
  `title`/`isAgent`/`agentType` but can NEVER change `state`. This is the structural equivalent of
  herdr placing `osc_title_idle` at priority 50.

Unread tab + worktree rollup reuses the existing `applySessionActivity`, so no attention logic was
duplicated.

## Verification

Every result below was produced by my own run, not accepted from a subagent report.

| Gate | Result |
| --- | --- |
| `cargo test --lib --features native-terminal` | **300 passed, 0 failed** (285 baseline + 15 new) |
| `cargo clippy --lib --features native-terminal` | exit 0 |
| `cd ui && npx vitest run` | **99 files, 851 tests, 0 failed** |
| `cd ui && npx tsc --noEmit` | exit 0 |
| `bun run --cwd ui build` | exit 0 |

`--lib` is mandatory on the cargo command; without it the filter yields a false green.

### Mutation proofs

A mechanism with no mutation proof is treated as unverified.

**MUT-N — hold-on-no-match** (`engine.rs`): made the no-match branch return `None` instead of
holding. Result: 2 failures, including `probe_working_then_unclassifiable_screen_holds_working`
(`left: None, right: Some(Working)`). Restored byte-identical, re-ran: 14 passed.

**MUT-O — title demotion** (`workspaceStore.ts`): let `SESSION_TITLE_ACTIVITY` overwrite `state`
even when `source === "screen"`. Result: test 5 failed, `expected 'done' to be 'working'`. Restored
byte-identical, re-ran: 6 passed.

**MUT-P — idle guard** (`workspaceStore.ts`): removed the previous-state gate on `idle`. Result:
test 4 failed, an entry `{state: "done", source: "screen"}` was created where `undefined` was
required.

### Independent probes — `src-tauri/src/agent_detect/independent_probe.rs`

Six probes I wrote separately from the implementation, two designed specifically to catch
over-generic rules under global evaluation:

- bare measured titles alone (`OmO - orca-lite`, `OpenCode`, `orca-lite`) fabricate no state
- a working screen reaches `Working` for all three bare-title agents
- a permission screen reaches `Blocked`
- **plain `ls -la` output produces no state** (false-positive trap)
- **a git log whose subject quotes "do you want to proceed" produces no state** (false-positive trap)
- working followed by unclassifiable build noise HOLDS working

All 6 pass. The two traps are the ones that would have exposed careless rules; they did not fire.

### Real-surface capture

`ui/src/devtools/ActivitySurfaceHarness.tsx` served at `/activity-qa.html` mounts the REAL `TabBar`,
REAL `WorktreeList` and REAL `workspaceReducer`, driven by `agent-browser` (no desktop automation).
Evidence: `docs/evidence/agent-activity/screen-rules/`.

All 9 scenarios green on the BACKGROUND tab:

| Scenario | tab-bg | worktree |
| --- | --- | --- |
| screen-working | `working`, spinning | `working` |
| screen-title-cannot-override | `working`, spinning, +icon | `working` |
| screen-blocked | `waiting` | `waiting` |
| screen-idle-attention | `unread` | `unread` |

`screen-title-cannot-override` is the decisive one: a bare `OmO - orca-lite` title arrives AFTER
screen-working and the tab stays working while gaining the agent icon. Screenshot visually confirmed.

One measurement bug found and fixed during capture: the probe read `String(el.className)`, but
`animate-spin` sits on an SVG where `className` is an `SVGAnimatedString`, so every spinner reported
`spinning:false` — including the known-good title path. Reading the `class` ATTRIBUTE fixed it. The
first capture run was discarded rather than reported.

## Defect found in real-app use: detach killed detection

Reported after the first real run: `blocked` never appeared, and a background tab's spinner spun
forever until that tab was visited.

`detach_session` REMOVED the session and aborted `stream_task` + `pump_task`. React unmounts an
off-screen pane, the UI calls `cmd_native_terminal_detach`, the pump dies, no further `feed` happens
and detection stops. State froze at its last observed value — hence a spinner stuck at `working` and
a blocker that never arrived. Title and bell for background panes had the same hole.

Fix: `detach_session` releases only the GPU surface (drops the `hosts` entry, clears
`focused`/`layout`/`logical_bounds`) and keeps the session and its pump alive. A new `close_session`
performs the destructive teardown, exposed as `cmd_native_terminal_close` and invoked from
`closeTerminal()` before `cmd_terminal_close`.

Safe on both edges, verified in code: re-attach already aborts the previous stream/pump before
installing new ones, so returning to a tab replaces rather than duplicates; and the pump breaks on
`DaemonStreamMessage::Exit`, so a dead terminal reclaims itself.

Rejected alternative: adding a parameter to `cmd_terminal_close` for the cleanup — it breaks six
existing `ipc::tests` callers for no benefit.

RED captured first: `detached_session_still_reports_agent_state_transitions` failed with
`left: ["working"]` against `right: ["working", "blocked"]`. **MUT-Q** (make detach delegate to
`close_session` again) reproduces that failure; restored byte-identical.

Gates after the fix: cargo **302 passed**, clippy 0 errors, vitest **852 passed**, tsc exit 0,
build exit 0.

A second hypothesis was investigated and disproved: that omo's permission prompt wording
(`Allow once` / `Allow always` / `Deny with feedback`, read from its shipped
`permission-system/prompt.js`) matched no rule. It does match — via gemini's `apply_or_allow_change`
under global evaluation, confirmed by printing the winning rule. Cross-agent coverage is the
global-evaluation design working as intended. A probe for those exact labels is now pinned.

## Honest limitations

**Viewport only.** `RenderSnapshot` has no scrollback, so `whole_recent` is approximated by the
visible viewport. Bottom-anchored rules are unaffected; a blocker that scrolled off screen is missed.

**Rule coverage is a starting point, not herdr parity.** 23 rules against herdr's 128. herdr's
manifests are accumulated field experience — one comment pins the exact Claude Code version
(2.1.228) where the spinner glyph changed. Ferryx has to build that history.

**Global evaluation trades precision for coverage.** It is what makes omo and opencode work at all
without agent cooperation, but a sufficiently generic future rule could fire on unrelated output.
The two false-positive probes are the guardrail; keep adding them when authoring rules.

**No diagnostic command.** herdr ships `agent agent explain --json` to show why a state was chosen.
`Detection` already carries `rule_id`/`manifest_id` and the event ships them, so the data is there,
but nothing surfaces it yet. This is the single highest-value follow-up: it is what turns the next
debugging session from guesswork into a lookup.

**Desktop E2E is unverified by me.** Everything above is unit, integration and real-browser
rendering evidence. Confirming it in the running app requires a desktop session I do not drive.
Manual steps: launch via `open src-tauri/target/debug/Ferryx.app` (launching through
`cargo tauri dev` from a terminal makes macOS attribute notifications to that terminal), run an
agent in a background tab, and watch the tab and worktree row.
