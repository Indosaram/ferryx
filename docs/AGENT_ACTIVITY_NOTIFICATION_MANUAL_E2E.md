# Agent Activity Notification and Tab Icon Manual E2E Checklist

Run these manual checks on a packaged desktop build or local desktop session. Desktop notifications, audio playback, and Dock badging rely on operating system integrations that automated unit tests mock out.

## Launch

Three traps will make a correct build look broken. All three were hit during verification, so do
these in order.

**1. Kill the daemon first.** The daemon owns PTY spawning, and it survives quitting the app.
Launching a new build while an old daemon is running means your panes are spawned by the *old*
binary, so you reproduce the old behavior and conclude the fix failed.

```bash
pkill -f 'Ferryx|ferryx' ; sleep 1
open src-tauri/target/debug/Ferryx.app
```

**2. Launch the bundle, not the bare binary.** macOS attributes a notification to the *responsible
process*. Started from a terminal, notifications are attributed to that terminal (they appeared as
`cmux` during verification), not to Ferryx. Use `open ...Ferryx.app` as above.

**3. A debug bundle needs the Vite dev server.** The debug binary is `cfg(dev)`, so Tauri loads
`devUrl` at `http://127.0.0.1:5173`. Without it the window renders fully black.

```bash
bun run --cwd ui dev    # leave running, in a separate shell
```

For a release-style check instead, build the bundle so no dev server is needed:
`bun run --cwd ui build && cargo tauri build --manifest-path src-tauri/Cargo.toml`.

### Known open defect: no banner is delivered, and it is not our permission logic

Measured on the running app. Clicking Settings > Notifications > Send Test Notification logs:

```
ferryx: (UserNotifications) [com.ferryx.app] Getting notification settings (async)
ferryx: (UserNotifications) [com.ferryx.app] Got notification settings [ hasResult: 1 ... ]
```

macOS answers by bundle id and returns real settings, so registration works and the in-app
`authorized` status is authoritative (`permission.rs` `read_settings` sets `authoritative: true`; the
`dev_fallback_status` path would report `false`). `preflight()` therefore returns `Submit`.

But filtering the same window for `com.apple.UserNotifications` lines excluding those two settings
queries returns **empty**, across repeated clicks: `builder().show()` reports no error yet no
add-request ever reaches `usernoted`. The gap is inside the Tauri notification plugin's submit path,
not Ferryx's permission logic, signing, Focus, or SIP.

Until that is resolved, treat every notification row below as blocked, and verify the rest of the
chain (spinner, attention marker) which does work.

Two measurement traps: the mtime of
`~/Library/Group Containers/group.com.apple.usernoted/db2/db` does **not** change even for a
notification that demonstrably arrives, so use `log show` instead. And a dead Vite dev server
collapses the accessibility tree to window chrome only (6 elements) with a black window, which looks
exactly like a broken app.

### Sanity check before judging any agent behavior

In a fresh Ferryx pane:

```bash
echo "TERM=$TERM FSID=${FERRYX_SESSION_ID:-unset} SOCK=${FERRYX_AGENT_STATE_SOCKET:-unset}"
```

`TERM` must not be `dumb`, and both `FERRYX_*` values must be set. If `TERM` is `dumb`, agent TUIs
run non-interactive and never report state (this was the root cause of "no agent feature works"). If
the `FERRYX_*` values are unset, the pane belongs to an older daemon generation: quit and redo step 1.

Record PASS or FAIL for every item, including what you observed if a step fails.

---

## A. Permission and Settings

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| A1 | Open Settings (`Cmd+,` or `Ctrl+,`), select Notifications, and check System Permission | PASS: Status displays "authorized" or "granted" on a packaged build. FAIL: Status shows "denied" without recovery options. |
| A2 | Verify default toggle states in Settings > Notifications | PASS: "Enable Notifications" is ON, "Agent Task Complete" is ON, and "Terminal Bell" is OFF. FAIL: Any default toggle differs. |
| A3 | With window focused, click "Send Test Notification" | PASS: An OS notification banner arrives and notification sound plays immediately. FAIL: No notification arrives or no audio plays. |
| A4 | Toggle "Enable Notifications" OFF, click "Send Test Notification", then toggle it back ON | PASS: No notification arrives while master toggle is OFF; notifications resume when switched back ON. FAIL: Test notification fires while disabled. |

