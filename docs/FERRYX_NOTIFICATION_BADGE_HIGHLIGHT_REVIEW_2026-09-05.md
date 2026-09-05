# Notification, Badge, and Highlight Review

Date: 2026-09-05
Scope: Current working tree at `ea27a71`, including existing uncommitted changes.
Verdict: Changes recommended. Passing tests do not establish correct end-to-end attention behavior.

This is a review, not an implementation. No application or test source was changed. The working tree was actively changing in other sessions; line numbers below identify the reviewed version and may move. The additional reviewer produced `.omo/evidence/notification-badge-highlight-gate-review.md`; this report supersedes its severity labels, unverified claims, and suggested patches.

## Findings

### F01 - P1: Reading a waiting pane does not dismiss its highlight

- References: `ui/src/components/TerminalSplitView.tsx:1015`, `ui/src/lib/activity.ts:59`, `ui/src/state/workspaceStore.ts:1988`.
- The reducer accepts `MARK_SESSION_ACTIVITY_SEEN` for waiting, but the frame predicate checks `seen` only for done. `summarizeActivities` also counts every waiting entry, including acknowledged ones. The Dock badge does check `seen`, so the three surfaces disagree.
- Reproduction: Render the real `TerminalSplitView` with `state: "waiting", seen: true`. Isolated JSDOM contained **one** `attention-frame-bottom`. The real selector still returned `waiting`.
- Impact: Clicking or revisiting a waiting pane can clear the Dock count while leaving its warning dot and native/DOM frame lit. The earlier claim that waiting dismissal was fixed covered only the reducer.
- Fix: Apply the same unseen-attention condition to the frame and attention summaries. If persistent waiting status is useful, expose it separately from dismissible attention. Add a real component test for acknowledged waiting.

### F02 - P1: Inactive projects and unfocused windows can be classified as already observed

- References: `ui/src/state/workspaceStore.ts:328`, `ui/src/state/workspaceStore.ts:2128`, `ui/src/state/workspaceStore.ts:2163`, `ui/src/state/workspaceStore.ts:1571`.
- Parked project events run through the normal reducer using a snapshot that retains its former active tab and active leaf. `isSessionActivelyObserved` considers only that layout; it knows neither the mounted project nor native window focus.
- Reproduction: Project A last had tab A focused, then the user visits project B. Applying working -> idle to A's parked snapshot produced `done, seen: true` and badge **0**. A completion in the focused tab while the entire app is unfocused has the same layout-only acknowledgment problem. Coordinator unread marking cannot repair that tab because `MARK_TAB_UNREAD` refuses visible tabs.
- Impact: The completion that most needs background attention can disappear from the Dock count, pane highlight, and done indicator. This is distinct from F03: correctly counting attention still would not dispatch the missing notification.
- Fix: Carry actual observation context into activity processing: mounted workspace, window foreground focus, visible tab, and focused pane. Parked projects must never be observed. Preserve layout state rather than clearing active IDs as a workaround.

### F03 - P1: Other projects do not feed notification dispatch

- References: `ui/src/state/workspaceStore.ts:371`, `ui/src/state/workspaceStore.ts:383`, `ui/src/state/workspaceStore.ts:1159`, `ui/src/state/workspaceStore.ts:1368`, `ui/src/App.tsx:554`.
- Native BEL resolves only against the mounted workspace and returns immediately for another project. Agent-state events do update parked snapshots, but notification targets are derived only from the mounted state. The App effect never receives parked completion edges.
- Reproduction: Leave an agent in project A, select project B, then let A complete or emit BEL while Ferryx is unfocused. Trace: parked state update or immediate BEL return -> no A target -> no `handleAgentStateChange`/`handleTerminalBell` call for A.
- Impact: No banner or sound for work in other projects. Switching back can produce a late alert or no alert depending on whether the previous state was observed by the coordinator.
- Fix: Route activity and BEL events through a workspace-aware application-level coordinator, including ownership metadata and unread dispatch back to the owning workspace. Merely adding cached targets without event identity risks delayed or duplicate notifications.

