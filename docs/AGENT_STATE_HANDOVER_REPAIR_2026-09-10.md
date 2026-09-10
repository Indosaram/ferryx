# Agent state delivery across daemon handover

Date: 2026-09-10

## Scope and outcome

Implemented the approved daemon state-delivery repair and quiet frontend restoration.
The changes are in the main working tree, uncommitted. No installed application bundle
was replaced and no live user daemon was terminated or explicitly upgraded.

The initial live investigation found 24 sessions behind canonical PID 65513, of which
21 belonged to predecessor daemons. A connection without a payload to agent-state.sock
confirmed that reports went to the canonical daemon, while legacy Attach streams read
their reports from predecessor broadcasts. The missing merge explained why PTY output
survived while agent activity stopped updating.

## Implementation

- `src-tauri/src/daemon/agent_state.rs` retains the latest report per session and
  subscribes under the same mutex used for publication. A report is therefore either
  in the attach snapshot or in the live receiver queue.
- `src-tauri/src/daemon/server.rs` publishes extension reports through that hub,
  subscribes before attach work, emits an initial snapshot, and removes retained
  state on explicit close and local process exit.
- `src-tauri/src/daemon/proxy.rs` merges canonical reports into predecessor output
  streams. Predecessor reports are quiet baseline candidates, not new completion
  edges. Once a baseline or canonical report is known, predecessor backlog cannot
  replace it. Stream reads use cancellation-safe `Lines::next_line`.
- Lag recovery atomically replaces the receiver and takes a current snapshot.
  It does not emit a current snapshot and then replay the obsolete queued states.
- `DaemonStreamMessage::AgentState` carries optional `isSnapshot`. Old messages
  default to false; ordinary live messages omit the field.
- The native terminal bridge forwards the snapshot marker even when its native
  state is unchanged. A newly mounted frontend still needs that restoration event.
- The workspace reducer applies snapshots in mounted and parked workspaces.
  Working restores the running indicator. Idle and blocked restore quiet, seen,
  notification-suppressed attention states, without adding unread state. The next
  live working-to-idle or working-to-blocked transition behaves normally.
- Existing App/native/render-chain test mocks now provide the runtime exports
  required by the current code, rather than crashing before the behavior assertions.
- `ui/activity-qa.html` exposes the existing real-component activity harness with
  snapshot controls and the production badge selector.

This does not add heartbeat-based inference or change the permanent self-report
priority rule. Reports already lost by an old daemon cannot be reconstructed
retroactively. An old producer without retained state becomes accurate after its
next real report reaches a patched canonical daemon.

## Verification

Work was isolated at `/private/tmp/ferryx-agent-state-20260910` because an existing
`bun tauri dev` process watched the main working tree. Existing dependency versions
were retained with the main Cargo.lock. The final 14 source/test/harness files were
transferred with file patches; SHA-256 comparison confirmed exact equality.

### RED evidence

- The existing debug binary underwent a real isolated rolling handover and then
  timed out waiting for an agent state through the canonical Attach stream.
  Its temporary runtime and QA daemons were cleaned up.
- Frontend snapshot tests failed on discarded initial idle state, missing quiet
  restoration, and parked workspace restoration before the implementation.
- The native pump test failed with snapshot marker `false` instead of `true`.
- Additional hub tests failed because predecessor backlog was treated as a live
  completion and because obsolete queued states remained after resynchronization.

### GREEN evidence

Commands below ran in the isolated tree. Rust commands used `--locked`.

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib agent_state -- --nocapture
# 18 passed

cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  extension_reported_state_wins_over_screen_inference -- --nocapture
# 1 passed

cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  agent_state_snapshot_flag -- --nocapture
# 2 passed, including old-frame compatibility

cargo build --locked --manifest-path src-tauri/Cargo.toml --bin ferryx
# exit 0
```

Frontend verification used the Node Vitest runner:

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/state/screenActivity.test.ts \
  src/state/attentionAcknowledgment.test.ts \
  src/state/workspaceNativeActivity.test.tsx \
  src/state/worktreeActivityAcrossSwitch.test.ts \
  src/lib/notificationCoordinator.test.ts \
  src/App.notifications.test.tsx \
  src/state/activityRenderChain.test.tsx \
  src/state/activityStatePersistence.test.ts \
  --maxWorkers=1 --reporter=dot
# 8 files, 91 tests passed

bun run build
# TypeScript and Vite: exit 0
```

The isolated debug executable was copied to `qa-bin/ferryx`, signed with
`Developer ID Application: Indo Yoon (5DUM8WPB4C)`, and passed
`codesign --verify --strict`. Signing inside the copied Cargo output initially
failed with "unsealed contents present in the bundle root"; signing the standalone
QA executable succeeded. No app bundle was created.

```sh
bun scripts/verify-daemon-handover.mjs qa-bin/ferryx baseline-ferryx --agent-state
bun scripts/verify-daemon-handover.mjs qa-bin/ferryx qa-bin/ferryx --agent-state
```

Both commands exited 0. Each verified two generations and preserved original PTYs,
history, executed input, and resize. Agent verification covered ordered
working/blocked/idle delivery, burst transitions, and quiet idle snapshots on
reattach. The first scenario used an unpatched predecessor. All QA runtime
directories and their daemon processes were cleaned up by the verifier.

In a separate headless Chromium session, the real workspace reducer, TabBar, and
WorktreeList produced:

- Snapshot working: running state, badge 0.
- Snapshot idle: done, seen, notification suppressed, badge 0.
- Subsequent live completion: unseen done, suppression cleared, badge 1.
- Snapshot blocked at 390px width: seen waiting, badge 0.
- Subsequent live input request: unseen waiting, badge 1.

The 390px DOM bounds placed the tab bar at x=16..374 and the worktree list at
x=16..336 without overlap. Screenshots were captured, but this session's image
reader could not display them, so this is not a visual-pixel approval.
The headless browser and the isolated Vite server were closed.

`git diff --check` passed. LSP diagnostics were unavailable because the shared LSP
daemon socket was unreachable. Compiler and TypeScript checks were used instead.
Existing unrelated Rust warnings in macOS file-drop, input, and font-manager code
were not suppressed or changed.

## Runtime rollout and manual checks

The verified binary is a QA artifact, not an installed release. These results do
not establish that the user's current canonical daemon is executing the new code.
Do not kill predecessor daemons to activate the fix: they still own live PTYs.

After separately applying the patched debug app and daemon through the supported
process-preserving update path, launch the desktop only with `bun tauri dev` and
manually verify:

1. Start a real agent turn in an existing, pre-update terminal and switch to another
   tab or project. Check that running state continues to update.
2. Complete the turn while Ferryx is unfocused. Confirm one OS notification,
   the expected Dock badge, and the tab/worktree/pane attention indication.
3. Focus the completed pane. Confirm its attention clears without acknowledging
   an unrelated sibling pane.
4. Reconnect the GUI while an agent is working, then while it is idle. Confirm
   current state restores without a new completion notification.
5. Run another turn after restoration. Confirm completion and input-request
   notifications still work.

OS banner delivery, Dock pixels, native attention-frame pixels, notification
click navigation, and Windows/Linux native execution were not exercised in this
implementation pass.