---

## B. Agent Type Icons and Status Dots

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| B1 | In Tab 1, run `printf '\033]2;⠋ codex: analyzing repository\007'` | PASS: Tab 1 shows the Code2 agent icon with a spinning loader status dot overlaid on the bottom right corner. FAIL: Generic icon appears, or spinner dot is missing or detached from the icon. |
| B2 | In Tab 1, run `printf '\033]2;⠋ claude: thinking\007'` | PASS: Tab 1 icon switches to the Sparkles icon with the spinning loader dot. FAIL: Icon does not change to Claude icon. |
| B3 | In Tab 1, run `printf '\033]2;⠋ custom-bot: running steps\007'` | PASS: Tab 1 shows the generic Bot agent icon with the spinning loader dot overlaid. FAIL: No agent icon renders for an unrecognized agent name with a spinner title. |
| B4 | In Tab 1, run `printf '\033]2;codex: needs input\007'` | PASS: Tab 1 displays the Code2 icon overlaid with an amber waiting dot. FAIL: Amber dot does not appear on top of the icon. |
| B5 | In Tab 1, run `printf '\033]2;codex: done\007'` | PASS: While Tab 1 is focused, it displays the Code2 icon overlaid with a green done dot. FAIL: Green done dot is missing. |

---

## C. Background Agent Completion Notifications

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| C1 | In Tab 1, queue the whole transition in one command so it completes while the tab is in the background: `printf '\033]2;⠋ codex: running tests\007'; sleep 8; printf '\033]2;codex: done\007'`. Immediately switch to Tab 2 and unfocus the Ferryx window (click another application). Wait for the sleep to elapse. | PASS: An OS notification banner arrives with the completion title, sound plays, and Tab 1 displays an unread indicator. FAIL: No notification arrives, sound stays silent, or tab is not marked unread. |
| C2 | In Tab 1, queue: `printf '\033]2;omo: working\007'; sleep 8; printf '\033]2;omo: needs input\007'`. Immediately switch to Tab 2 and unfocus Ferryx. Wait for the sleep to elapse. | PASS: An OS notification arrives indicating input is needed, sound plays, and Tab 1 shows unread status. FAIL: No notification or sound fires on transition to waiting state. |
| C3 | Open a brand new terminal tab (Tab 3). Without any prior working state, queue `sleep 8; printf '\033]2;codex: done\007'`, then unfocus Ferryx. | PASS: No notification arrives and no sound plays because the first observed state for a session never notifies. FAIL: Spurious notification fires on initial state arrival. |

---

## D. Focused Window Silence

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| D1 | Keep Tab 1 focused with the Ferryx window in the foreground. Run `printf '\033]2;⠋ codex: build\007'`, wait 1 second, then run `printf '\033]2;codex: done\007'`. | PASS: Tab 1 shows green done state visually, but NO OS notification banner appears and NO audio plays. FAIL: OS notification appears or sound plays while window is focused. |
| D2 | With Ferryx focused, keep Tab 1 active and run `printf '\033]2;⠋ codex: review\007'`. Switch to Tab 2 while keeping the Ferryx window focused. Transition Tab 1 to done: `printf '\033]2;codex: done\007'`. | PASS: Tab 1 shows the unread dot, but no OS notification banner pops up and no sound plays because Ferryx remains the focused foreground window. FAIL: OS notification pops up while the application window is focused. |

---

