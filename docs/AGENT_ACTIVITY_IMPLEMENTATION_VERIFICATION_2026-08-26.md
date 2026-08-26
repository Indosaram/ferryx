# Agent Activity Implementation Verification — 2026-08-26

## Result

Three agent-activity features were reviewed; only one was reachable from the running app. Activity detection already drove the spinner and status dots, but desktop notifications and agent-type tab icons were complete, tested modules with **zero non-test callers** — dead code behind a green suite. Four correctness bugs sat alongside them.

All nine items are now implemented and verified. Every fix carries a failing-first proof and a mutation proof showing its asserting test can actually fail.

**Superseded in part — see "Defect O2" below.** The first pass wired activity through the per-pane
prop chain, which the user's real-app run proved dead. The event source is now the store's global
native subscription; the per-pane path has been removed.

| Gate | Result |
| :--- | :--- |
| `cd ui && npx vitest run` | 96 files / 840 tests passed, 0 skipped |
| `cd ui && npx tsc --noEmit` | exit 0 |
| `bun run --cwd ui build` | exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib notification` | 91 passed, 0 failed |

Desktop end-to-end verification is performed manually against a real build; the checklist is
`docs/AGENT_ACTIVITY_NOTIFICATION_MANUAL_E2E.md`.

## What was broken

| Item | Defect |
| :--- | :--- |
| B1 | `ui/src/lib/notificationCoordinator.ts` had no non-test importer, and `ui/src/App.tsx` never passed `onBell` to `TerminalSplitView`. The prop chain below it was already complete, so bell events walked from Rust to `undefined`. No desktop notification could ever fire. (Wiring that chain was necessary but **not sufficient** — see Defect O2.) |
| B2 | `resolveAgentIcon` in `ui/src/lib/agentIcon.ts` (22 agent types) had no non-test caller. `agentType` was stored on `TerminalActivity` and never read. |
| C1 | `isAgent: Boolean(parsed?.isAgent \|\| classified)` was unconditionally true: the reducer already returned early when `classified` was null. A plain shell titled `make: running tests` counted as an agent. |
| C2 | `DONE_RE` / `WAITING_RE` scanned the whole title, so a live working title containing the word "done" classified as **done** — a false completion notification once B1 was wired. |
| C3 | Unread was gated on `state.layout.activeTabId` alone, but each tab group renders its own active tab, so a tab the user was watching in a non-focused group was marked unread and badge-counted. |
| C4 | Three O(tabs x sessions) selectors ran unmemoized on every render, returning fresh identities and defeating `memo` on `SortableTab`. |
| N1 | `WorktreeList.tsx` idle dot was a hardcoded `#00bc7d`, bypassing theme tokens and reading as "success" for an idle row. |
| N2 | `notificationSettings.ts` still emitted the legacy `rorca:notifications:settings-changed` event name beside migrated `ferryx.*` keys. |

## Title classification

C2 took three iterations because both naive directions fail.

1. **Free-text matching** (original) → false positives: `omo: fixing the done-state handler` read as done.
2. **End-only anchoring** (first fix) → silent nulls, strictly worse: `omo: permission required` and
   `codex: done (3 files changed)` classified as *no status at all* — no dot, no notification.
3. **Shipped rule**: start-or-end anchored on a normalized status segment, with a spinner gate.

`WAITING_AT_END_RE` / `DONE_AT_END_RE` match a status word ending the title, allowing trailing `.`, `!`,
or an ellipsis. `statusSegment()` strips leading decoration via `LEADING_DECORATION_RE`
(`/^[^\p{L}\p{N}]+/u`, so any glyph rather than only the known spinner set) and then strips a known
agent-name prefix built from `KNOWN_AGENT_MATCHERS`. `WAITING_AT_SEGMENT_START_RE` /
`DONE_AT_SEGMENT_START_RE` match a status word opening that segment, with `(?![\w-])` so
`done-state handler` stays work.

Those segment-start checks are gated on the absence of a spinner glyph. A live spinner is direct
evidence of active work, so a spinning `omo: idle connection cleanup` is working, while
`codex: run finished` — status word at the end — is still done.

| Title | Classification |
| :--- | :--- |
| `omo: done` | done |
| `codex: done (3 files changed)` | done |
| `omo: completed in 4m12s` | done |
| `omo: permission required to edit src` | waiting |
| spinner + `omo: fixing the done-state handler` | working |
| spinner + `omo: idle connection cleanup` | working |
| spinner + `codex: run finished` | done |
| `zsh /repo` | no status |

## Defect O2 — activity never fired in the real app

The unit suite was green and the feature was still dead on launch. Two independent breaks:

1. **Wrong event source.** `workspaceStore` fed activity from `terminalEventBus.subscribeTitle`, which
   is driven by the legacy `terminal_output` channel. Native-ghostty sessions never stream that
   channel, so `activityBySessionId` stayed empty: no icon, no spinner, no notification. The bus and
   `scanTerminalOscTitles` remain in place for the remote/web client, which still uses them.