### F04 - P1: Mute and System Default can replay the old custom sound

- References: `ui/src/lib/tauri.ts:722`, `ui/src/components/settings/NotificationsSection.tsx:300`, `ui/src/lib/notificationCoordinator.ts:175`.
- Changing the sound dropdown changes only `customSoundId`. `customSoundPath` remains saved. The adapter tests only whether a path exists and never uses `soundId`.
- Reproduction through the real adapter with an intercepted Tauri transport: both `soundId: "none"` and `soundId: "system"`, with `/review/old-custom.wav`, invoked `cmd_notification_play_sound` with that path and volume 80. No actual audio or OS notification was emitted by the probe.
- Impact: A user who explicitly selects mute still hears the previous custom file on automatic notifications. Test notification and automatic notification paths also differ because the settings preview checks the sound ID but the coordinator does not.
- Fix: Only invoke custom audio when the selected mode is custom. Keep the path for convenience if desired, but never interpret its existence as the selected mode. Carry a deliberate system/silent mode to native banner submission.

### F05 - P2: A title refresh revives an acknowledged completion

- References: `ui/src/state/workspaceStore.ts:2010`, `ui/src/state/workspaceStore.ts:2068`, `ui/src/state/workspaceStore.ts:2140`.
- Title handling constructs a new activity without `previous.seen`. `applySessionActivity` recomputes acknowledgment from the incoming object and current visibility, even when the activity state has not changed.
- Reproduction using the real reducer: working -> idle -> `MARK_SESSION_ACTIVITY_SEEN` -> non-status title `OmO - review` while the tab is backgrounded. The observed value changed from `seen: true` to `seen: false`, with state still done.
- Impact: A dismissed done dot, badge contribution, and frame can return without a new task. The first-auto-resume suppression is also vulnerable to a subsequent title repaint.
- Fix: Preserve acknowledgment across metadata updates within the same attention episode. Reset it only on a genuine new activity cycle. Blindly copying `previous.seen` across all transitions would hide legitimate subsequent completions.

### F06 - P2: Reading through pane navigation or worktree selection is inconsistent

- References: `ui/src/state/workspaceStore.ts:1537`, `ui/src/state/workspaceStore.ts:1808`, `ui/src/state/workspaceStore.ts:437`, `ui/src/App.tsx:1483`, `ui/src/components/NativeTerminalPane.tsx:1878`.
- `ACTIVATE_TAB` acknowledges its focused session. `FOCUS_PANE` updates only layout. `SELECT_WORKTREE` clears unread flags for every tab in the worktree but does not acknowledge the active session. The native sink focus handler sends terminal focus, not a read acknowledgment.
- Reproduction: `FOCUS_PANE` changed the active leaf to the completed session while leaving `seen: false`, badge 1. Returning to a completed single-pane worktree left `seen: false`, badge 1, and `unreadTabIds: {}`.
- Impact: Keyboard pane switching and sidebar worktree selection leave read completion frames/counts behind. Worktree selection can also clear bell-only unread markers for hidden tabs the user never visited.
- Platform detail: Native mouse focus events provide a separate acknowledgment on macOS and Windows. The reviewed native focus emitter search found no Linux counterpart; portable DOM/keyboard navigation cannot depend on those OS mouse hooks.
- Fix: Acknowledge the actually selected session in the shared navigation path, not all sessions/tabs. Have native clicks and keyboard/DOM navigation converge on that operation.

### F07 - P2: Auto-resume suppression stops before notifications

- References: `ui/src/App.tsx:969`, `ui/src/state/workspaceStore.ts:2162`, `ui/src/state/workspaceStore.ts:1357`, `ui/src/App.tsx:558`, `ui/src/lib/notificationCoordinator.ts:148`.
- `SUPPRESS_NEXT_ATTENTION` stores the first resume completion as seen and consumes its suppression flag. The notification target contains state but no suppression/episode metadata. The App still forwards working -> done to the coordinator.
- Reproduction: Feed the real targets from suppressed working and suppressed done states to the real coordinator using App's `nextState: target.state` mapping, with window focus false. The final activity had `seen: true`, but the intercepted transport recorded **one** notification dispatch.
- Impact: An unfocused restart can still emit completion notifications and sounds; the coordinator can also re-add unread flags that the reducer intentionally suppressed.
- Fix: Preserve the reason for suppressing that completion through the notification boundary. Do not suppress all seen entries indiscriminately, since foreground observation and synthetic restore are different policies.

