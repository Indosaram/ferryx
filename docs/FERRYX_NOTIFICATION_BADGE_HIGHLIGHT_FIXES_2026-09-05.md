# Notification, badge, and highlight fixes

Date: 2026-09-05
Scope: F01-F15 in [the review](FERRYX_NOTIFICATION_BADGE_HIGHLIGHT_REVIEW_2026-09-05.md).
Status: All fifteen source fixes are implemented. Verification results and limits are distinguished below; this is not an OS-delivery or release certification.

## Implemented changes

| Finding | Change | Regression coverage |
|---|---|---|
| F01 | Waiting and done require `seen !== true` for dismissible summaries and pane frames. | Activity, acknowledgment, and real split-view component tests. |
| F02 | Mounted activity receives actual window-observation context; parked projects are always unobserved. | Native activity and acknowledgment tests. |
| F03 | Activity edges and BEL route with owning-workspace metadata, including parked project snapshots. | Workspace native events and App/coordinator tests. |
| F04 | Retained custom paths no longer override system or mute selection. Test and automatic dispatch carry explicit native sound mode. | Sound-wire, coordinator, settings tests; browser interaction trace. |
| F05 | Metadata/title refreshes preserve acknowledgment within an attention episode. A new work cycle can still request attention. | Acknowledgment and native activity tests. |
| F06 | Pane focus and worktree selection acknowledge the selected session, without clearing every hidden tab. | Reducer/navigation regressions. |
| F07 | Auto-resume suppression is preserved through notification targets and the coordinator. | App/coordinator suppression regressions. |
| F08 | The request models system/silent sound; macOS content sets default sound only for system mode. | Native notification content tests and frontend wire tests. |
| F09 | macOS submission observes its completion callback and reports rejection or unavailable completion instead of unconditional success. The wait is bounded. | Native callback success/rejection/missing-completion tests. |
| F10 | Published inventory includes parked worktrees/projects and acknowledged-attention semantics. Server and remote UI use waiting > unseen done > working. | App publication, remote normalizer, Rust inventory, and authenticated HTTP tests. |
| F11 | Exact provider-session ownership overrides speculative DAG claims, and a pane cannot render another pane's exact match. | Simultaneously mounted sibling-pane regression; browser badge counts. |
| F12 | Collapsed project attention includes unseen done, not only waiting/unread. | Sidebar activity regression. |
| F13 | Non-authoritative permission is shown as unknown, unavailable runtime as unavailable; returning focus refreshes status. | Settings component regression and browser focus event. |
| F14 | Intentional `deduped` audio results no longer produce error toasts; other failures remain errors. | Coordinator regressions. |
| F15 | Notification requests distinguish waiting from done so approval/input requests are not announced as completion. | DTO/formatting and coordinator regressions. |

The implementation retains the existing whole-window notification-focus policy, waiting-only remote quick-jump policy, independent in-app attention setting, and Dock max-count policy. Those product choices were not silently changed.

## Automated verification

### Frontend

The related regression command completed in one run: **23 files, 456 tests passed, exit 0**.

```sh
CI=1 bun run --cwd ui test \
  src/lib/notificationCoordinator.test.ts src/lib/notificationSettings.test.ts \
  src/lib/notificationSoundWire.test.ts src/lib/activity.test.ts src/lib/agentTitle.test.ts \
  src/state/screenActivity.test.ts src/state/workspaceActivity.test.tsx \
  src/state/workspaceNativeActivity.test.tsx src/state/activityStatePersistence.test.ts \
  src/state/activityRenderChain.test.tsx src/state/worktreeActivityAcrossSwitch.test.ts \
  src/state/attentionAcknowledgment.test.ts src/App.notifications.test.tsx src/App.test.tsx \
  src/components/settings/NotificationsSection.test.tsx src/components/Sidebar.activity.test.tsx \
  src/components/WorktreeList.test.tsx src/components/TabBar.test.tsx \
  src/components/TerminalSplitView.test.tsx src/components/NativeTerminalPane.test.tsx \
  src/remote/RemoteAttention.test.tsx src/remote/attentionInventory.test.ts \
  src/components/dag/DagPaneBadge.test.tsx
```

`bun run --cwd ui build` subsequently passed, including TypeScript checking and the Vite production bundle. An earlier attempt failed at an unrelated, concurrently edited `ui/src/features/ferryx/control/client.ts:5` (`TS2550`, `replaceAll` versus the configured target). Its owner changed that file; this task did not edit it. The successful later build supersedes that failure.

### Rust

The full application test target compiled and these commands passed, exit 0:

```sh
cargo test --offline --manifest-path src-tauri/Cargo.toml --lib notification -- --test-threads=1
# 99 passed
cargo test --offline --manifest-path src-tauri/Cargo.toml --lib remote_attention_inventory -- --nocapture
# 1 passed
cargo test --offline --manifest-path src-tauri/Cargo.toml --lib test_workspace_state_agent_activity_and_worktree_attention_rollup -- --nocapture
# 1 passed
```

The HTTP test runs an authenticated Axum listener against a temporary repository and verifies attention on the non-selected primary worktree, not just the selected feature worktree. The separate inventory test covers another project. Five omitted `attention_inventory` initializers in existing Rust test fixtures initially blocked compilation; all five were corrected.

An earlier isolated harness imported the production notification files directly and passed **70 tests**. These overlap the full notification tests above and must not be added as unique coverage.

Existing Rust warnings were not suppressed. Full remote-suite and debug-build outcomes are recorded in the completion addendum.

## Real browser/component exercise

The lead drove actual React components in Bun.WebView with existing Tailwind styling. Only the Tauri IPC boundary was replaced to avoid touching the user's live desktop, notification center, or speakers. This verifies component behavior and emitted arguments, not native delivery.

