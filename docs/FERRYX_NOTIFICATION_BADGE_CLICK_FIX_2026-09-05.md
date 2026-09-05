# Notification badge and click follow-up

Date: 2026-09-05. Follow-up to the F01-F15 implementation.

User reports: the application badge still does not appear, and clicking an OS
notification does not select its originating pane. The earlier source and
component checks did not establish either native OS outcome.

## Confirmed defects

- macOS explicit authorization requested Alert and Sound, but omitted Badge.
  The production authorization-options function returned bitmask 6. A failing
  assertion for Badge reproduced this omission; after the fix it returns 7,
  exactly Alert | Sound | Badge. No OS permission prompt was invoked by QA.
- Notification requests had no workspace/frontend-session target, and no native
  response delegate or frontend activation subscription routed a click. An
  ordinary window-focus event cannot identify which notification was clicked.

The permission omission is a source defect, not proof that it alone explains
the installed application's Dock pixels. Whether requesting an option superset
updates an already-authorized macOS installation has not been verified. The
claim that such an update is ignored was an unverified inference and is not
used as a conclusion here.

## Permission recovery verification

- `NotificationsSection` keeps its existing Request Permission action visible
  for authorized/provisional as well as unknown/denied states. Unsupported
  platforms still hide it. This offers an explicit request, not an automatic
  prompt and not a guarantee that macOS will change an existing grant.
- Focused component suite: 13 tests passed after the new authorized-state test
  first failed because the button was absent. Both changed settings files and
  `permission.rs` returned no LSP errors.
- Production component in isolated WebKit, mocked only at Tauri IPC: mount and
  focus produced two status reads and zero permission requests. One trusted
  click produced one request followed by one additional status read. No
  JavaScript errors or horizontal overflow occurred at 1280x1000.
- [Dark capture](evidence/notification-fixes/settings-authorized-recovery.png)
  and [light capture](evidence/notification-fixes/settings-authorized-recovery-light.png)
  use the actual runtime `data-theme` styles. Both independent final visual
  reviewers passed the corrected pair; the first pair used the wrong fixture
  theme selector and was replaced before acceptance.

## Click navigation runtime evidence

An isolated WebKit page imported the production
`subscribeNotificationActivations`, `workspaceReducer`, and
`selectGlobalUnreadBadgeCount` modules, with only the activation IPC replaced
by a deterministic queue. A startup target selected a parked worktree's exact
left pane: listener registration preceded the drain, the badge count changed
from 2 to 1, the sibling remained unseen, the sessions object was preserved,
and the outgoing worktree was parked rather than destroyed. A closed-session
action returned the same state with no navigation. This verifies module
behavior in a browser, not an OS click or the complete desktop App shell.

## Native manual verification boundary

Desktop execution must use exactly `bun tauri dev`, debug only. No installed
application, daemon, OS settings, Dock UI, or desktop input was changed by this
follow-up. The previously inspected running application was the installed
`/Applications/Ferryx.app`, not the edited debug build. Source changes alone do
not update that running application.

After running the updated debug build, manually check macOS System Settings >
Notifications > Ferryx, including the badge setting. Enable notifications in
Ferryx, keep two agent panes unobserved, and let both enter waiting/done. Check
that the Dock count appears; click one newly generated notification and verify
the exact project, worktree, tab group, and pane are selected, with the other
pane still unread. Repeat from a minimized window, with a different sibling
expanded, and after closing the original target pane. A closed target must not
spawn a replacement or select another pane. Notifications sent before target
metadata was added do not acquire that metadata retroactively.

## Integration status

Badge authorization and notification-click routing are implemented. Each
notification carries its owning workspace and frontend session, rather than a
backend PTY ID. macOS carries this target in native content userInfo and routes
the default response through a retained notification-center delegate. Windows
and Linux use notify-rust 4.18.0 response handling on a separate thread. Dismiss
responses and notifications without a destination do not navigate.