### F08 - P2: macOS System Default does not request a system sound

- References: `src-tauri/src/notification/permission.rs:130`, `ui/src/lib/tauri.ts:728`, `src-tauri/src/notification/model.rs:45`.
- The frontend does nothing for system sound without a custom path, expecting the native banner to supply sound. The macOS backend creates `UNMutableNotificationContent` and sets only title/body; it never assigns a notification sound. The dispatch DTO cannot express system versus silent mode.
- Impact: The default sound path is missing even for a bundled macOS app. This is a source-level finding; actual speaker output was not tested.
- Fix: Represent the banner sound mode in the request, set `UNNotificationSound::defaultSound` for system mode, and leave the native sound unset for silent/custom mode. Verify custom mode does not cause a second OS sound.

### F09 - P2: macOS submission reports success without observing native acceptance

- References: `src-tauri/src/notification/permission.rs:143`, `src-tauri/src/notification/service.rs:96`.
- `addNotificationRequest_withCompletionHandler` receives `None`, and the function immediately returns `Ok(())` unless an Objective-C exception occurred. Asynchronous request errors have no observation path. The service maps that result to `submitted: true`.
- Impact: The settings test can report submission success after native rejection. This is distinct from Focus/Do Not Disturb hiding an accepted notification.
- Fix: Subscribe to the native completion callback and report its result with a bounded timeout, following the existing callback pattern used for permission queries. Do not claim visible delivery.

### F10 - P2: Remote attention has a different and incomplete state contract

- References: `ui/src/App.tsx:310`, `ui/src/App.tsx:334`, `src-tauri/src/remote/server.rs:270`, `src-tauri/src/remote/server.rs:318`, `ui/src/remote/RemoteSessionList.tsx:105`, `ui/src/remote/RemoteApp.tsx:73`.
- Remote publication iterates the active layout, not parked worktree layouts/projects, and publishes raw state without acknowledgment. The server returns no worktree attention unless that worktree matches the current desktop selection. Both remote aggregation layers rank working above done, unlike the desktop unseen-done precedence.
- Reproduction: Normalize one worktree with working and done entries through the real remote normalizer. Its output remained `attention: "working"`. The server test explicitly expects the same precedence at `src-tauri/src/remote/tests.rs:3079`. A client test fabricates a waiting context for another worktree, while the production attention rollup returns `None` for non-selected worktrees.
- Impact: Remote users can miss completions behind working tasks, see acknowledged completions as fresh done state, and receive no attention indication for another worktree/project. The mobile attention jump affordance cannot compensate for missing server data.
- Fix: Publish activity inventory across the intended workspace scope, including read/attention semantics; aggregate per owning worktree rather than selected context. Share precedence with desktop. Retain the remote waiting-only jump behavior if intentional, but separate it from general status aggregation.

### F11 - P2: DAG badges can appear on the wrong pane and the right pane simultaneously

- References: `ui/src/components/dag/DagPaneBadge.tsx:117`, `ui/src/components/dag/DagPaneBadge.tsx:145`, `ui/src/state/dagRunOwnership.ts:37`.
- Every working pane claims every running project run. The first claimant wins even when a run has an exact provider-session match to another pane. Rendering checks the exact match first, but then accepts the wrong cached owner without applying the exact-owner exclusion.
- Reproduction with real components/store: A and B are working in the same project; one run has `rootSessionId: "provider-B"`. Mount A before B. The owner map recorded A, and **both A and B rendered the same run badge**.
- Fix: Resolve exact provider-session ownership before speculative claiming. Never let an unrelated working pane claim a run that explicitly belongs elsewhere. Test simultaneous panes, not only single-pane ownership.

