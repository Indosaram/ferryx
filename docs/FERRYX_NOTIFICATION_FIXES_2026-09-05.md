# Notification, badge, and highlight repairs

Date: 2026-09-05
Scope: F01-F15 in `FERRYX_NOTIFICATION_BADGE_HIGHLIGHT_REVIEW_2026-09-05.md`.
Status: All fifteen code repairs implemented. Automated and isolated browser
verification are described below; native desktop delivery remains unverified.

## Repairs

| Findings | Change | Regression coverage |
| --- | --- | --- |
| F01/F05 | Acknowledged waiting/done no longer contributes attention; metadata/title refresh preserves acknowledgment within the episode. | activity, attentionAcknowledgment, TerminalSplitView |
| F02/F06 | Parked workspaces are never observed; native/window focus supplies observation context; pane/tab/worktree selection acknowledges only the selected target. | workspaceNativeActivity, attentionAcknowledgment |
| F03/F07 | Workspace-owned activity and BEL events reach the coordinator; unread routing returns to the owner; synthetic restore suppression crosses the notification boundary. | workspaceNativeActivity, App.notifications, notificationCoordinator |
| F04/F08 | Custom, system, and mute are explicit. Retained custom paths cannot override system/mute. macOS content requests the default sound only in system mode; settings probe carries the same mode. | notificationSoundWire, NotificationsSection, notification native-content test |
| F09 | macOS submission waits for its completion callback, propagates rejection, and fails after a bounded missing-callback timeout. Submission is not proof of visible delivery. | macos_submission tests |
| F10 | Publish mounted/parked worktree and project attention inventory, filter acknowledged attention, and use waiting > unseen done > working on both sides. Fill every Rust DTO initializer. | App, attentionInventory, Rust remote inventory, real HTTP worktree rollup |
| F11 | Exact provider-session ownership overrides speculative DAG claims; a different exact owner excludes the badge from a pane. | DagPaneBadge simultaneous panes |
| F12 | Collapsed project headers include unseen split-pane completion. | Sidebar.activity |
| F13 | Nonauthoritative permission is unknown rather than a green grant; unsupported is unavailable; returning focus refreshes status. | NotificationsSection |
| F14 | Expected audio `deduped` is not an error toast. | notificationCoordinator |
| F15 | Native notification payload carries waiting versus done; approval/input requests are no longer formatted as completion. | notification model/coordinator |

No policy changes were made to the independent pane-frame setting, Dock max-count
policy, whole-window notification focus gate, or remote waiting-only quick jump.

## Automated evidence

- Frontend related regression suite: **23 files, 456 tests passed**, one Vitest run
  through `bun run --cwd ui test`. Covers coordinator/settings/wire, activity/title,
  workspace state/render/persistence, App, Sidebar/WorktreeList/TabBar, terminal
  panes, RemoteAttention/inventory, and DAG pane badges.
- Full Rust library target compiled; `cargo test --manifest-path src-tauri/Cargo.toml
  --lib notification -- --test-threads=1`: **99 passed**.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote_attention_inventory
  -- --nocapture`: **1 passed**.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib
  test_workspace_state_agent_activity_and_worktree_attention_rollup -- --nocapture`:
  **1 passed**, including real authenticated HTTP state retrieval and attention
  for a worktree different from the active desktop selection.
- `bun run --cwd ui build`: **passed**, including TypeScript and Vite production
  bundle. An earlier run failed on another session's untracked
  `features/ferryx/control/client.ts` ES library mismatch; this session did not
  change that file, and the current build passes.
- A separate `cargo build --manifest-path src-tauri/Cargo.toml` attempt remained
  blocked behind concurrent Cargo jobs on the shared target directory for over
  six minutes. Only this session's waiting build was cancelled; other sessions'
  jobs were left untouched. The standalone application build is **not verified**;
  the full Rust library test binary did compile and execute successfully above.
- `git diff --check`: passed.
- Final LSP calls across UI, notification, remote, and changed IPC files could not
  reach the LSP daemon after the harness restart. Earlier remote-tests diagnostics
  were clean. No blanket LSP-clean claim is made.
