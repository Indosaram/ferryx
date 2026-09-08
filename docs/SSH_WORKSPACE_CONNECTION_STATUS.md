# SSH workspace connection status

## Change

SSH workspace initialization now takes precedence over the empty-tab screen.
The main content shows a spinner, host label, and the current stage:
connecting via SSH, restoring the workspace, or opening the first terminal.
The pending first-tab operation is tracked until its promise settles, including
sidebar root selection, queued remote selection, and manual terminal creation.

Failed registration or first-tab creation stops the spinner and exposes the
structured error code/message with a Retry connection button. Retry repeats the
failed operation. Completed empty workspaces still show the original empty-tab
screen; existing tab surfaces are not replaced just for background registration.
Operation identity and workspace ownership prevent late results from updating
the selected project's connection status.

Raw SSH log streaming is not included. Short phase text is sufficient for normal
connection feedback; raw logs would need a separate backend progress interface.

## Verification

- Added four deferred-promise regression tests to `ui/src/App.remote.test.tsx`.
  They cover registration/restore/first-tab delays, first-tab failure and retry,
  registration retry, and late registration failure after switching locally.
- RED: the original App fails all four because `ssh-workspace-status` is absent.
- GREEN: all four pass with the change. No sleeps or polling were added.
- `bun run --cwd ui build`: exit 0, TypeScript and Vite build passed.
- Expanded run across App, SSH App, notifications, workspace runtime, and restore:
  144 passed, 6 failed, 150 total.
- A temporary Vitest loader supplied `HEAD:ui/src/App.tsx` without changing the
  working tree. All six existing failures reproduced with the old App:
  - Native signed-update startup callback is not called.
  - Native remote selection does not call the expected worktree activation.
  - Queued cross-project leaf selection does not call expected pane focus.
  - The project chooser test does not call the registration spy.
  - Canonical SSH project equality differs on host/branch metadata.
  - Inactive-root selection searches for an obsolete `build SSH` button.
  These were left unchanged. Other sessions were editing SSH metadata and sidebar
  code in the shared tree during verification.
- LSP diagnostics could not run: the local LSP daemon socket was unreachable.
  TypeScript checking succeeded through the build.
- The UI detector reported no findings for `SshWorkspaceStatus.tsx`.

## Browser evidence and limits

Rendered the production status component with the project's Tailwind theme in an
isolated Bun WebView, not the user's desktop app:

- `evidence/ssh-status-desktop.png`: loading at 1000 x 700.
- `evidence/ssh-status-mobile-error.png`: failure at 390 x 844, with a long host label.
- `evidence/ssh-status-mobile-retry.png`: spinner after clicking Retry connection.

Browser inspection confirmed flex layout, `animation-name: spin`, the failure
message and retry button, and `aria-busy=true` after retry. The 390px viewport
reported `scrollWidth=390`, with no horizontal overflow. Reduced motion is handled
by `motion-reduce:animate-none`.

The lead image tool reported that this model cannot receive images. A separate
visual review failed before execution because its provider OAuth token was
invalid. Screenshots exist, but a visual pass is therefore not claimed.
No real SSH connection or native desktop E2E was performed. The temporary harness,
baseline loader, browser instances, and server were removed or closed.

## Manual desktop check

Using the debug app launched with `bun tauri dev`, select an SSH root that takes
time to connect. Confirm that progress remains visible until the terminal opens,
without a flash of No open tabs. Check a failed connection and Retry connection,
then switch to a local project during an SSH attempt and confirm its result does
not replace the local screen.

The connection-status fix is scoped separately from concurrent SSH metadata,
worktree-management, and sidebar changes in the shared working tree.