### F12 - P2: A collapsed project header can hide a completed split pane

- References: `ui/src/components/Sidebar.tsx:621`, `ui/src/state/workspaceStore.ts:2183`, `ui/src/lib/activity.ts:141`.
- Project attention checks only `hasWaiting` and `hasUnread`, omitting `hasDone`. An unseen completion in a non-focused split pane of a visible tab intentionally has no unread-tab flag, while still contributing `hasDone` and a Dock count.
- Impact: The pane/tab/worktree can require attention while the collapsed project header has no completion indicator.
- Fix: Include unseen done in project attention resolution, preserving its precedence above working. Use the shared summary contract instead of a separate reduced predicate.

### F13 - P2: The settings permission badge treats fallback values as a real grant

- References: `src-tauri/src/notification/permission.rs:40`, `src-tauri/src/notification/permission.rs:165`, `ui/src/components/settings/NotificationsSection.tsx:150`.
- Windows/Linux fallback and unbundled macOS fallback return authorized/enabled with `authoritative: false`. The settings page derives its green status solely from authorization. In unbundled macOS, the same backend refuses submission with `notifications require a bundled .app`.
- Impact: Users see an apparently granted permission even when it was not queried, or while the current dev launch cannot submit banners.
- Fix: Render unsupported/unverified/dev-unavailable states explicitly using `supported`, `authoritative`, and runtime capability. Refresh authoritative status on return from OS settings rather than only on initial mount/request.

### F14 - P2: Normal sound deduplication is surfaced as an error

- References: `src-tauri/src/notification/audio.rs:62`, `ui/src/lib/notificationCoordinator.ts:103`, `ui/src/lib/notificationCoordinator.ts:182`, `ui/src/App.tsx:545`.
- The audio player intentionally collapses automatic sounds within 400 ms and returns `played: false, reason: "deduped"`. Both coordinator paths treat any non-played custom sound as an error.
- Reproduction with the real coordinator and a transport response shaped exactly like the backend dedupe result: `onError` received `Error: notification sound not played (deduped)`.
- Impact: Several agents finishing together generate misleading error toasts although sound suppression worked as designed.
- Fix: Distinguish expected suppression from missing files, decode/device errors, and native submission errors.

### F15 - P2: A request for approval is announced as task completion

- References: `ui/src/lib/notificationCoordinator.ts:148`, `ui/src/lib/notificationCoordinator.ts:194`, `src-tauri/src/notification/model.rs:360`.
- Both working -> waiting and working -> done use source `agent-task-complete`. Native formatting unconditionally produces `<agent> finished`; the request carries no next state.
- Impact: An agent blocked on approval is described as finished, sending the opposite action cue.
- Fix: Carry the attention reason through the DTO and format waiting as input/approval needed. Preserve actual completion wording for done.

## Contract questions and lower-confidence risks

- The master notification switch says it controls desktop notifications and audio. Pane frames currently remain enabled independently, but their setting switch is disabled by that master switch (`NotificationsSection.tsx:281`, `notificationSettings.ts:120`). Decide whether frames are independent; either keep their switch usable or gate both behavior and control. This review does not assume that turning off banners should disable in-app badges.
- Dock count uses `max(unseen attention sessions, unread tabs)` per workspace. This is the current documented policy, not a deduplicated union. If the number should count all attention targets, disjoint bell-only tabs and completed panes will undercount. Clarify the unit before changing it.
- Windows/Linux Dock badge updates explicitly return unsupported. That is a platform capability gap, not a failed macOS badge implementation.
- Native frame update is issued independently of asynchronous attach, with no reapplication tied to attach completion. A new-session attach ordering failure is a plausible residual risk, but this review did not reproduce it and does not count it as a confirmed finding.
- Desktop banners and sound are intentionally gated on whole-window focus, not merely a background tab. This policy is tested; changing it requires a product decision. BEL unread handling while the app remains focused is likewise currently suppressed.
- The remote waiting-only quick-jump control deliberately excludes done in its tests. Its lack of a done shortcut is not, by itself, classified as a bug here.

