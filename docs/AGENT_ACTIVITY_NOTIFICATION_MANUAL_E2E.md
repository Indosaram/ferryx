# Agent Activity Notification and Tab Icon Manual E2E Checklist

Run these manual checks on a packaged desktop build or local desktop session. Desktop notifications, audio playback, and Dock badging rely on operating system integrations that automated unit tests mock out.

## Launch

```bash
cargo tauri dev --manifest-path src-tauri/Cargo.toml
```

For macOS notification permission tests, use the packaged app bundle (`Ferryx.app`). Dev builds without a bundle identifier cannot register with macOS Notification Center.

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