## E. Terminal Bell Notifications and Throttling

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| E1 | Open Settings > Notifications. Turn ON "Terminal Bell". Switch to Tab 2. Unfocus Ferryx. In Tab 1, trigger a bell: `printf '\a'`. | PASS: OS notification banner arrives for the terminal bell, sound plays, and Tab 1 gets marked unread. FAIL: No notification or sound fires. |
| E2 | Open Settings > Notifications. Turn OFF "Terminal Bell". Unfocus Ferryx. In Tab 1, trigger a bell: `printf '\a'`. | PASS: Tab 1 is marked unread, but NO OS notification arrives and NO sound plays. FAIL: Bell notification fires while the bell toggle is off. |
| E3 | With "Terminal Bell" turned ON, unfocus Ferryx. In Tab 1, trigger two rapid bells within 500 milliseconds: `printf '\a\a'`. | PASS: Exactly ONE notification arrives and one sound plays due to the 1-second bell throttle. FAIL: Two notification banners pop up or audio plays twice in rapid succession. |
| E4 | With "Terminal Bell" ON and Ferryx window focused, run `printf '\a'` in the active tab. | PASS: No OS notification banner pops up and no audio plays. FAIL: Focused terminal bell emits desktop notification. |

---

## F. Post-Completion Bell Suppression

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| F1 | Ensure "Terminal Bell" is ON. Unfocus Ferryx. In Tab 1 (currently in background), transition from working to done and immediately emit a bell within 1 second: `printf '\033]2;⠋ codex: run\007'; sleep 1; printf '\033]2;codex: done\007\a'`. | PASS: The agent completion notification fires, but the trailing terminal bell is suppressed (no duplicate bell notification or duplicate sound within 1.5 seconds). FAIL: Two separate notifications fire in immediate succession. |

---

## G. Unread Dots, Worktree Row, and Dock Badge

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| G1 | Create two tabs in Worktree A (Tab 1, Tab 2) and one tab in Worktree B (Tab 3). Unfocus Ferryx. Trigger background completion in Tab 1 and Tab 3. | PASS: Tab 1 and Tab 3 show unread dots. Both Worktree A and Worktree B rows in the sidebar show unread indicator dots. The macOS Dock badge shows count 2. FAIL: Sidebar rows miss unread dots, or Dock badge count doesn't equal 2. |
| G2 | Focus Ferryx and click Tab 1 to activate it. | PASS: Tab 1 unread dot disappears. Worktree A unread dot disappears. Dock badge count decrements from 2 to 1. Tab 3 remains unread. FAIL: Tab 1 dot stays unread, Worktree A dot remains stuck, or Dock badge doesn't decrement. |
| G3 | Click Tab 3 to activate it. | PASS: Tab 3 unread dot disappears. Worktree B unread dot disappears. Dock badge clears completely to blank (count 0). FAIL: Dock badge remains visible or unread dot lingers. |

---

## H. Mid-Sentence Keyword Discrimination

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| H1 | In Tab 1, queue a title containing "done" mid-sentence during active work: `sleep 8; printf '\033]2;⠋ omo: fixing done-state logic\007'`. Switch to Tab 2 and unfocus Ferryx. | PASS: Tab 1 shows the OMO icon with spinning working dot. NO completion notification fires, no sound plays, and no unread dot appears. FAIL: Title is mistakenly parsed as completed and emits a completion notification. |
| H2 | In Tab 1, send a title that ends with done: `printf '\033]2;omo: done\007'`. | PASS: Tab 1 transitions to done and fires an agent completion notification. FAIL: Genuine completion title is ignored. |
| H3 | Switch to Tab 2, unfocus Ferryx, then send a completion title that carries trailing detail: `printf '\033]2;⠋ codex: run\007'; sleep 1; printf '\033]2;codex: done (3 files changed)\007'`. | PASS: Tab 1 transitions to done and fires one completion notification. FAIL: The title is treated as having no status, leaving the tab spinning with no notification. |
| H4 | Switch to Tab 2, unfocus Ferryx, then send a waiting title that carries trailing detail: `printf '\033]2;omo: permission required to edit src\007'`. | PASS: Tab 1 shows the amber waiting dot and fires an input-needed notification. FAIL: No waiting dot and no notification appear. |

---

## I. Background and Unmounted Tab Delivery

