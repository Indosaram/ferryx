# Agent state management — what is done, and what is not

Measured 2026-08-28 against the working tree (quiescent, all gates green:
`cargo test --lib --features native-terminal` 326 passed, `bun run --cwd ui test`
111 files / 987 tests, `tsc --noEmit` exit 0).

This document exists because "is agent state management 100% done?" has a split answer:
**the machinery is done; the per-agent detection data is not.** Nothing below is a
proposal — it is the recorded state of the gap.

---

## 1. Complete and verified: the state machinery

| Piece | Where | Status |
| --- | --- | --- |
| Persisted per-session state machine | `workspaceStore.ts` `activityBySessionId` | done |
| Settle on PTY exit (forces `done`) | `workspaceStore.ts:1589-1595` | done |
| Settle on screen `idle` (only from working/waiting) | `workspaceStore.ts:1614-1626` | done |
| In-flight guard (a bare title cannot erase a live state) | `SESSION_TITLE_ACTIVITY` | done |
| Native terminal channel always compiled in | `Cargo.toml` `default = ["native-terminal"]` | done |
| Brand icons + terminal fallback | `agentIcon.ts` (12 SVGs, `?? null`) | done |
| Remote/mobile propagation | `remote/protocol.rs` `activity_state`, `RemoteSessionList.tsx` | done |
| Notification on `working -> waiting\|done` only | `notificationCoordinator.ts:120-123` | done |
| Tests | 26 state files / 216 tests, 15 `agent_detect` tests | passing |

There is no known path where a pane can spin forever: PTY exit unconditionally forces
`done`, independent of any detection.

---

## 2. The gap: detection exists as a mechanism, but rules exist for only 5 agents

Three signal layers feed the state machine. They differ in *which agents they apply to*:

| Layer | Mechanism | Agents actually covered |
| --- | --- | --- |
| 1. Explicit protocol | `ferryx-agent-state.ts` installed into the agent's extensions dir; reports over `agent-state.sock` (injected into the PTY env in `terminal/pty.rs:116`, broadcast by `daemon/server.rs`) | **omo, pi, omp** (dirs `~/.omo`, `~/.pi`, `~/.omp`) |
| 2. Screen rules | TOML manifests in `src-tauri/src/agent_detect/manifests/`, matched against recent screen content | **claude, codex, gemini, omo, opencode** |
| 3. Title heuristics | OSC title parsing, lowest priority | nominally ~25 names, but see below |

**Layer 3 is close to useless on its own.** The project's own measurement (memory:
`ferryx-agent-activity-state-model.md`) found most agent titles carry no status word at
all — omo emits `OmO - orca-lite`, opencode emits `OpenCode`. Only codex (braille spinner)
and gemini (leading-diamond `Ready`) yield a verdict, and gemini's is glyph luck.

### Consequence: 6 agents have an icon but effectively no state

These ship a brand icon and are identified correctly, but have **no rule manifest and no
extension support**, so their tab will generally show no working/waiting indicator:

- antigravity
- cline
- copilot
- cursor
- grok
- kimi

This is the honest answer to "screen rules don't cover it": the screen-rule *engine* works
and is tested — there is simply **no rule file for these six**, so the engine never has
anything to match for them.

### Rule depth is thin even where it exists

Total: **5 manifests, 23 rules.**

| manifest | rules | states covered |
| --- | --- | --- |
| claude | 7 | working / blocked / idle / unknown |
| codex | 6 | working / blocked / idle |
| omo | 4 | working / blocked / idle |
| gemini | 3 | working / blocked / idle |
| opencode | 3 | working / blocked / idle |

gemini and opencode at 3 rules each are the thinnest and are the most likely to miss
states in practice.

---

## 3. Correction: the "128 rules / 20 manifests" figure is herdr's, not Ferryx's

An earlier report in this session cited screen detection as having "128 rules / 20
manifests". That number is real but belongs to **herdr** (`~/.local/bin/herdr`), the
external tool whose declarative screen-rule design Ferryx adopted. It is not Ferryx's own
coverage. Ferryx ships **5 manifests / 23 rules**.

Read the right way, this is the useful benchmark rather than an error to file away:

| | agent manifests | rules |
| --- | --- | --- |
| herdr (reference design) | 20 | 128 |
| Ferryx (today) | 5 | 23 |

So Ferryx has implemented the engine faithfully but roughly a quarter of the reference
rule corpus. That ratio, not any bug, is what "agent state management is not 100%" means.

---

## 4. How to close the gap (not done — this is the recipe)

For each uncovered agent, add `src-tauri/src/agent_detect/manifests/<agent>.toml`. The
format, taken from the shipped `gemini.toml`:

```toml
id = "gemini"
version = "2026.08.27.1"
min_engine_version = 1
aliases = ["gemini-cli"]

[[rules]]
id = "apply_or_allow_change"
state = "blocked"          # blocked -> UI "waiting"
priority = 300
region = "whole_recent"    # or "osc_title"
visible_blocker = true
any = [
  { contains = ["Apply this change"] },
  { line_regex = ['(?i)^\s*.*(yes|allow)'] },
]

[[rules]]
id = "esc_cancel_working"
state = "working"
priority = 100
region = "whole_recent"
visible_working = true
contains = ["esc to cancel"]
```

Practical notes:
- `state = "blocked"` maps to the UI's `waiting`; `idle` maps to `done`, but **only** from
  working/waiting (idle is never an entry state).
- Higher `priority` wins.
- Writing these requires capturing each CLI's real screen output (its working spinner, its
  confirmation prompt, its ready state) — it cannot be done from documentation alone, which
  is why it is deferred rather than guessed.

## 5. Also open (unrelated to detection)

- kimi `--session <id>` is create-or-resume, so a stale id silently starts a new session
  (`docs/evidence/agent-session-restore/resume-negative-controls.txt`).
- cursor and gemini resume round-trips remain credential-blocked and unproven.
- opencode and gemini session-id discovery is impossible by construction (single global DB /
  index-based resume).
