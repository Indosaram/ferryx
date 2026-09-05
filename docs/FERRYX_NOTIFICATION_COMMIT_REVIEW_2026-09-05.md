# Notification implementation commit review

Date: 2026-09-05
Request: Re-review the F01-F15 implementation and commit its changes.
Base inspected: `7551b3c` on `main`.

## Scope and attribution

- Reviewed the notification, acknowledgment, remote attention inventory, and DAG
  ownership diffs against the original F01-F15 findings.
- All 34 source/test SHA-256 hashes in
  `evidence/notification-fixes/verification.json` match the files tested and
  captured earlier in this session. The existing QA is current for this scope.
- `src-tauri/Cargo.toml` adds the required `UNNotificationSound` feature.
- `ui/src/components/TerminalSplitView.tsx` also contains another session
  changing pane-to-tab drag previews. Only the waiting/done `!activity.seen`
  condition belongs to this commit scope; the drag changes stay unstaged.
- Native selection/input, terminal lifecycle, scoped features, store packaging,
  and their documentation are unrelated shared-tree work and are excluded.

## Evidence reused and refreshed

- Reused related frontend regression run: 23 files, 456 passing tests.
- Reused Rust tests: 99 notification, 1 remote inventory, and 1 authenticated
  HTTP worktree-attention test passed.
- Reused the passing TypeScript/Vite build and five production-component WebKit
  captures. No product source changed after the recorded captures.
- Refreshed LSP diagnostics: all 34 scoped source/test files returned no errors.
  This supersedes the earlier unavailable-daemon result for this checkpoint.
- `cargo build --manifest-path src-tauri/Cargo.toml` passed with exit code 0:
  `Finished dev profile [unoptimized + debuginfo] target(s) in 13m 37s`.
  This supersedes the earlier cancelled standalone-build attempt. It is a build,
  not evidence of a visible desktop window or native notification delivery.

## Final review verdict and commits

The independent gate reviewer (`st_01a071e3`) returned **APPROVE**. Lead review
found no additional blocking defect in F01-F15. No extra product changes were
needed during this re-review.

- `a52042f` - `fix(notification): align attention and delivery across workspaces`
  includes the core cross-layer implementation and direct regression tests.
- `ea85021` - `fix(dag): honor exact provider session ownership` contains the
  independent DAG ownership correction and its sibling-pane regression.
- This document, the original findings, repair reports, screenshots, and recorded
  source hashes are kept together in a separate documentation commit.

The sound-path review distinguished F04 (never replay the retained custom file
in system/mute mode) from F08 (macOS system sound). The existing non-macOS plugin
does not consume the new sound field; Windows defaults to silent and Linux omits
sound hints. No new guarantee about system sounds or local notification-daemon
rules on those platforms is claimed.

The working tree still contains unrelated work. In particular, the DND changes
in `TerminalSplitView.tsx` remain outside these commits. No push was requested.

## Verification limits

The five browser views demonstrate emitted IPC arguments, permission state,
remote context selection, and exact DAG ownership with isolated fixtures. They
do not prove actual speakers, OS banners, Dock pixels, or WGPU compositing. The
remote chooser obscures the terminal-tab strip. The denied-permission screenshot
retains a successful test message from before the permission change.

Miri cannot execute the macOS `objc_getClass` foreign call. No Miri pass or
Windows/Linux runtime pass is claimed. Desktop manual verification remains the
checklist in `FERRYX_NOTIFICATION_FIXES_2026-09-05.md`; the authorized desktop
launch remains exactly `bun tauri dev`, debug only.
