# Notification fixes: final code gate review

Date: 2026-09-05
Request: Review the F01-F15 implementation and commit the related changes.
Verdict: **APPROVE. No blocking code findings.**

## Scope and review method

One read-only code gate reviewer (`st_01a071e8`) audited the original
[F01-F15 findings](FERRYX_NOTIFICATION_BADGE_HIGHLIGHT_REVIEW_2026-09-05.md),
the scoped implementation diff against `7551b3c`, the affected source and tests,
and the lead's existing browser/component evidence. The reviewer reported no
index mutations, commits, or source edits. Earlier screenshot-only reviews are
separate from this code gate.

The 34 recorded source/test hashes still matched at the final review checkpoint,
so the existing component QA remained applicable. The required
`UNNotificationSound` Cargo feature is also included in the implementation.
Only the attention predicate in `TerminalSplitView.tsx` belongs to this scope;
the other session's pane-to-tab drag changes were not part of the reviewed fix.

## Resolution summary

- F01, F02, F05, F06, F12: unseen waiting/done summaries and frames now agree;
  observation includes real window/project context; title refresh preserves
  acknowledgment; navigation reads the selected session; collapsed projects
  expose unseen done.
- F03, F07, F15: parked-project activity and BEL reach the owner-aware
  coordinator; restore suppression survives the dispatch boundary; waiting
  requests are distinguished from completion notifications.
- F04, F08, F09, F13, F14: saved custom paths no longer override mute/system
  mode; macOS content explicitly selects system/silent sound; submission waits
  for a bounded native callback; permission fallback is not shown as a grant;
  expected audio deduplication is not surfaced as an error.
- F10, F11: remote inventory includes parked contexts with waiting > unseen
  done > working priority; exact provider-session DAG ownership overrides
  speculative ownership and excludes the unrelated pane.

## Accepted verification evidence

- Frontend regression: 23 files, 456 passing tests in one run.
- Rust focused regression: 99 notification tests, 1 remote inventory test, and
  1 authenticated HTTP attention test passed; the full library test target
  compiled.
- Full TypeScript/Vite build passed.
- The lead drove production React components in Bun.WebView with only Tauri
  IPC replaced: mute/system wire arguments, permission focus refresh, Project B
  selection callback, and two-pane DAG placement (A: 0 badges; B: 1 badge).
- Five PNGs, exact commands, source hashes, and manual procedures are in the
  [implementation report](FERRYX_NOTIFICATION_BADGE_HIGHLIGHT_FIXES_2026-09-05.md)
  and [machine-readable evidence](evidence/notification-fixes/verification.json).
- The gate reviewer returned APPROVE with no blocking issues. Source and
  staged diff whitespace checks passed during the commit handoff.

## Residual limits, not blockers for this code review

- Windows/Linux currently submit through the existing notification plugin
  without forwarding `NotificationContent.sound`. Silent/custom mode therefore
  does not guarantee suppression of an OS chime on those platforms. The source
  establishes this adapter limitation, not an impossibility in the plugin API.
- A DAG run without a known provider-session match still uses first-claim
  ownership until an explicit match becomes available.
- Real OS banners, speaker output, Dock/native compositing, live remote desktop
  switching, and Windows/Linux execution were not certified by the component
  evidence. The denied screenshot is not a native rejection-delivery test.
- Miri stopped at unsupported `objc_getClass`; no Miri pass is claimed. The
  full remote suite did not produce a completed pass. Earlier LSP/build limits
  and subsequent checks reported by the concurrent process remain separately
  attributed in the implementation and commit-review reports.
- Any authorized desktop check must use exactly `bun tauri dev`, debug only.
  No app/daemon replacement or desktop automation was performed by this lead.

## Commit attribution and shared-tree boundary

During this gate review another live process created these local commits:

- `a52042f`: `fix(notification): align attention and delivery across workspaces`.
  Contains the notification implementation, direct tests, and required native
  sound feature.
- `ea85021`: `fix(dag): honor exact provider session ownership`.
  Contains the three reviewed DAG files. Its actual commit also contains two
  concurrently staged rendering-review evidence files:
  `docs/audits/TAURI_FRONTEND_RENDERING_SESSION_REREVIEW_2026-09-05.md` and
  `docs/evidence/rendering-review-2026-09-05/session-rereview-runtime.json`.
  Those artifacts are outside this notification review. This lead neither
  created that mixed commit nor rewrote its history.

The implementation was not duplicated. This final gate report is committed by
an explicit path-only operation so unrelated shared index entries remain with
their owner. Other dirty source and documentation work remains outside this
task; no push was requested.
