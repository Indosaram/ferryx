# SSH agent activity indicators

## Scope and result

The standalone SSH worktree row bypassed the common worktree activity indicator.
Its project header displayed only the running count. The fix reuses `StatusDot`
and `resolveActivityIndicator` for the SSH row and shows a working spinner on the
project header when there is no higher-priority attention state. The header
indicator remains visible when the project is collapsed.

Waiting and unseen completion take priority over working, including when another
agent in the project is still working. Existing notification and pane-highlight
consumers are unchanged.

This session changed the activity wiring in `ui/src/components/Sidebar.tsx` and
added:

- `ui/src/components/Sidebar.sshActivity.test.tsx`
- `ui/src/state/sshActivity.integration.test.tsx`

Other sessions concurrently changed SSH labels, worktree management, project
dialogs, registration, and connection status UI. Those changes were preserved.
The full working-tree diff is not attributable to this activity fix.

## Automated evidence

The four new sidebar regression cases first failed because the state glyph was
absent. After the fix, all four passed for POSIX and Windows paths, collapsed
headers, and attention precedence.

The following command completed with exit code 0: 6 files, 54 tests passed.

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/state/sshActivity.integration.test.tsx \
  src/components/Sidebar.sshActivity.test.tsx \
  src/components/Sidebar.activity.test.tsx \
  src/state/workspaceNativeActivity.test.tsx \
  src/lib/notificationCoordinator.test.ts \
  src/lib/sshAgentState.test.ts --maxWorkers=1
```

The new integration cases mount the real workspace store, Sidebar,
TerminalSplitView, TerminalPane, and NotificationCoordinator. Only the native
terminal surface and native IPC boundaries are substituted. They deliver events
using a backend session ID in an SSH workspace and verify:

- Working appears in both sidebar and tab spinners.
- Blocked maps to waiting; idle after working maps to completion.
- A background-window transition invokes the notification IPC with the correct
  frontend workspace/session navigation target and invokes its sound path once.
- The real pane attention frame appears and the unread badge count becomes one.
- Duplicate state reports do not repeat the notification.
- Acknowledging the session clears the frame.
- Both `/srv/repo` and `C:\work\repo` paths follow the same pipeline.

The existing socket test also executes the bundled extension against a real
ephemeral loopback TCP listener and checks the authenticated report and the
agent-generated provider session reference. This is transport-seam evidence,
not an external SSH-host end-to-end test.

## Browser evidence

An isolated Bun WebView rendered the real Sidebar at 1000 x 700. The private Vite
server used its own dependency cache to avoid interference with other sessions.
DOM and computed-style observations:

- Working produced two indicators with CSS animation name `spin`.
- An actual `animationiteration` event fired; the spinner was not merely marked
  with an unused CSS class.
- Waiting produced two `waiting` indicators; completion produced two `done`
  indicators.
- Collapsing the project removed its worktree row while retaining one working
  spinner on the header.
- The document had no horizontal overflow.

Captured PNGs:

- `docs/evidence/ssh-activity-working.png`
- `docs/evidence/ssh-activity-waiting.png`
- `docs/evidence/ssh-activity-done.png`
- `docs/evidence/ssh-activity-collapsed.png`

The current model could not visually inspect image attachments. These screenshots
are retained for human review; the browser verdict above is based on observed DOM,
geometry, computed styles, and animation events, not a claimed image review.

The private server and browser were stopped and this session's temporary harness
and baseline configuration files were removed.

## Verification boundaries

The LSP daemon was unreachable, so it did not provide diagnostics. The final
`bun run --cwd ui build` passed with exit code 0: TypeScript compilation succeeded
and Vite transformed 1873 modules, completing in 2.13 seconds.

During concurrent editing, two earlier build attempts encountered incomplete
ProjectDialogs JSX and then unrelated SSH-target type-narrowing errors. Other
sessions subsequently corrected those source locations.

Two earlier Sidebar.remote grouping tests also failed with the unmodified HEAD
Sidebar loaded through a temporary in-memory Vite loader. This established that
those failures were not caused by the activity-indicator patch. The final current
`Sidebar.remote.test.tsx` run passed all 10 tests with exit code 0 after the
concurrent work landed. The activity suite and compatibility suite therefore
passed 64 tests across their two final runs.

No live user SSH session was driven, no desktop app was launched, and no daemon
was restarted. Actual OS notification delivery and native compositor appearance
remain manual desktop checks rather than claims inferred from the IPC tests.

## Manual desktop check

Use the debug application through `bun tauri dev`:

1. Start work in an existing SSH agent pane. Check the SSH row and its project
   header show a spinner; collapse the project and check the header keeps it.
2. Move focus away from Ferryx. Let the agent request input or finish. With
   notifications enabled, verify the OS notification arrives.
3. Return to Ferryx. Check the waiting/completion indicator and pane attention
   frame, then focus the pane and verify the attention clears.

At implementation handoff, changes were uncommitted in the shared working tree.
The activity fix is isolated from concurrent SSH label and worktree-management
edits for its commit. Screenshots show the combined working-tree UI at verification
time, not the isolated commit's unrelated row labels.