## Verified behavior and coverage

- Native title/BEL/agent-state listeners exist outside mounted pane components. Background tabs within the current workspace can report state, and parked project state is updated separately. The defects concern observation and downstream routing, not a blanket absence of native events.
- Screen-state detection is preferred over title classification. Non-status titles do not erase an in-flight screen-derived working state.
- Desktop activity precedence is waiting -> unseen done -> working -> bell unread. A seen done entry does not count as a new completion.
- Notification formatting normalizes control/whitespace and truncates text. Custom audio validates extension, file presence, and size before decoding; volume uses a 0-100 backend scale.
- Dispatch, permission query, and audio work use the blocking IPC boundary. macOS Dock updates are scheduled on the main thread.
- Native frame geometry supplies top/left/right bands; DOM supplies the bottom band and corner caps. Geometry was reviewed, not visually certified on an OS compositor.
- Auto-resume suppression does mark and consume the first attention event in the reducer. F07 explains why that is not an end-to-end notification fix.

## Verification evidence

### Existing frontend tests

Command:

```sh
bun run --cwd ui test \
  src/lib/notificationCoordinator.test.ts \
  src/lib/notificationSettings.test.ts \
  src/lib/notificationSoundWire.test.ts \
  src/lib/activity.test.ts src/lib/agentTitle.test.ts \
  src/state/screenActivity.test.ts \
  src/state/workspaceActivity.test.tsx \
  src/state/workspaceNativeActivity.test.tsx \
  src/state/activityStatePersistence.test.ts \
  src/state/activityRenderChain.test.tsx \
  src/state/worktreeActivityAcrossSwitch.test.ts \
  src/App.notifications.test.tsx \
  src/components/settings/NotificationsSection.test.tsx \
  src/components/Sidebar.activity.test.tsx \
  src/components/WorktreeList.test.tsx src/components/TabBar.test.tsx \
  src/components/TerminalSplitView.test.tsx \
  src/components/NativeTerminalPane.test.tsx \
  src/remote/RemoteAttention.test.tsx \
  src/components/dag/DagPaneBadge.test.tsx
```

Result: **20 files, 339 tests passed, exit 0**, one run using the project Vitest script. Native attach-error messages in the output came from deliberate failure-path cases; no test failed.

### Backend notification tests

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib notification -- --test-threads=1
```

Result: **93 passed, 0 failed, exit 0**. The build emitted 11 existing warnings, including unnecessary mut/unsafe and unused fields/helpers. No warnings were suppressed.

### Additional backend checks

Both targeted checks completed successfully: **21 agent detection tests** and **4 attention aggregation/frame tests** passed, with zero failures. The chained command exited 0.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib agent_detect -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib attention -- --test-threads=1
```

### Direct runtime probes

The lead imported the real TypeScript reducer, selectors, coordinator, adapter, and remote normalizer in Bun. For UI assertions it mounted real React components in an isolated JSDOM, using an intercepted IPC transport only where OS effects would otherwise occur. No user desktop, live terminal, actual audio device, or notification center was manipulated. The DOM and module-local DAG stores were cleaned up afterward.

Captured outputs:

```json
{ "waitingSeen": true, "actualDomAttentionFrames": 1, "tabIndicator": "waiting" }
{ "doneSeenBeforeTitle": true, "doneSeenAfterNonStatusTitle": false }
{ "parkedLastActiveTabCompletionSeen": true, "badge": 0 }
{ "selectedSound": "none", "ipcCommand": "cmd_notification_play_sound" }
{ "selectedSound": "system", "ipcCommand": "cmd_notification_play_sound" }
{ "suppressedResumeCompletionSeen": true, "notificationDispatches": 1 }
{ "focusedCompletedLeaf": "l", "seen": false, "badge": 1 }
{ "selectedCompletedWorktreeActiveTab": "a", "seen": false, "badge": 1, "unreadTabs": {} }
{ "dagProviderOwner": "B", "claimedOwner": "A", "panesShowingSameRunBadge": ["A", "B"] }
{ "remoteStates": ["working", "done"], "rollup": "working" }
{ "normalDedupeError": "notification sound not played (deduped)" }
```

