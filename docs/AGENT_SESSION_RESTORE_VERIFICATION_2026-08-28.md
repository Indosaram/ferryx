# Agent Session Restore — verification wave (session 01a04339 continuation)

Continues session `01a04339-8665-7cf7-864f-2b0dd9f6678c`, which died at
`Context remains above the compaction threshold because compaction did not complete`
mid-verification: GATE 1 had just reported 19 failed test files and the session was
deciding whether those were regressions or pre-existing. This document closes that
verification wave.

Design doc: [`AGENT_SESSION_RESTORE_2026-08-27.md`](./AGENT_SESSION_RESTORE_2026-08-27.md)
Evidence: [`evidence/agent-session-restore/`](./evidence/agent-session-restore/)

## 1. What the dead session had actually shipped

| Artifact | Purpose |
| --- | --- |
| `ui/src/lib/agentResume.ts` | Table-driven per-agent resume argv (`AGENT_RESUME_SPECS`) |
| `ui/src/lib/agentSessionDiscovery.ts` | Reads the agent-generated session id off disk |
| `ui/src/lib/agentResumeAffordance.ts` | Offers resume for exited panes; never auto-spawns |
| `ui/src/lib/types.ts` | `agentType` / `agentSessionId` as real fields |
| `ui/src/lib/sessionPersistence.ts` | Persists agent identity across restart |

Resume capability as shipped: 5 `uuid` (claude, codex, copilot, kimi, opencode),
1 `positional-index` (gemini), 1 `chat-id` (cursor), 1 `uuid` (omo), 4 `none`
(grok, antigravity, cline, pi).

### Node claims re-verified against evidence

| Claim | Method | Result |
| --- | --- | --- |
| Never mints a session id | grep `--session-id`/`randomUUID`/`Uuid::new_v4` across all 4 modules | CLEAN |
| No `as any` / `declare module` | grep across the 3 new modules | CLEAN |
| Discovery regexes match real on-disk paths | 6 real ids (claude/codex/copilot/kimi) | PASS (prior session) |
| Real CLI accepts the id | real vs bogus id contrast, both claude and copilot | PASS, see `real-cli-resume.txt` |
| Process-keyed discovery | 2 panes, same cwd + same agent, real `ps` table | PASS, distinct pids at depth 3 |

The hard user constraint — **Ferryx never mints or injects a session id** — holds:
restore only ever reads back the id the agent generated itself.

## 2. Why the gate looked catastrophic: two abandoned tracks in one tree

The 19 (then 24) failures were **not** caused by the session-restore work. The working
tree contained a second, concurrent, uncommitted track (embedded-browser "parity":
profiles, zoom, load errors, tab duplication, browser shortcuts, history-navigation
redesign). Its agent was still writing during this session — `TerminalSplitView.tsx`
(23:53:15) and `SettingsDialog.tsx` (23:52:36) changed while test suites were running —
and it then **died mid-write at ~23:53**, leaving four files syntactically broken.

Those syntax errors cascaded: any test importing `App.tsx` or `TabBar.tsx` failed to
collect, which is why collected test count *dropped* from 876 to 803 as things got worse.

### Half-applied edits repaired (mechanical, unambiguous)

| File | Damage | Repair |
| --- | --- | --- |
| `ui/src/App.tsx:1233` | 4 statements injected between `useMemo(` and its `() => ({` | hoisted above the `useMemo` |
| `ui/src/components/TabBar.tsx:284` | `onDuplicateBrowser` JSX prop injected inside the `onTogglePin` handler body | restored handler, prop made a sibling |
| `ui/src/components/TabBar.tsx:399` | `BrowserDuplicateControl` entry injected inside a `<button>`'s attribute list | moved out as the menu's first entry |
| `src-tauri/src/browser/manager.rs:265` | `cancel_history_navigation` lost its closing brace | brace restored |
| `src-tauri/src/ipc/browser.rs:962,973` | two orphaned trailing args after `update_webview_state(...)` | placed as the real 7th `error` param |
| `ui/src/state/browserSessionHydration.ts:47` | `content.browser` optional passed to a non-optional param | narrowed with a guard |