- Miri was attempted against the real macOS rejection callback test. It cannot
  call the foreign `objc_getClass` operation on macOS; **Miri is not a passing
  proof**. Native callback/content tests passed under the normal Rust runtime.
- Cargo warnings remain visible (native input variable, font buffer mutability,
  notification IPC app field, and terminal/worktree dead code). They were not
  suppressed or changed as unrelated cleanup.

## Browser evidence and limits

Fresh WebKit renders imported production React components, actual Tailwind styles,
and the real frontend Tauri adapter. Only the external OS transport was intercepted.
The browser was isolated from the user's application/storage. This is component
surface verification, **not** full Tauri/native compositor or remote daemon E2E.

All images are PNGs with verified signatures and dimensions:

| Capture in `evidence/notification-fixes/` | Size | Evidence |
| --- | --- | --- |
| `settings-custom-unknown.png` | 1280x1000 | Retained custom path; nonauthoritative status shown as unknown. |
| `settings-mute.png` | 1280x1000 | Real dropdown mute selection; test sends `sound: silent`, with no audio IPC. |
| `settings-system-denied.png` | 1280x1000 | System mode sends `sound: system`; subsequent focus refresh shows denied in destructive color. |
| `remote-inventory-390.png` | 390x844 | Project A unseen done outranks working; Project B waiting visible; Korean label fits. |
| `dag-exact-owner.png` | 1280x720 | Pane A has zero badges, B one; owner map is B. |

Observed interactions:

- Custom -> mute -> system with `/fixture/retained.wav` retained: only native
  probe calls with silent/system, never custom-audio playback.
- Focus refresh: `unknown` -> `denied`; final computed text color
  `rgb(255, 101, 104)` after finite CSS transitions completed. The retained test
  success message predates the permission change, not a successful denied send.
- Selecting Project B/main waiting returned that precise context. Remote
  `scrollWidth == innerWidth == 390`; one done and one waiting worktree indicator.
- DAG exact-owner DOM counts A=0, B=1. The final inset fixture captures the whole
  badge at x=1191..1211, y=612..632 within 1280x720.

Capture repairs: the first DAG image put the badge on the frame boundary, and
the first denied image was mid color transition. Only the QA framing/wait was
corrected; no product code was changed to manufacture a visual pass.

Two independent initial reviewers found only those evidence issues. A fresh
reviewer opened all five final captures and returned **PASS**, with no product or
evidence blockers in the scoped browser views. The open remote chooser obscures
the terminal-tab strip; that strip is not visually certified by these captures.
Temporary browser entry files and the isolated Rust harness were removed after
verification; the private Vite server and WebViews were closed.

Actual OS banners, speakers, Dock pixels, WGPU attention bands, and Windows/Linux
desktop effects were not manually exercised. No running GUI or daemon was stopped,
no release application was replaced, and no PTY sessions were deliberately disturbed.

## Remaining supported-runtime manual checklist

1. In the authorized `bun tauri dev` launch, complete/block an agent in another
   tab, split pane, worktree, and project while focused/unfocused. Check consistent
   unseen state across pane/tab/sidebar/Dock.
2. Dismiss by mouse, keyboard pane navigation, and worktree selection. Refresh
   the title; attention must not reappear until a genuinely new work cycle.
3. In a separately authorized bundled macOS runtime, verify system/custom/mute
   output and native rejection reporting. Unbundled dev cannot submit native
   macOS banners. Accepted submission does not prove display under Focus/DND.
4. Trigger simultaneous completions and confirm expected sound dedupe is quiet.
5. Verify actual mobile remote attention across projects/worktrees and exact DAG
   ownership with two real providers. Browser fixtures do not certify live PTYs.

At the initial verification handoff, these changes were uncommitted. The user
subsequently requested re-review and commits. See
[the commit review](FERRYX_NOTIFICATION_COMMIT_REVIEW_2026-09-05.md) for the
approved verdict, implementation commits, refreshed LSP results, and successful
debug application build. Unrelated shared-tree changes remain uncommitted.