These probes demonstrate existing failures; they are not passing regression tests for a fix. No new test files were introduced during this review.

### Why existing tests pass

- `screenActivity.test.ts` tests that the reducer can set waiting to seen, not that its frame disappears.
- `TerminalSplitView.test.tsx` covers waiting, unseen done, and seen done, but not seen waiting.
- `worktreeActivityAcrossSwitch.test.ts:140` explicitly expects a parked project's former active tab to be acknowledged. That expectation encodes F02 rather than catching it.
- `notificationSoundWire.test.ts` tests custom mode with a path and system mode without a path; it misses switching modes with a retained path.
- Coordinator unit tests mock the whole frontend Tauri adapter, so they cannot detect F04. Restore suppression tests do not cross the reducer -> App target -> coordinator boundary.
- Remote attention tests inject cross-worktree waiting data that production does not currently publish. Server tests preserve the older working-over-done ordering.
- Existing async React tests include `waitFor` polling in `workspaceNativeActivity.test.tsx`. They passed here, but event-based subscription fixtures would provide a stronger deterministic contract when this area is changed.

### Limits

- LSP symbol/diagnostic requests failed because the daemon was unreachable at `/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`. Source traversal and compiler/test results were used instead. No LSP-clean claim is made.
- No full application build was required for this read-only review. Cargo compiled the test target; no release build was made.
- No desktop automation was performed. The isolated DOM probe proves rendered element presence, not WGPU/compositor pixels. An attempted server render was unsuitable because `DagPaneBadge` uses `useSyncExternalStore` without a server snapshot; client rendering in JSDOM was used instead.
- Actual macOS banner delivery, sound output, Dock appearance, Windows notifications, and Linux desktop behavior were not manually verified. The authorized desktop launch path remains exactly `bun tauri dev`; no alternate binary or app bundle was created. Native macOS banner submission is explicitly unavailable in the current unbundled dev runtime.
- Other sessions edited files during the review. Existing tests passed on the tree at their execution time, and source predicates used for the final findings were rechecked. This is not a frozen-tree release certification.

## Proposed repair order

1. Unify observation and acknowledgment across mounted/parked projects, focus changes, pane navigation, and worktree selection. Lock F01/F02/F05/F06/F12 with real reducer-to-component regressions.
2. Make notification events workspace-aware and carry suppression plus attention reason. Cover F03/F07/F15 through the App/coordinator boundary.
3. Make sound mode explicit end to end, observe native submission completion, and distinguish dedupe from failure. Cover F04/F08/F09/F13/F14.
4. Align the remote published attention contract and exact DAG ownership. Cover F10/F11.

Product decisions to confirm while implementing: whether acknowledged waiting retains a quiet status indicator, whether pane highlighting is independent of the banner master switch, and whether the Dock number is a max count or a union count. These decisions do not block fixing missing notifications, revived acknowledgment, ignored mute, or wrong DAG ownership.

## Manual checks requested after fixes

- In `bun tauri dev`, finish or block agents in another tab, another split pane, another worktree, and another project. Verify each surface identifies the same unseen target.
- Dismiss waiting/done with mouse, keyboard pane navigation, and sidebar selection. Repaint the agent title and confirm attention does not return without a new activity cycle.
- Test custom -> mute and custom -> system transitions. Test several simultaneous completions without false dedupe-error toasts. Native macOS banner/system-sound checks require a separately authorized supported runtime; dev source cannot prove them.
- With two active agent panes, verify a DAG with an explicit provider-session owner is shown only on that pane.
- On the mobile remote client, verify another worktree/project can report attention and that unseen done is not hidden behind working.

## Completion addendum

All requested review work and started checks are complete. Existing tests passed: **339 frontend + 93 notification + 21 agent detection + 4 attention = 457 tests**. The direct runtime probes still reproduced the defects documented above. The report is uncommitted; no application or test source was changed by this review.