The native callback enqueues the target before emitting a wake event and
schedules window activation on the main thread. The frontend registers before
its startup drain, serializes subsequent drains, and ignores stale projects
and closed sessions. Cross-project navigation waits for the target workspace
state. The reducer selects the owning worktree, tab group, tab, and pane, reveals
the target if a sibling was expanded, and acknowledges only that pane. It does
not spawn a replacement terminal or discard sibling sessions.

## Final verification

All commands below completed with exit code 0:

```bash
CI=1 bun run --cwd ui test src/lib/notificationActivation.test.ts src/lib/notificationCoordinator.test.ts src/state/workspaceStore.test.tsx src/App.notifications.test.tsx src/App.test.tsx src/components/settings/NotificationsSection.test.tsx
bun run --cwd ui build
cargo test --offline --manifest-path src-tauri/Cargo.toml --lib notification -- --test-threads=1
cargo build --offline --manifest-path src-tauri/Cargo.toml
cargo check --offline --manifest-path .omo/notification-click-check/Cargo.toml --target x86_64-pc-windows-gnu --tests
cargo check --offline --manifest-path .omo/notification-click-check/Cargo.toml --target x86_64-unknown-linux-gnu --tests
```

- Frontend: 6 files, 198 tests passed in one final run. This includes startup
  and live clicks, serialized drains, stale targets, mounted cross-project
  selection, sibling attention preservation, and permission recovery.
- Frontend build: TypeScript and Vite passed; Vite completed in 3.16 seconds.
- Rust: 123 notification tests passed, including native macOS content
  encoding/decoding and default-versus-dismiss routing. The full macOS debug
  application build passed, with the build phase completing in 3m 42s.
- Windows and Linux: the production model, activation queue, and notify-rust
  adapter compiled with their tests through a local isolated harness. This is
  cross-target type-check evidence, not full platform builds or execution.
  The harness points directly at production source files and pins notify-rust
  to 4.18.0; it does not copy or replace the adapter implementation.
- A Windows compile failure exposed that notify-rust does not export
  NotificationHandle on that target. Keeping the inferred handle in the submit
  function removed the invalid public-type dependency; both targets then passed.
- The strengthened App cross-project test initially timed out because its
  runtime mock recreated registration-effect callbacks every render. Stable
  callbacks removed that artificial effect loop. React act now settles the
  mount and click updates without polling or larger timeouts; all 11 App
  notification tests passed before the final 198-test run.
- Final LSP error checks were clean for App, its notification test, activation
  subscription and tests, coordinator, and workspace store. Five Rust LSP
  requests timed out waiting for fresh diagnostics; successful Rust compiler
  checks above are the available evidence instead. `git diff --check` passed.

Native OS delivery, Dock pixels, actual notification-body clicks, minimized
window activation, and Windows/Linux runtime behavior remain manual acceptance
checks, not verified outcomes. If the prescribed `bun tauri dev` runtime reports
that it lacks a macOS bundle identity, native delivery is blocked in that
runtime; do not treat component success as an OS pass or substitute a release
build. No desktop application was launched or replaced for this follow-up.

## Commit status

- `d1d39de`: badge permission and explicit recovery, with tests and captures.
- `faad966`: notification-click routing and its tests. The staged scope check
  immediately before the commit contained 21 notification files, but the actual
  commit contains 27 files. Six files from concurrent terminal work were also
  included through the shared index:
  - `docs/NATIVE_TERMINAL_WORKSPACE_RETURN_FIX_2026-09-05.md`
  - `docs/NATIVE_TERMINAL_WORKSPACE_RETURN_INVESTIGATION_2026-09-05.md`
  - `src-tauri/src/ipc/native_terminal.rs`
  - `src-tauri/src/native_terminal/surface_host.rs`
  - `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`
  - `ui/src/components/NativeTerminalPane.tsx`

This is not the requested isolated notification commit. No history was rewritten
and no other session work was reverted. Splitting `faad966` requires explicit
approval to rewrite that local commit while preserving all file contents. This
report remains uncommitted pending that decision. No push was performed.