Observed interactions:

- Opened the real sound dropdown and selected mute with `/fixture/retained.wav` still saved. Clicking Send Test Notification emitted `cmd_notification_probe_delivery` with `{ sendTest: true, sound: "silent" }`; no custom-audio command was emitted.
- Selected System Default and clicked the test button. The command used `sound: "system"`; the retained custom path still did not play.
- Changed simulated permission to authoritative denied and emitted a focus event. The rendered badge changed from `unknown` to `denied`.
- Clicked the waiting Project B worktree in the mobile selector. The selection callback identified Project B / main, not Project A.
- Mounted the real DAG badge on two sibling panes for a run whose root provider session belongs to B. The DOM contained **0 badges in A and 1 in B**. The screenshot proves placement; the store-level ownership assertion is separately covered by the regression test.

### Captures

All five artifacts have valid PNG signatures and the requested dimensions:

| Artifact | Dimensions | Evidence |
|---|---|---|
| [Custom / unknown](evidence/notification-fixes/settings-custom-unknown.png) | 1280 x 1000 | Non-authoritative permission, retained custom path. |
| [Mute](evidence/notification-fixes/settings-mute.png) | 1280 x 1000 | Mute selected; custom controls absent. |
| [System / denied](evidence/notification-fixes/settings-system-denied.png) | 1280 x 1000 | System selected; refreshed denied label/icon. |
| [Remote inventory](evidence/notification-fixes/remote-inventory-390.png) | 390 x 844 | Completed worktree, waiting other project, readable Korean label. |
| [DAG exact owner](evidence/notification-fixes/dag-exact-owner.png) | 1280 x 720 | Badge appears only on B. |

Two independent read-only reviewers opened every capture and returned **PASS for the bounded component surface**, with no blocking findings. Neither review certifies the complete desktop application. The reviews identified these evidence limits:

- The denied capture retains earlier successful test feedback; it is not a denied-delivery test. The fixture returned submitted for the earlier test. A missing banner cannot be diagnosed solely as Focus/Do Not Disturb from this image.
- Label/icon changes are demonstrated; the captured denied tint does not establish final desktop theme colors.
- Remote selection was observed at its callback, not as a completed live desktop context switch. Its open selector covers the underlying tab strip.
- The existing mobile selector's small top offset exposes a sliver of tab chrome. This is cosmetic, pre-existing, and outside the notification fix.

## Limits and remaining manual checks

- **No actual macOS banner, default sound, custom speaker output, or Dock pixels were manually certified.** Unbundled macOS dev runtime cannot submit native banners; the implementation does not disguise that limitation.
- Miri was attempted for the native rejection boundary and stopped at `unsupported operation: can't call foreign function objc_getClass on OS macos`. This is an unsupported interpreter boundary, not a passing memory-safety result. Native tests passed outside Miri.
- Windows/Linux code paths are isolated behind existing platform boundaries, but were not executed on those operating systems.
- No release bundle, app replacement, daemon restart, or live terminal manipulation was performed for this verification.
- LSP was intermittently available, then became unreachable/time-limited. Compiler/test outcomes are independent evidence; no blanket LSP-clean claim is made.
- At this initial handoff the fixes and evidence were uncommitted. A subsequent
  user request authorized re-review and commits; see the commit addendum below.

Manual acceptance on an authorized desktop runtime remains:

1. Finish/block agents in another split pane, tab, worktree, and project while the app is focused and unfocused. Compare highlights, navigation indicators, and Dock count.
2. Read waiting/done via mouse, keyboard pane navigation, and worktree selection. Repaint the title and confirm attention stays dismissed until a new work cycle.
3. Exercise custom -> mute -> system, simultaneous completions, and OS-denied delivery. Confirm no stale custom sound, false dedupe toast, or approval-as-completion message.
4. Verify native macOS accepted/rejected submissions in a supported bundle and check Windows/Linux behavior separately.
5. Verify remote project/worktree selection against a live desktop and a two-agent DAG against actual provider sessions.

## Completion addendum

- Frontend: 456 related tests passed; full TypeScript/Vite build passed.
- Rust: full library test target compiled; 99 notification, 1 cross-project inventory, and 1 authenticated HTTP attention test passed.
- Additional `cargo build --offline` and the full remote Cargo suite were started, but remained behind shared package/build locks. These two extra commands were stopped without changing any other session process. They are **not** passing build/suite evidence.
- A further attempt to run the `remote` filter directly from the compiled test executable produced no test output before being stopped. No full-remote-suite pass is claimed. A process sample also produced no usable stack report. These interrupted checks do not supersede the successful focused Cargo runs above.
- Final per-file LSP requests covered 34 notification-related source/test files; all 34 returned an unreachable-daemon error. Earlier individual diagnostics were intermittently successful.
- Both independent visual reviewers passed the five captured component states, with the limitations above.
- The temporary browser fixture and isolated Cargo harness are absent from the working tree. Permanent screenshots, this report, and [machine-readable results/source hashes](evidence/notification-fixes/verification.json) remain. The recorded 34 source hashes were rechecked with no changes, and `git diff --check` passed.

## Commit addendum

[The subsequent commit review](FERRYX_NOTIFICATION_COMMIT_REVIEW_2026-09-05.md)
records independent approval, all 34 scoped LSP checks without errors, and a
successful debug application build (exit 0, 13m 37s). Those new checks supersede
the earlier unavailable LSP and cancelled standalone build results, but not the
native runtime and full remote-suite limitations. Implementation commits are
`a52042f` and `ea85021`; unrelated shared-tree changes were excluded.