Only the active tab's panes are mounted, so this section is the one that catches the defect a green
unit suite missed: a title or bell arriving for a tab that is not on screen. Every step here must be
queued with `sleep` **before** switching away, because you cannot type into a tab you have left.

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| I1 | In Tab 1 run `printf '\033]2;⠋ codex: building\007'; sleep 10; printf '\033]2;codex: done\007'`, then immediately switch to Tab 2 and stay there with the window focused. | PASS: While in Tab 2, Tab 1 shows the Codex icon with a spinning dot, then flips to the completion state when the sleep elapses. FAIL: Tab 1 shows no icon or dot, or its state never changes while backgrounded. |
| I2 | Repeat I1 but unfocus Ferryx entirely after switching to Tab 2. | PASS: An OS notification banner arrives for Tab 1's completion and Tab 1 is marked unread. FAIL: No notification arrives. |
| I3 | Open a second project (or worktree) so its tabs live in a different layout, start a working title in one of its tabs, switch to a tab in the first project, unfocus Ferryx, and let the queued completion fire. | PASS: The completion notification names the other project's worktree and its tab is marked unread. FAIL: Nothing arrives, which means the daemon stream pump for the parked layout is not attached — record this as a finding, it is a known open question. |
| I4 | In Tab 1 run `sleep 10; printf '\a'`, switch to Tab 2, unfocus Ferryx, and enable Settings > Notifications > Terminal Bell first. | PASS: The bell notification arrives for Tab 1 while it is unmounted. FAIL: No bell notification, which means bells are still mount-scoped. |
| I5 | With the window focused and Tab 1 active, run `printf '\033]2;⠋ codex: working\007'` then `printf '\033]2;codex: done\007'` in Tab 1 itself. | PASS: The icon and dot update in place and NO notification fires, because the user is watching that tab. FAIL: A notification fires for the tab currently on screen (double-fire regression). |

---

## J. Real agent, extension-reported state (the omo path)

Sections A-I drive state with `printf` OSC titles. That covers `agy`, `codex`, and every agent that
announces status in its terminal title, but it does **not** cover `omo`: omo's title is just a bare
name, so its state can only ever come from the agent extension reporting over
`/tmp/rorca-$UID/agent-state.sock`. This section is the only one that exercises that path, and it is
the one remaining check that cannot be automated from a coding session.

Complete the Launch steps and the sanity check first. If `TERM` is `dumb`, stop: nothing here can pass.

| # | Action | Expected (PASS / FAIL) |
|---|---|---|
| J1 | Open Tab 1 and run `agy`. | PASS: Tab 1 shows the Antigravity brand mark. FAIL: it shows the plain terminal icon. |
| J2 | Open Tab 2 and run `omo`. | PASS: Tab 2 shows the **terminal icon**. This is correct, not a bug: there is no vetted omo brand mark, and `ui/src/assets/agent-logos/ATTRIBUTION.md` requires agents without one to use the terminal icon. FAIL: any invented or generic stand-in glyph appears. |
| J3 | In Tab 2, send omo a prompt that takes a few seconds, then **switch to Tab 1** while it is still working. | PASS: Tab 2 (now non-active) shows a spinning working dot. FAIL: no dot, which means the extension is not reporting - re-run the sanity check. |
| J4 | Stay on Tab 1, unfocus Ferryx, and let the omo turn finish. | PASS: a notification banner arrives attributed to **Ferryx** and naming the agent as `OMO`, and Tab 2 gets the attention dot. FAIL: no banner (check Settings > Notifications > Agent Task Complete), a banner attributed to your terminal app (you skipped Launch step 2), or a banner with no agent name. |
| J5 | Repeat J3 but stay **on** Tab 2 with Ferryx focused. | PASS: the dot animates in place and **no** notification fires, because you are already watching. FAIL: a notification fires for the tab on screen. |

### Why J4's agent name matters

`selectActivityNotificationTargets` originally derived the notification's agent label only from the
terminal title. Because the extension reducer keeps the previous (empty) title while setting
`agentType`, a completed omo turn produced a notification with **no agent name**. That is fixed via
`agentDisplayNameForType`, so J4 checking for the literal `OMO` in the banner is a real regression
guard, not cosmetic.