## 3. The one genuine bug in this track

`workspaceStore.ts` `SESSION_TITLE_ACTIVITY` deleted the whole activity entry whenever a
title was unclassified, non-agent and title-sourced. That **erased an in-flight `working`
state** and partially re-introduced documented root cause 1 ("the spinner never spins").

Deletion is now additionally gated on the run having settled:

```ts
const inFlight = previous?.state === "working" || previous?.state === "waiting";
if (!classified && !parsed?.isAgent && !isScreenSource && !inFlight) { /* delete */ }
```

Justified by measured PTY capture, not by preference: **codex emits a bare project-name
title (`ESC]0;orca-lite BEL`) while working.** A cwd-shaped title is therefore not
evidence that the agent stopped, so it must not clear status. The stale brand is still
dropped. Deletion remains gated on `isBareAgentTitle()`, matching the documented model.

### A contradiction between two tests, resolved

Two tests written in the same thread asserted opposite things about the identical
scenario (title-derived `working`, then a non-agent shell title):

- `activityStatePersistence.test.ts:71` — `state` must stay `"working"`
- `screenActivity.test.ts:168` — the whole entry must be `undefined`

Both cannot hold. The authoritative state model (measured per-agent title output) backs
preservation, so `screenActivity.test.ts:168` was wrong. Its *intent* ("clears a stale
agent identity") is real and still holds — only its whole-entry assertion was replaced,
to match its own sibling test at :178:

```ts
expect(state.activityBySessionId?.["session-a"]).toMatchObject({
  state: "working", source: "title", isAgent: false,
});
expect(state.activityBySessionId?.["session-a"]?.agentType).toBeUndefined();
```

## 4. Gate results

| Gate | Command | Result |
| --- | --- | --- |
| Typecheck | `cd ui && bunx tsc --noEmit -p tsconfig.json` | **exit 0** |
| Rust lib compile | `cargo check --lib --features native-terminal` | **exit 0** |
| Session-restore + activity scope | `vitest run src/lib/agent*.test.* src/lib/sessionPersistence.test.ts src/state` | **28 files / 280 tests passed** |
| Full UI suite | `bun run --cwd ui test` | 10 files failing, **all browser-parity** (see below) |
| Rust lib tests | `cargo test --lib --features native-terminal` | test target does not compile, **all 9 errors browser-parity** |

Failure count across the repair sequence: 24 files → 11 → 10, with collected tests
recovering 803 → 939 as the cascade cleared.

## 5. Outstanding — abandoned browser-parity track (not session-restore)

Delegated to a subagent; needs its original author's intent in places.

Rust (blocks the whole lib test target):
- `CreateBrowserRequest` gained `browser_id` + `zoom_factor`; 8 struct literals omit them
  (`browser/tests.rs` ×7, `ipc/browser_cli.rs` ×1).
- `begin_history_navigation` gained `forward: bool` **and was behaviorally redesigned** —
  it now only marks loading when a history entry exists in that direction, so
  `test_history_navigation_marks_loading_without_overwriting_url` cannot pass on a
  single-entry history. Its setup needs a real 2-entry history.

UI (10 files):
- `BROWSER_SHORTCUT_EVENT` missing from explicit `vi.mock` factories (largest cluster).
- New built-in `private` profile; new `profileId` argument on `onNewBrowser`/`onAddBrowser`.
- Shortcut id list 34 → 39 (new `browser.*` actions).
- `SettingsDialog` finds multiple `⌘[` elements — **possible real UX conflict**: two
  actions may bind the same chord. Flagged for a product decision, not silently hidden.

## 6. Recurrence audit — two live defects found after the gates were green

The gates above were all green, and the implementation was still wrong in two places.
Unit tests asserted `buildResumeArgv()` equalled a hardcoded array, which proves
self-consistency but **not that the flag exists**. Probing each CLI's own `--help`
(cheap, non-destructive) found two argv shapes that would fail silently at runtime:

1. **omo was `--resume <id>` — wrong.** `--resume, -r` carries no value placeholder
   (it opens an interactive picker), unlike `--session <path|id>`. The id would have
   been parsed as a positional **prompt**, starting a brand-new session. Now
   `--session <id>`. The adjacent `--session-id <id>` is documented as *"creating it
   if missing"* — the forbidden minting flag — so a regression test now asserts the
   omo argv never contains `--session-id`.
2. **opencode was `run -s <id>` — wrong.** `run [message..]` is the non-interactive
   path; in a pane it would run headless and exit rather than resume the TUI. The TUI
   is the top-level default command, so resume is plain `opencode -s <id>`.

Verified correct as shipped: kimi `--session`, gemini `-r <index|latest>`
(index, not a uuid — the `positional-index` capability label is real), cursor
`--resume [chatId]`, plus claude/copilot/codex which had real round-trips.

Durable note: `reference/project/ferryx-agent-resume-argv-verified.md`.

### Residual risk, stated plainly

- **Five agents have flags confirmed but no real resume round-trip**: kimi, gemini,
  cursor, opencode, omo. Flag existence is proven; end-to-end behavior is not.
- **Discovery regexes track vendor on-disk layouts** (`~/.kimi/sessions/...`,
  `~/.gemini/tmp/<project>/chats`, ...). A vendor reorganizing storage breaks
  discovery silently — the resume affordance simply stops appearing. Degradation is
  safe (no crash, no minting), but there is no signal that it happened.
- **Title-only panes**: the `working` HOLD is settled by screen-rule `idle` -> `done`
  or by PTY exit -> `done`. A pane with no screen-rule coverage whose agent exits
  into a surviving shell holds `working` until the PTY itself exits. This is the
  documented "absence of evidence is never idle" tradeoff, chosen deliberately over
  re-introducing root cause 1.

## 7. The tree has multiple live writers — gates are not certifiable

The "half-applied edits" in §2 were **torn reads, not corruption.** At least three
independent tracks were editing this one working tree concurrently, uncommitted:
agent session restore, embedded-browser parity, and a remote/protocol track
(`src-tauri/src/remote/*`, `ipc/remote.rs`, `lib/tauri.ts`).

Proof, measured: `remote/server.rs` (00:22:49) and `remote/tests.rs` (00:23:08) were
written *while a test suite was running*. Separately, one edit here failed to apply
because another agent had already added the exact export being patched between the read
and the write — that failure is the only thing that prevented a collision.

Consequence: a delegate measured `105 files / 940 tests passed` + `tsc exit 0`; minutes
later the same commands gave `1 failed file / 2 failed tests` + `tsc rc=2`. Neither run
was wrong — the tree differed. **A green gate on a tree with live writers certifies
nothing.**

What IS certifiable, because these files were hash-verified byte-stable across the run:
the session-restore track, `7 files / 109 tests, rc=0`.

Detection and working rules: `reference/project/orca-lite-concurrent-agent-worktree-hazard.md`.
Real fix: one git worktree per agent.

## 8. Confirmed product bug needing a decision

Two chords are each bound to two different actions:

| Chord | Action A | Action B |
| --- | --- | --- |
| `Mod+[` | `terminal.focusPrevious` | `browser.back` |
| `Mod+]` | `terminal.focusNext` | `browser.forward` |

Dispatched globally without focus scoping, whichever is registered earlier in `SHORTCUTS`
swallows the chord. This was surfaced, not hidden: the test query was scoped per shortcut
row rather than relaxed. Needs either focus-scoped routing (browser history only when a
browser pane has focus) or distinct chords.

## 9. Resolution of every open item (2026-08-28, second pass)

All gates below were run on a tree proven quiescent by `scripts/check-tree-quiescent.sh`
immediately BEFORE and AFTER the run: `cargo test --lib --features native-terminal`
**326 passed**, `bun run --cwd ui test` **111 files / 987 tests passed**, `tsc --noEmit`
**exit 0**.

### 9.1 Shortcut collisions — NOT a bug; my earlier claim was wrong

I previously reported that `⌘[` / `⌘]` double-binding broke terminal pane focus. That was
incorrect. `App.tsx` already scopes these mutually exclusively — `browser.*` handlers are
`browserShortcutsActive ? fn : undefined` while `terminal.focusNext/Previous` are
`browserShortcutsActive ? undefined : fn` — and the dispatcher skips entries with no
handler. Exactly one side is ever registered, so no conflict occurs at runtime.

The real (latent) risk was that this safety rested on hand-coordination at distant call
sites. It is now locked by tests in `shortcuts.test.tsx`: a collision-inventory tripwire,
an assertion that every colliding pair spans the browser/terminal split, per-surface
dispatch tests, and a test documenting array-order precedence if both were ever registered.

The inventory found a **third** collision manual inspection had missed: `⌘F` —
`browser.find` vs `terminal.search`. Also cross-surface, also safe. Any NEW collision, or
any same-surface one, now fails CI.

### 9.2 Discovery was half-wired for 4 of 8 agents — fixed

`extractSessionIdFromPath` supported only claude/codex/copilot/kimi; everything else hit
`default: return null`, so gemini/cursor/opencode/omo declared resume argv whose session id
could never be discovered. Measured the real stores rather than assuming (my earlier
"vendor layout" risk list was largely fabricated):

- **cursor** `~/.cursor/chats/<hash>/<uuid>/store.db{,-wal,-shm}` — SQLite, held open → **added**
- **omo** `~/.omo/sessions/<cwd-slug>/<ISO>_<uuid>.jsonl` — id in filename → **added**
- **opencode** ONE global `opencode.db`, no per-session path → impossible by construction
- **gemini** resumes by INDEX/`latest`, there is no id → out of the model by design

The last two are now an explicit `UNSUPPORTED_DISCOVERY_AGENTS` set that logs the verified
reason. A **vendor-layout-drift warning** fires when a matched agent pid holds open files
that match nothing — converting the old silent no-op into an observable signal. Verified
against the real measured paths independently of the unit tests.

### 9.3 Resume round-trips — 2 proven, 1 hazard found, 2 credential-blocked

Negative controls with a valid-shaped nonexistent id; transcripts in
`docs/evidence/agent-session-restore/resume-negative-controls.txt`.

- **omo** rejects (`No session found matching ...`), creates nothing — **proven resume-only**
- **opencode** rejects (`Invalid session ID`) — **proven resume-only**
- **kimi** — **new finding**: help says "Session ID to resume", but it ACCEPTED the bogus id
  and created `~/.kimi/sessions/<hash>/<that-id>` (probe artifact deleted). It is really
  CREATE-OR-RESUME, so a stale id degrades silently into a new empty session instead of
  erroring. Ferryx still never mints, so the hard constraint holds — but kimi is the one
  agent that fails quietly.
- **cursor** exits at `Authentication required` before evaluating the flag — unverifiable
  without using the operator's credentials. Not pursued.
- **gemini** account auth disabled — inconclusive, and moot given index-based resume.

### 9.4 Concurrent-writer hazard — now detectable

`scripts/check-tree-quiescent.sh [seconds]` (default 90) fails with exit 1 and lists the
offending paths when anything under `ui/src` or `src-tauri/src` was written inside the
window. Run it before and after any gate; a green gate on a moving tree certifies nothing.
The structural fix remains one git worktree per agent.

### 9.5 Still open, honestly

- cursor and gemini resume round-trips remain unproven (credential-gated).
- omo discovery is opportunistic: `lsof` on a live omo process showed no session jsonl
  open, so it only resolves when caught mid-write.
- kimi's silent create-on-stale-id has no CLI-side remedy.

## 10. Scope note

Repairs here were limited to (a) the session-restore track and (b) mechanical
syntax/type repair of half-applied edits that blocked every gate. No browser-parity
*behavior* was changed, no failing test was deleted, skipped, or weakened, and no
uncommitted work by the other track was reverted.