2. **Mount-scoped listener.** `TerminalSplitView` renders panes for the active tab only, so the
   per-pane `native_terminal_title` / `native_terminal_bell` listeners existed solely for the tab
   already on screen. Since the coordinator deliberately stays silent while a tab is focused, the one
   tab that could emit was the one tab that must not notify.

Fix: `workspaceStore` subscribes **once** to `onNativeTerminalTitle` / `onNativeTerminalBell`
(`ui/src/lib/tauri.ts`). Those events are emitted per attached session by the daemon stream pump
(`src-tauri/src/native_terminal/surface_host.rs`), independent of React mounting, and broadcast
globally via `app.emit`. The store resolves the payload's **backend** session id to the local session,
maps it to a tab, and dispatches `SESSION_TITLE_ACTIVITY`. Bells reach `App` through
`subscribeTerminalBell`. This also fixes an id mismatch: the old per-pane path handed the coordinator a
**local** session id where the payload carried a backend one.

The now-unreachable per-pane path was deleted — `onBell` / `onTitleChange` props across
`TerminalSplitView`, `TerminalPane`, `NativeTerminalPane`, plus that component's duplicate listeners.
`ui/src/state/workspaceNativeActivity.test.tsx` covers the new path; MUT-K confirms it fails when the
native subscription is disabled.

Verified by unit tests only. Real-app confirmation is manual: section I of the E2E checklist.

## Notification wiring

`ui/src/App.tsx` holds one `NotificationCoordinator` for the app's lifetime, wired to `markTabUnread`
and `markWorktreeUnread` through refs. An effect reports each session's current state to
`handleAgentStateChange` when it changes. Bells arrive via `subscribeTerminalBell` from the store's
global native subscription, and `App` resolves the worktree label and terminal title from
`activityNotificationTargets`, falling back to the session record because a bell can arrive before any
title.

All notification policy stays in the coordinator — focus gating, the 1s per-session bell throttle, the
1.5s post-completion bell suppression, and completion-edge detection where a session's first observed
state never notifies. `App.tsx` reports state and never calls `dispatchNotification` directly.

`selectActivityNotificationTargets` in `ui/src/state/workspaceStore.ts` is the bridge: one
`ActivityNotificationTarget` per activity-carrying session, carrying tab id, worktree path, worktree
label, agent label, terminal title, and state.

## Mutation proofs

A passing test is only evidence if it can fail. Each fix was temporarily reverted, the asserting test
observed failing, then the file restored and confirmed byte-identical with `diff -q`.

| Proof | Mutation | Observed failure |
| :--- | :--- | :--- |
| MUT-A | restore free-text `DONE_RE` / `WAITING_RE` | `agentTitle.test.ts` — `Received: "done"` |
| MUT-K | disable the store's `onNativeTerminalTitle` subscription | `workspaceNativeActivity.test.tsx` — 2 of 3 failed |
| MUT-B | restore `isAgent: Boolean(parsed?.isAgent \|\| classified)` | `workspaceActivity.test.tsx` — `isAgent: false` |
| MUT-C | short-circuit `isTabVisible` before the group loop | `workspaceActivity.test.tsx` — `unreadTabIds["tab-b"]` |
| MUT-D | force `AgentIcon = null` | `SortableTab.agentIcon.test.tsx` — 2 of 3 |
| MUT-E | drop `onBell` from `TerminalSplitView` | `App.notifications.test.tsx` — bell dispatch |
| MUT-F | disable the `handleAgentStateChange` call | `App.notifications.test.tsx` — completion dispatch |
| MUT-G | disable both `playNotificationSound` calls | `App.notifications.test.tsx` — 2 of 4 |
| MUT-H | delete the segment-start checks | `agentTitle.test.ts` — 2 of 21 |
| MUT-I | replace the spinner gate with `if (true)` | `agentTitle.test.ts` — spinner precedence |
| MUT-J | remove one selector `useMemo` | `workspaceActivity.test.tsx` — identity `toBe` |

N1 is asserted by `WorktreeList.test.tsx` (token class present, no hex). N2 is asserted by a repo-wide
search returning no remaining legacy event references.

## Notes for the next reader

- The test runner is **vitest**. `bun test` fails on `ui/src` for lack of a DOM preload and its
  failures there are meaningless.
- `cargo test <filter>` **without `--lib`** matches only integration-test binaries in this repo: every
  target prints `running 0 tests ... N filtered out` and exits 0. That is a false green. Use `--lib`
  for the notification unit tests, and treat "0 passed" as failure to verify.
- `terminalBell` defaults OFF in notification settings, so bell notifications stay silent until a user
  enables them. `agentTaskComplete` defaults ON.
